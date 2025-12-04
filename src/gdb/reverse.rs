use gdbstub::target;

use crate::{
    machine::{ExecDir, ExecMode, Machine},
    riscv_arch::RiscvArch,
};

// Reverse debugging support

impl<A: RiscvArch> target::ext::base::reverse_exec::ReverseCont<()> for Machine<A> {
    fn reverse_cont(&mut self) -> Result<(), Self::Error> {
        self.exec_mode = ExecMode::Continue;
        self.exec_dir = ExecDir::Backwards;
        Ok(())
    }
}

impl<A: RiscvArch> target::ext::base::reverse_exec::ReverseStep<()> for Machine<A> {
    fn reverse_step(&mut self, _tid: ()) -> Result<(), Self::Error> {
        self.exec_mode = ExecMode::Step;
        self.exec_dir = ExecDir::Backwards;
        Ok(())
    }
}
