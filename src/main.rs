//! A simple gdbserver implementation for RISC-V trace files.

mod cheriot_ibex_trace;
mod cpu;
mod gdb;
mod ibex_trace;
mod logging;
mod machine;
mod mem_sniffer;
mod memory;
mod riscv;
mod trace;

use anyhow::Context as _;
use anyhow::bail;
use gdbstub::common::Signal;
use gdbstub::conn::Connection;
use gdbstub::conn::ConnectionExt;
use gdbstub::stub::DisconnectReason;
use gdbstub::stub::GdbStub;
use gdbstub::stub::SingleThreadStopReason;
use gdbstub::stub::run_blocking;
use gdbstub::target::Target;
use log::error;
use log::info;
use std::marker::PhantomData;
use std::net::TcpListener;
use std::net::TcpStream;
#[cfg(unix)]
use std::os::unix::net::UnixListener;
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
use std::path::Path;
use tokio::io::AsyncReadExt as _;
use tokio::io::AsyncWriteExt as _;
use tokio::process::Command;
use tokio::select;
use tokio::sync::watch;
use tokio::sync::watch::Receiver;
use tokio::sync::watch::Sender;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

use crate::riscv::RiscvArch;
use crate::riscv::RiscvArch32;
use crate::riscv::RiscvArch64;

fn wait_for_tcp(port: u16) -> Result<TcpStream> {
    let sockaddr = format!("127.0.0.1:{}", port);
    info!("Waiting for a GDB connection on {:?}...", sockaddr);

    let sock = TcpListener::bind(sockaddr)?;
    let (stream, addr) = sock.accept()?;
    info!("Debugger connected from {}", addr);

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

    info!("Waiting for a GDB connection on {}...", path.display());

    let sock = UnixListener::bind(path)?;
    let (stream, addr) = sock.accept()?;
    info!("Debugger connected from {:?}", addr);

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
        // We can use the same poll-based model to check for interrupt events
        // as gdbstub's `armv4t` example. See that example for a more detailed comment.
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

    /// Path to a vanilla Ibex trace file.
    #[arg(long, value_name = "TRACE_FILE")]
    ibex_trace: Option<PathBuf>,

    /// Path to a Cheriot-Ibex trace file.
    #[arg(long, value_name = "TRACE_FILE")]
    cheriot_ibex_trace: Option<PathBuf>,

    /// Path to a waves file to open with Surfer (VCD or FST).
    #[arg(long, value_name = "WAVE_FILE")]
    waves: Option<PathBuf>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    logging::init_logging()?;

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
        info!("32-bit ELF");
        main_impl::<RiscvArch32>(args, elf).await
    } else {
        info!("32-bit ELF");
        main_impl::<RiscvArch64>(args, elf).await
    }
}

async fn main_impl<A: RiscvArch>(args: Args, elf: Vec<u8>) -> Result<()> {
    let (send_time, receive_time) = watch::channel(0);

    if let Some(waves) = &args.waves {
        let waves = waves.to_owned();
        // Start the task to spawn Surfer and connect to us.
        tokio::task::spawn(async {
            if let Err(e) = main_waves(waves, receive_time).await {
                error!("{e:?}");
            }
        });
    }

    tokio::task::spawn_blocking(move || main_gdb::<A>(args, elf, send_time)).await??;

    Ok(())
}

fn main_gdb<A: RiscvArch>(args: Args, elf: Vec<u8>, send_time: Sender<u64>) -> Result<()> {
    let trace = match (args.ibex_trace, args.cheriot_ibex_trace) {
        (Some(path), None) => ibex_trace::read_trace(&path),
        (None, Some(path)) => cheriot_ibex_trace::read_trace(&path),
        _ => bail!("Please provide exactly one trace file."),
    }?;

    let mut done = false;

    // TODO: I think it's possible to make this async using run_state_machine.

    while !done {
        done = true;

        let mut machine = machine::Machine::new(elf.clone(), trace.clone(), send_time.clone())?;

        let connection: Box<dyn ConnectionExt<Error = std::io::Error>> = match &args.uds {
            Some(uds_path) => {
                #[cfg(not(unix))]
                {
                    return Err("Unix Domain Sockets can only be used on Unix".into());
                }
                #[cfg(unix)]
                {
                    Box::new(wait_for_uds(uds_path)?)
                }
            }
            None => Box::new(wait_for_tcp(9001)?),
        };

        let gdb = GdbStub::new(connection);

        match gdb.run_blocking::<TraceGdbEventLoop<A>>(&mut machine) {
            Ok(disconnect_reason) => match disconnect_reason {
                // VSCode's "Restart" is really disconnect and reattach
                // for remote connections. In that case we'll just start from
                // scratch so it really is like restarting. Bit of a hack but eh.
                DisconnectReason::Disconnect => {
                    println!("GDB client has disconnected. Restarting...");
                    done = false;
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
    }

    Ok(())
}

async fn main_waves(waves: PathBuf, mut receive_time: Receiver<u64>) -> Result<()> {
    // Start TCP server on random port.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();

    // Verify we can run surfer successfully.
    let child = Command::new("surfer")
        .arg("--version")
        .status()
        .await
        .context("running `surfer --version`")?;
    if !child.success() {
        bail!("`surfer --version` returned non-zero exit code");
    }

    // Run the surfer process.
    let mut child = Command::new("surfer")
        .arg("--wcp-initiate")
        .arg(port.to_string())
        .arg(waves)
        .spawn()?;

    // Accept a connection.
    let (mut socket, _) = listener.accept().await?;

    // TODO: Use concat_bytes when stable.
    socket
        .write_all(
            br#"{
    "type": "greeting",
    "version": "0",
    "commands": ["cursor_set", "set_viewport_to"]
}"#,
        )
        .await?;
    socket.write_all(&[0]).await?;

    let mut read_buf = [0u8; 4096];

    // Listen for incoming data (which is discarded), receive_time events,
    // and for surfer to exit.
    loop {
        select! {
            read = socket.read(&mut read_buf) => {
                let n = read?;
                if n == 0 {
                    break; // connection closed
                }
                // discard received data

                // TODO: Ideally we would process the responses.
            }
            changed = receive_time.changed() => {
                changed?;
                let time = *receive_time.borrow_and_update();

                // Move cursor.
                let message = format!(r#"{{
    "type": "command",
    "command": "cursor_set",
    "timestamp": {time}
}}"#);
                socket.write_all(message.as_bytes()).await?;
                socket.write_all(&[0]).await?;

                // Centre viewport.
                let message = format!(r#"{{
    "type": "command",
    "command": "set_viewport_to",
    "timestamp": {time}
}}"#);
                socket.write_all(message.as_bytes()).await?;
                socket.write_all(&[0]).await?;
            }
            status = child.wait() => {
                // Surfer process exited.
                let _status = status?;
                return Ok(());
            }
        }
    }

    Ok(())
}
