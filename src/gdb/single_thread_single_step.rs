use gdbstub::{common::Signal, target};

use crate::{
    machine::{ExecDir, ExecMode, Machine},
    riscv::RiscvArch,
};

impl<A: RiscvArch> target::ext::base::singlethread::SingleThreadSingleStep for Machine<A> {
    fn step(&mut self, signal: Option<Signal>) -> Result<(), Self::Error> {
        if signal.is_some() {
            return Err("no support for stepping with signal");
        }

        self.exec_mode = ExecMode::Step;
        self.exec_dir = ExecDir::Forwards;

        Ok(())
    }
}
