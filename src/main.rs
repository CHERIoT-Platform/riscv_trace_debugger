//! A simple gdbserver implementation for RISC-V trace files.

use anyhow::bail;
use gdbstub::common::Signal;
use gdbstub::conn::Connection;
use gdbstub::conn::ConnectionExt;
use gdbstub::stub::DisconnectReason;
use gdbstub::stub::GdbStub;
use gdbstub::stub::SingleThreadStopReason;
use gdbstub::stub::run_blocking;
use gdbstub::target::Target;
use std::marker::PhantomData;
use std::net::TcpListener;
use std::net::TcpStream;
#[cfg(unix)]
use std::os::unix::net::UnixListener;
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
use std::path::Path;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

use crate::riscv_arch::RiscvArch;
use crate::riscv_arch::RiscvArch32;
use crate::riscv_arch::RiscvArch64;

mod cpu;
mod gdb;
mod machine;
mod mem_sniffer;
mod memory;
mod riscv_arch;
mod trace;

fn wait_for_tcp(port: u16) -> Result<TcpStream> {
    let sockaddr = format!("127.0.0.1:{}", port);
    eprintln!("Waiting for a GDB connection on {:?}...", sockaddr);

    let sock = TcpListener::bind(sockaddr)?;
    let (stream, addr) = sock.accept()?;
    eprintln!("Debugger connected from {}", addr);

    Ok(stream)
}

#[cfg(unix)]
fn wait_for_uds(path: &Path) -> Result<UnixStream> {
    match std::fs::remove_file(path) {
        Ok(_) => {}
        Err(e) => match e.kind() {
            std::io::ErrorKind::NotFound => {}
            _ => return Err(e.into()),
        },
    }

    eprintln!("Waiting for a GDB connection on {}...", path.display());

    let sock = UnixListener::bind(path)?;
    let (stream, addr) = sock.accept()?;
    eprintln!("Debugger connected from {:?}", addr);

    Ok(stream)
}

struct TraceGdbEventLoop<A: RiscvArch> {
    _phantom: PhantomData<A>,
}

impl<A: RiscvArch> run_blocking::BlockingEventLoop for TraceGdbEventLoop<A> {
    type Target = machine::Machine<A>;
    type Connection = Box<dyn ConnectionExt<Error = std::io::Error>>;
    type StopReason = SingleThreadStopReason<u64>;

    #[allow(clippy::type_complexity)]
    fn wait_for_stop_reason(
        target: &mut machine::Machine<A>,
        conn: &mut Self::Connection,
    ) -> Result<
        run_blocking::Event<SingleThreadStopReason<u64>>,
        run_blocking::WaitForStopReasonError<
            <Self::Target as Target>::Error,
            <Self::Connection as Connection>::Error,
        >,
    > {
        // The `armv4t` example runs the emulator in the same thread as the GDB state
        // machine loop. As such, it uses a simple poll-based model to check for
        // interrupt events, whereby the emulator will check if there is any incoming
        // data over the connection, and pause execution with a synthetic
        // `RunEvent::IncomingData` event.
        //
        // In more complex integrations, the target will probably be running in a
        // separate thread, and instead of using a poll-based model to check for
        // incoming data, you'll want to use some kind of "select" based model to
        // simultaneously wait for incoming GDB data coming over the connection, along
        // with any target-reported stop events.
        //
        // The specifics of how this "select" mechanism work + how the target reports
        // stop events will entirely depend on your project's architecture.
        //
        // Some ideas on how to implement this `select` mechanism:
        //
        // - A mpsc channel
        // - epoll/kqueue
        // - Running the target + stopping every so often to peek the connection
        // - Driving `GdbStub` from various interrupt handlers

        let poll_incoming_data = || conn.peek().map(|b| b.is_some()).unwrap_or(true);

        match target.run(poll_incoming_data) {
            machine::RunEvent::IncomingData => {
                let byte = conn
                    .read()
                    .map_err(run_blocking::WaitForStopReasonError::Connection)?;
                Ok(run_blocking::Event::IncomingData(byte))
            }
            machine::RunEvent::Event(event) => {
                use gdbstub::target::ext::breakpoints::WatchKind;

                // translate emulator stop reason into GDB stop reason
                let stop_reason = match event {
                    machine::Event::DoneStep => SingleThreadStopReason::DoneStep,
                    machine::Event::Halted => SingleThreadStopReason::Terminated(Signal::SIGSTOP),
                    machine::Event::Break => SingleThreadStopReason::SwBreak(()),
                    machine::Event::WatchWrite(addr) => SingleThreadStopReason::Watch {
                        tid: (),
                        kind: WatchKind::Write,
                        addr,
                    },
                    machine::Event::WatchRead(addr) => SingleThreadStopReason::Watch {
                        tid: (),
                        kind: WatchKind::Read,
                        addr,
                    },
                };

                Ok(run_blocking::Event::TargetStopped(stop_reason))
            }
        }
    }

    // Called when Ctrl-C is sent to GDB. We can just exit.
    fn on_interrupt(
        _target: &mut machine::Machine<A>,
    ) -> Result<Option<SingleThreadStopReason<u64>>, <machine::Machine<A> as Target>::Error> {
        Ok(Some(SingleThreadStopReason::Signal(Signal::SIGINT)))
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Use UNIX domain socket instead of TCP.
    #[arg(long, value_name = "SOCKET_PATH")]
    uds: Option<PathBuf>,

    /// Path to the ELF file
    #[arg(long, value_name = "ELF_PATH")]
    elf: PathBuf,

    /// Path to the trace file
    #[arg(long, value_name = "TRACE_FILE")]
    trace: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let elf = std::fs::read(&args.elf)?;

    let elf_header = goblin::elf::Elf::parse(&elf)?;

    if !elf_header.little_endian {
        bail!(
            "ELF is Big Endian. Either something has gone horribly wrong and the file is corrupted or something has gone horribly wrong and you're using Big Endian in the 21st century."
        );
    }
    // Apparently this isn't a reliable check?

    // if !elf_header.header.e_machine != goblin::elf::header::EM_RISCV {
    //     bail!("Not a RISC-V ELF");
    // }

    if elf_header.is_64 {
        main_impl::<RiscvArch32>(args, elf)
    } else {
        main_impl::<RiscvArch64>(args, elf)
    }
}

fn main_impl<A: RiscvArch>(args: Args, elf: Vec<u8>) -> Result<()> {
    let trace = trace::read_trace(&args.trace)?;

    let mut machine = machine::Machine::new(elf, trace)?;

    let connection: Box<dyn ConnectionExt<Error = std::io::Error>> = match args.uds {
        Some(uds_path) => {
            #[cfg(not(unix))]
            {
                return Err("Unix Domain Sockets can only be used on Unix".into());
            }
            #[cfg(unix)]
            {
                Box::new(wait_for_uds(&uds_path)?)
            }
        }
        None => Box::new(wait_for_tcp(9001)?),
    };

    let gdb = GdbStub::new(connection);

    match gdb.run_blocking::<TraceGdbEventLoop<A>>(&mut machine) {
        Ok(disconnect_reason) => match disconnect_reason {
            DisconnectReason::Disconnect => {
                println!("GDB client has disconnected. Exiting...");
            }
            DisconnectReason::TargetExited(code) => {
                println!("Target exited with code {}!", code)
            }
            DisconnectReason::TargetTerminated(sig) => {
                println!("Target terminated with signal {}!", sig)
            }
            DisconnectReason::Kill => println!("GDB sent a kill command!"),
        },
        Err(e) => {
            if e.is_target_error() {
                println!(
                    "target encountered a fatal error: {}",
                    e.into_target_error().unwrap()
                )
            } else if e.is_connection_error() {
                let (e, kind) = e.into_connection_error().unwrap();
                println!("connection error: {:?} - {}", kind, e,)
            } else {
                println!("gdbstub encountered a fatal error: {}", e)
            }
        }
    }

    Ok(())
}
