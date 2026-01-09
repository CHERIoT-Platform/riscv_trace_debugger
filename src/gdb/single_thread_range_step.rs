use gdbstub::target;

use crate::{
    machine::{ExecDir, ExecMode, Machine},
    riscv::RiscvArch,
};

impl<A: RiscvArch> target::ext::base::singlethread::SingleThreadRangeStepping for Machine<A> {
    fn resume_range_step(&mut self, start: A::Usize, end: A::Usize) -> Result<(), Self::Error> {
        self.exec_mode = ExecMode::RangeStep(start, end);
        // TODO: Not totally sure about this but it's probably right based on `single_thread_single_step` requiring it.
        self.exec_dir = ExecDir::Forwards;

        Ok(())
    }
}
