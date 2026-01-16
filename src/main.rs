//! A simple gdbserver implementation for RISC-V trace files.

mod buffered_connection;
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
use gdbstub::stub::DisconnectReason;
use gdbstub::stub::GdbStub;
use gdbstub::stub::SingleThreadStopReason;
use gdbstub::stub::state_machine;
use log::error;
use log::info;

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

use crate::buffered_connection::BufferedConnection;
use crate::riscv::RiscvArch;
use crate::riscv::RiscvArch32;
use crate::riscv::RiscvArch64;
use crate::trace::TraceEvent;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    // TODO: Add UDS support back, maybe.
    // /// Use UNIX domain socket instead of TCP.
    // #[arg(long, value_name = "SOCKET_PATH")]
    // uds: Option<PathBuf>,
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
        info!("64-bit ELF");
        main_impl::<RiscvArch64>(args, elf).await
    } else {
        info!("32-bit ELF");
        main_impl::<RiscvArch32>(args, elf).await
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

    main_gdb::<A>(args, elf, send_time).await
}

async fn main_gdb<A: RiscvArch>(args: Args, elf: Vec<u8>, send_time: Sender<u64>) -> Result<()> {
    let trace: Vec<TraceEvent<A::Usize>> = match (args.ibex_trace, args.cheriot_ibex_trace) {
        (Some(path), None) => ibex_trace::read_trace(&path),
        (None, Some(path)) => cheriot_ibex_trace::read_trace(&path),
        _ => bail!("Please provide exactly one trace file."),
    }?;

    let mut done = false;

    while !done {
        done = true;

        let mut machine =
            machine::Machine::<A>::new(elf.clone(), trace.clone(), send_time.clone())?;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:9001").await?;

        // Accept a connection.
        let (mut socket, _) = listener.accept().await?;

        let connection = BufferedConnection::default();

        let gdb = GdbStub::new(connection);

        let mut gdb = gdb.run_state_machine(&mut machine)?;
        let disconnect_reason: Result<DisconnectReason> = loop {
            gdb = match gdb {
                state_machine::GdbStubStateMachine::Idle(mut gdb) => {
                    // Flush any data to be sent.
                    gdb.borrow_conn().flush(&mut socket).await?;

                    // Wait for data from the GDB client.
                    // TODO: What does read_u8 do on disconnection?
                    let byte = socket.read_u8().await?;
                    gdb.incoming_data(&mut machine, byte)?
                }

                state_machine::GdbStubStateMachine::Disconnected(mut gdb) => {
                    // Flush any data to be sent.
                    gdb.borrow_conn().flush(&mut socket).await?;

                    // We're going to restart the whole process on disconnection.
                    break Ok(gdb.get_reason());
                }

                state_machine::GdbStubStateMachine::CtrlCInterrupt(mut gdb) => {
                    // Flush any data to be sent.
                    gdb.borrow_conn().flush(&mut socket).await?;

                    // Stop on Ctrl-C.
                    let stop_reason = Some(SingleThreadStopReason::Signal(Signal::SIGINT));
                    gdb.interrupt_handled(&mut machine, stop_reason)?
                }

                state_machine::GdbStubStateMachine::Running(mut gdb) => {
                    // Flush any data to be sent.
                    gdb.borrow_conn().flush(&mut socket).await?;

                    // Wait for a byte from the client, and a break in the simulation.
                    select! {
                        // TODO: What does read_u8 do on disconnection?
                        byte = socket.read_u8() => {
                            gdb.incoming_data(&mut machine, byte?)?
                        }
                        stop_reason = machine.run() => {
                            gdb.report_stop(&mut machine, stop_reason)?
                        }
                    }
                }
            }
        };

        match disconnect_reason? {
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
    "commands": ["set_cursor", "set_viewport_to"]
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
    "command": "set_cursor",
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
