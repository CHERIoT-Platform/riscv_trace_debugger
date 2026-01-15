// Additional GDB extensions

mod auxv;
mod breakpoints;
mod exec_file;
mod host_io;
mod lldb_register_info_override;
mod monitor_cmd;
mod reverse;
mod single_register_access;
mod single_thread_range_step;
mod single_thread_single_step;
mod tracepoints;

use crate::machine::ExecMode;
use crate::machine::Machine;
use crate::memory::Memory as _;
use crate::riscv::RiscvArch;
use gdbstub::arch::Arch;
use gdbstub::common::Signal;
use gdbstub::target;
use gdbstub::target::Target;
use gdbstub::target::TargetError;
use gdbstub::target::TargetResult;
use gdbstub::target::ext::base::singlethread::SingleThreadBase;
use gdbstub::target::ext::base::singlethread::SingleThreadResume;
use num_traits::FromPrimitive;
use num_traits::ToPrimitive;

/// Copy all bytes of `data` to `buf`.
/// Return the size of data copied.
pub fn copy_to_buf(data: &[u8], buf: &mut [u8]) -> usize {
    let len = buf.len().min(data.len());
    buf[..len].copy_from_slice(&data[..len]);
    len
}

/// Copy a range of `data` (start at `offset` with a size of `length`) to `buf`.
/// Return the size of data copied. Returns 0 if `offset >= buf.len()`.
///
/// Mainly used by qXfer:_object_:read commands.
pub fn copy_range_to_buf(data: &[u8], offset: u64, length: usize, buf: &mut [u8]) -> usize {
    let offset = offset as usize;
    if offset > data.len() {
        return 0;
    }

    let start = offset;
    let end = (offset + length).min(data.len());
    copy_to_buf(&data[start..end], buf)
}

impl<A: RiscvArch> Target for Machine<A> {
    type Arch = A::BaseArch;
    type Error = &'static str;

    // --------------- IMPORTANT NOTE ---------------
    // Always remember to annotate IDET enable methods with `inline(always)`!
    // Without this annotation, LLVM might fail to dead-code-eliminate nested IDET
    // implementations, resulting in unnecessary binary bloat.

    #[inline(always)]
    fn base_ops(&mut self) -> target::ext::base::BaseOps<'_, Self::Arch, Self::Error> {
        target::ext::base::BaseOps::SingleThread(self)
    }

    #[inline(always)]
    fn support_breakpoints(
        &mut self,
    ) -> Option<target::ext::breakpoints::BreakpointsOps<'_, Self>> {
        Some(self)
    }

    #[inline(always)]
    fn support_monitor_cmd(&mut self) -> Option<target::ext::monitor_cmd::MonitorCmdOps<'_, Self>> {
        Some(self)
    }

    // #[inline(always)]
    // fn support_lldb_register_info_override(
    //     &mut self,
    // ) -> Option<target::ext::lldb_register_info_override::LldbRegisterInfoOverrideOps<'_, Self>> {
    //     Some(self)
    // }

    #[inline(always)]
    fn support_host_io(&mut self) -> Option<target::ext::host_io::HostIoOps<'_, Self>> {
        Some(self)
    }

    #[inline(always)]
    fn support_tracepoints(
        &mut self,
    ) -> Option<target::ext::tracepoints::TracepointsOps<'_, Self>> {
        Some(self)
    }
}

impl<A: RiscvArch> SingleThreadBase for Machine<A> {
    fn read_registers(
        &mut self,
        regs: &mut <Self::Arch as Arch>::Registers,
    ) -> TargetResult<(), Self> {
        // if we selected a frame from a tracepoint, return registers from that snapshot
        let cpu = self
            .selected_frame
            .and_then(|selected| self.traceframes.get(selected))
            .map(|frame| frame.snapshot.clone())
            .unwrap_or_else(|| self.cpu.clone());

        todo!();
        // regs.pc = cpu.pc;
        // regs.x = cpu.xregs;

        Ok(())
    }

    fn write_registers(
        &mut self,
        _regs: &<Self::Arch as Arch>::Registers,
    ) -> TargetResult<(), Self> {
        // Can't modify registers.
        Err(TargetError::NonFatal)
    }

    #[inline(always)]
    fn support_single_register_access(
        &mut self,
    ) -> Option<target::ext::base::single_register_access::SingleRegisterAccessOps<'_, (), Self>>
    {
        Some(self)
    }

    fn read_addrs(&mut self, start_addr: A::Usize, data: &mut [u8]) -> TargetResult<usize, Self> {
        if self.selected_frame.is_some() {
            // we only support register collection actions for our tracepoint frames.
            // if we have a selected frame, then we don't have any memory we can
            // return from the frame snapshot.
            return Ok(0);
        }

        let mut addr = start_addr;

        for val in data.iter_mut() {
            *val = self.mem.r8(addr.to_u64().unwrap());
            addr += A::Usize::from_u32(1).unwrap();
        }
        Ok(data.len())
    }

    fn write_addrs(&mut self, _start_addr: A::Usize, _data: &[u8]) -> TargetResult<(), Self> {
        // Can't modify memory.
        Err(TargetError::NonFatal)
    }

    #[inline(always)]
    fn support_resume(
        &mut self,
    ) -> Option<target::ext::base::singlethread::SingleThreadResumeOps<'_, Self>> {
        Some(self)
    }
}

impl<A: RiscvArch> SingleThreadResume for Machine<A> {
    fn resume(&mut self, signal: Option<Signal>) -> Result<(), Self::Error> {
        // Upon returning from the `resume` method, the target being debugged should be
        // configured to run according to whatever resume actions the GDB client has
        // specified (as specified by `set_resume_action`, `resume_range_step`,
        // `reverse_{step, continue}`, etc...)

        if signal.is_some() {
            return Err("no support for continuing with signal");
        }

        self.exec_mode = ExecMode::Continue;

        Ok(())
    }

    #[inline(always)]
    fn support_reverse_cont(
        &mut self,
    ) -> Option<target::ext::base::reverse_exec::ReverseContOps<'_, (), Self>> {
        Some(self)
    }

    #[inline(always)]
    fn support_reverse_step(
        &mut self,
    ) -> Option<target::ext::base::reverse_exec::ReverseStepOps<'_, (), Self>> {
        Some(self)
    }

    #[inline(always)]
    fn support_single_step(
        &mut self,
    ) -> Option<target::ext::base::singlethread::SingleThreadSingleStepOps<'_, Self>> {
        Some(self)
    }

    #[inline(always)]
    fn support_range_step(
        &mut self,
    ) -> Option<target::ext::base::singlethread::SingleThreadRangeSteppingOps<'_, Self>> {
        Some(self)
    }
}
