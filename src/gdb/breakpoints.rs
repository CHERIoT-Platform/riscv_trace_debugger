use crate::machine::Machine;
use crate::riscv::RiscvArch;
use gdbstub::arch::Arch;
use gdbstub::target;
use gdbstub::target::TargetResult;
use gdbstub::target::ext::breakpoints::WatchKind;
use num_iter::range;

impl<A: RiscvArch> target::ext::breakpoints::Breakpoints for Machine<A> {
    #[inline(always)]
    fn support_sw_breakpoint(
        &mut self,
    ) -> Option<target::ext::breakpoints::SwBreakpointOps<'_, Self>> {
        Some(self)
    }

    #[inline(always)]
    fn support_hw_watchpoint(
        &mut self,
    ) -> Option<target::ext::breakpoints::HwWatchpointOps<'_, Self>> {
        Some(self)
    }
}

impl<A: RiscvArch> target::ext::breakpoints::SwBreakpoint for Machine<A> {
    fn add_sw_breakpoint(
        &mut self,
        addr: A::Usize,
        _kind: <A::BaseArch as Arch>::BreakpointKind,
    ) -> TargetResult<bool, Self> {
        self.breakpoints.push(addr);
        Ok(true)
    }

    fn remove_sw_breakpoint(
        &mut self,
        addr: A::Usize,
        _kind: <A::BaseArch as Arch>::BreakpointKind,
    ) -> TargetResult<bool, Self> {
        match self.breakpoints.iter().position(|x| *x == addr) {
            None => return Ok(false),
            Some(pos) => self.breakpoints.remove(pos),
        };

        Ok(true)
    }
}

impl<A: RiscvArch> target::ext::breakpoints::HwWatchpoint for Machine<A> {
    fn add_hw_watchpoint(
        &mut self,
        addr: A::Usize,
        len: A::Usize,
        kind: WatchKind,
    ) -> TargetResult<bool, Self> {
        for addr in range(addr, addr + len) {
            match kind {
                WatchKind::Write => self.watchpoints.push(addr),
                WatchKind::Read => self.watchpoints.push(addr),
                WatchKind::ReadWrite => self.watchpoints.push(addr),
            };
        }

        Ok(true)
    }

    fn remove_hw_watchpoint(
        &mut self,
        addr: A::Usize,
        len: A::Usize,
        kind: WatchKind,
    ) -> TargetResult<bool, Self> {
        for addr in range(addr, addr + len) {
            let pos = match self.watchpoints.iter().position(|x| *x == addr) {
                None => return Ok(false),
                Some(pos) => pos,
            };

            match kind {
                WatchKind::Write => self.watchpoints.remove(pos),
                WatchKind::Read => self.watchpoints.remove(pos),
                WatchKind::ReadWrite => self.watchpoints.remove(pos),
            };
        }

        Ok(true)
    }
}
