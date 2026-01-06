use crate::cpu::Cpu;
use crate::mem_sniffer::AccessKind;
use crate::mem_sniffer::MemSniffer;
use crate::memory::Memory;
use crate::memory::SimpleMemory;
use crate::riscv::RiscvArch;
use crate::trace::RetireEvent;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use gdbstub::target::ext::tracepoints::NewTracepoint;
use gdbstub::target::ext::tracepoints::SourceTracepoint;
use gdbstub::target::ext::tracepoints::Tracepoint;
use gdbstub::target::ext::tracepoints::TracepointAction;
use gdbstub::target::ext::tracepoints::TracepointEnumerateState;
use num_traits::FromPrimitive as _;
use num_traits::ToPrimitive;
use std::collections::BTreeMap;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Event {
    DoneStep,
    Halted,
    Break,
    WatchWrite(u64),
    WatchRead(u64),
}

pub enum ExecMode {
    Step,
    Continue,
    RangeStep(u64, u64),
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
    pub exec_mode: ExecMode,
    pub exec_dir: ExecDir,

    pub cpu: Cpu<A::Usize>,
    pub mem: SimpleMemory,

    // The execution trace to use.
    pub trace: Vec<RetireEvent<A::Usize>>,
    pub trace_index: usize,

    // The ELF (needed so GDB can read it remotely).
    pub elf: Vec<u8>,

    pub watchpoints: Vec<u64>,
    pub breakpoints: Vec<u64>,
    pub files: Vec<Option<std::fs::File>>,

    pub tracepoints: BTreeMap<
        Tracepoint,
        (
            NewTracepoint<u64>,
            Vec<SourceTracepoint<'static, u64>>,
            Vec<TracepointAction<'static, u64>>,
        ),
    >,
    pub traceframes: Vec<TraceFrame<A>>,
    pub tracepoint_enumerate_state: TracepointEnumerateState<u64>,
    pub tracing: bool,
    pub selected_frame: Option<usize>,
}

impl<A: RiscvArch> Machine<A> {
    pub fn new(elf: Vec<u8>, trace: Vec<RetireEvent<A::Usize>>) -> Result<Machine<A>> {
        // set up emulated system
        let mut cpu = Cpu::<A::Usize>::default();
        let mut mem = SimpleMemory::default();

        let elf_header = goblin::elf::Elf::parse(&elf)?;

        // copy all in-memory sections from the ELF file into system RAM
        let sections = elf_header
            .section_headers
            .iter()
            .filter(|h| h.is_alloc() && h.sh_type != goblin::elf::section_header::SHT_NOBITS);

        for h in sections {
            eprintln!(
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
        eprintln!("Setting PC to {:#010x?}", elf_header.entry);
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
        })
    }

    /// single-step the interpreter
    pub fn step(&mut self) -> Option<Event> {
        if self.tracing {
            let pc = self.cpu.pc.to_u64().expect("couldn't convert PC to u64");
            let frames: Vec<_> = self
                .tracepoints
                .iter()
                .filter(|(_tracepoint, (ctp, _source, _actions))| ctp.enabled && ctp.addr == pc)
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

        let mut sniffer = MemSniffer::new(&mut self.mem, &self.watchpoints, |access| {
            hit_watchpoint = Some(access)
        });

        match self.exec_dir {
            ExecDir::Forwards => {
                if self.trace_index >= self.trace.len() {
                    return Some(Event::Halted);
                }
                self.cpu
                    .step(&mut sniffer, &mut self.trace[self.trace_index]);
                self.trace_index += 1;
            }
            ExecDir::Backwards => {
                if self.trace_index == 0 {
                    // TODO: Double check this.
                    return Some(Event::DoneStep);
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

            return Some(match access.kind {
                AccessKind::Read => Event::WatchRead(access.addr),
                AccessKind::Write => Event::WatchWrite(access.addr),
            });
        }

        let pc = self.cpu.pc.to_u64().expect("couldn't convert PC to u64");

        if self.breakpoints.contains(&pc) {
            return Some(Event::Break);
        }

        None
    }

    /// run the emulator in accordance with the currently set `ExecutionMode`.
    ///
    /// since the emulator runs in the same thread as the GDB loop, the emulator
    /// will use the provided callback to poll the connection for incoming data
    /// every 1024 steps.
    pub fn run(&mut self, mut poll_incoming_data: impl FnMut() -> bool) -> RunEvent {
        match self.exec_mode {
            ExecMode::Step => RunEvent::Event(self.step().unwrap_or(Event::DoneStep)),
            ExecMode::Continue => {
                let mut cycles = 0;
                loop {
                    if cycles % 1024 == 0 {
                        // poll for incoming data
                        if poll_incoming_data() {
                            break RunEvent::IncomingData;
                        }
                    }
                    cycles += 1;

                    if let Some(event) = self.step() {
                        break RunEvent::Event(event);
                    };
                }
            }
            // just continue, but with an extra PC check
            ExecMode::RangeStep(start, end) => {
                let mut cycles = 0;
                loop {
                    if cycles % 1024 == 0 {
                        // poll for incoming data
                        if poll_incoming_data() {
                            break RunEvent::IncomingData;
                        }
                    }
                    cycles += 1;

                    if let Some(event) = self.step() {
                        break RunEvent::Event(event);
                    };

                    let pc = self.cpu.pc.to_u64().expect("couldn't convert PC to u64");

                    if !(start..end).contains(&pc) {
                        break RunEvent::Event(Event::DoneStep);
                    }
                }
            }
        }
    }
}

pub enum RunEvent {
    IncomingData,
    Event(Event),
}
