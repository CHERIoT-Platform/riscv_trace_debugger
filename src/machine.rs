use crate::cpu::Cpu;
use crate::mem_sniffer::AccessKind;
use crate::mem_sniffer::MemSniffer;
use crate::memory::Memory;
use crate::memory::SimpleMemory;
use crate::riscv::RiscvArch;
use crate::trace::TraceEvent;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use gdbstub::common::Signal;
use gdbstub::stub::SingleThreadStopReason;
use gdbstub::target::ext::tracepoints::NewTracepoint;
use gdbstub::target::ext::tracepoints::SourceTracepoint;
use gdbstub::target::ext::tracepoints::Tracepoint;
use gdbstub::target::ext::tracepoints::TracepointAction;
use gdbstub::target::ext::tracepoints::TracepointEnumerateState;
use log::info;
use num_traits::FromPrimitive as _;
use num_traits::ToPrimitive;
use std::collections::BTreeMap;
use tokio::sync::watch::Sender;
use tokio::task::yield_now;

pub enum ExecMode<A: RiscvArch> {
    Step,
    Continue,
    RangeStep(A::Usize, A::Usize),
}

#[derive(Copy, Clone)]
pub enum ExecDir {
    Forwards,
    Backwards,
}

#[derive(Debug)]
pub struct TraceFrame<A: RiscvArch> {
    pub number: Tracepoint,
    pub snapshot: Cpu<A::Usize>,
}

/// "Emulator" for RISC-V trace file. It reconstructs registers and
/// memory contents.
pub struct Machine<A: RiscvArch> {
    pub exec_mode: ExecMode<A>,
    pub exec_dir: ExecDir,

    pub cpu: Cpu<A::Usize>,
    pub mem: SimpleMemory,

    // The execution trace to use.
    pub trace: Vec<TraceEvent<A::Usize>>,
    pub trace_index: usize,

    // The ELF (needed so GDB can read it remotely).
    pub elf: Vec<u8>,

    pub watchpoints: Vec<A::Usize>,
    pub breakpoints: Vec<A::Usize>,
    pub files: Vec<Option<std::fs::File>>,

    pub tracepoints: BTreeMap<
        Tracepoint,
        (
            NewTracepoint<A::Usize>,
            Vec<SourceTracepoint<'static, A::Usize>>,
            Vec<TracepointAction<'static, A::Usize>>,
        ),
    >,
    pub traceframes: Vec<TraceFrame<A>>,
    pub tracepoint_enumerate_state: TracepointEnumerateState<A::Usize>,
    pub tracing: bool,
    pub selected_frame: Option<usize>,

    send_time: Sender<u64>,
}

impl<A: RiscvArch> Machine<A> {
    pub fn new(
        elf: Vec<u8>,
        trace: Vec<TraceEvent<A::Usize>>,
        send_time: Sender<u64>,
    ) -> Result<Machine<A>> {
        // set up emulated system
        let mut cpu = Cpu::<A::Usize>::default();
        let mut mem = SimpleMemory::default();

        let elf_header = goblin::elf::Elf::parse(&elf)?;

        // copy all in-memory sections from the ELF file into system RAM
        let sections = elf_header
            .section_headers
            .iter()
            .filter(|h| h.is_alloc() && h.sh_type != goblin::elf::section_header::SHT_NOBITS);

        // TODO: Initialise tags.

        for h in sections {
            info!(
                "loading section {:?} into memory from [{:#010x?}..{:#010x?}]",
                elf_header
                    .shdr_strtab
                    .get_at(h.sh_name)
                    .context("section name string access")?,
                h.sh_addr,
                h.sh_addr + h.sh_size,
            );

            for (i, b) in elf[h
                .file_range()
                .expect("No file range on section that isn't NOBITS")]
            .iter()
            .enumerate()
            {
                mem.w8(h.sh_addr + i as u64, *b);
            }
        }

        // setup execution state
        info!("Setting PC to {:#010x?}", elf_header.entry);
        cpu.pc = A::Usize::from_u64(elf_header.entry).ok_or_else(|| {
            anyhow!(
                "Couldn't convert ELF entry point to usize: {:#x}",
                elf_header.entry
            )
        })?;

        Ok(Machine {
            exec_mode: ExecMode::Continue,
            exec_dir: ExecDir::Forwards,

            cpu,
            mem,

            elf,

            trace,
            trace_index: 0,

            watchpoints: Vec::new(),
            breakpoints: Vec::new(),
            files: Vec::new(),

            tracepoints: BTreeMap::new(),
            traceframes: Vec::new(),
            tracepoint_enumerate_state: Default::default(),
            tracing: false,
            selected_frame: None,

            send_time,
        })
    }

    /// Single-step the interpreter. Returns None if it wasn't stopped (no breakpoint etc.).
    pub fn step(&mut self) -> Option<SingleThreadStopReason<A::Usize>> {
        if self.tracing {
            let frames: Vec<_> = self
                .tracepoints
                .iter()
                .filter(|(_tracepoint, (ctp, _source, _actions))| {
                    ctp.enabled && ctp.addr == self.cpu.pc
                })
                .map(|(tracepoint, _definition)| {
                    // our `tracepoint_define` restricts our loaded tracepoints to only contain
                    // register collect actions. instead of only collecting the registers requested
                    // in the register mask and recording a minimal trace frame, we just collect
                    // all of them by cloning the cpu itself.
                    TraceFrame {
                        number: *tracepoint,
                        snapshot: self.cpu.clone(),
                    }
                })
                .collect();
            self.traceframes.extend(frames);
        }

        let mut hit_watchpoint = None;

        let tmp = Vec::new();

        // TODO: Make MemSniffer generic? What about 34-bit physical addresses though?
        // let mut sniffer = MemSniffer::new(&mut self.mem, &self.watchpoints, |access| {
        //     hit_watchpoint = Some(access)
        // });

        let mut sniffer =
            MemSniffer::new(&mut self.mem, &tmp, |access| hit_watchpoint = Some(access));

        match self.exec_dir {
            ExecDir::Forwards => {
                if self.trace_index >= self.trace.len() {
                    return Some(SingleThreadStopReason::Terminated(Signal::SIGSTOP));
                }
                self.cpu
                    .step(&mut sniffer, &mut self.trace[self.trace_index]);
                self.trace_index += 1;
            }
            ExecDir::Backwards => {
                if self.trace_index == 0 {
                    // TODO: Double check this.
                    return Some(SingleThreadStopReason::DoneStep);
                }
                self.trace_index -= 1;
                let prev_event = if self.trace_index >= 1 && self.trace_index - 1 < self.trace.len()
                {
                    Some(&self.trace[self.trace_index - 1])
                } else {
                    None
                };
                self.cpu
                    .step_undo(&mut sniffer, &self.trace[self.trace_index], prev_event);
            }
        }

        if let Some(access) = hit_watchpoint {
            // TODO: I think this is setting PC back to the previous instruction,
            // but do we need to actually reverse instruction too?
            // Also seeing as we already know the access address I think we
            // can just check in advance if we'll hit the watchpoints without
            // even bothering with MemSniffer.

            // let fixup = if self.cpu.thumb_mode() { 2 } else { 4 };
            // self.cpu.pc = pc - fixup;

            todo!();

            // return Some(match access.kind {
            //     AccessKind::Read => Event::WatchRead(access.addr),
            //     AccessKind::Write => Event::WatchWrite(access.addr),
            // });
        }

        if self.breakpoints.contains(&self.cpu.pc) {
            return Some(SingleThreadStopReason::SwBreak(()));
        }

        None
    }

    /// Run the emulator in accordance with the currently set `ExecutionMode`.
    ///
    /// This will yield every 1024 steps to allow other things to run.
    ///
    /// Cancellation safety: This is cancellation safe. The only yield points
    /// are `yield_now()` and those happen before anything else.
    pub async fn run(&mut self) -> SingleThreadStopReason<A::Usize> {
        let event = match self.exec_mode {
            ExecMode::Step => self.step().unwrap_or(SingleThreadStopReason::DoneStep),
            ExecMode::Continue => {
                let mut cycles = 0;
                loop {
                    // TODO: Profile an optimal value here. Lower values
                    // will lead to more CPU overhead but higher values
                    // will lead to increased latency.
                    if cycles % 1024 == 0 {
                        // Yield back to Tokio so other things can run.
                        yield_now().await;
                    }
                    cycles += 1;

                    if let Some(event) = self.step() {
                        break event;
                    };
                }
            }
            // just continue, but with an extra PC check
            ExecMode::RangeStep(start, end) => {
                let mut cycles = 0;
                loop {
                    // TODO: Profile an optimal value here. Lower values
                    // will lead to more CPU overhead but higher values
                    // will lead to increased latency.
                    if cycles % 1024 == 0 {
                        // Yield back to Tokio so other things can run.
                        yield_now().await;
                    }
                    cycles += 1;

                    if let Some(event) = self.step() {
                        break event;
                    };

                    if !(start..end).contains(&self.cpu.pc) {
                        break SingleThreadStopReason::DoneStep;
                    }
                }
            }
        };

        // Update the time. Ignore errors.
        if let Some(event) = self.trace.get(self.trace_index) {
            let _ = self.send_time.send(event.time);
        }

        event
    }
}
