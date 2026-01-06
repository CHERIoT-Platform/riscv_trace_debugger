use crate::machine::Machine;
use crate::riscv::RiscvArch;

use super::copy_range_to_buf;
use gdbstub::common::Pid;
use gdbstub::target;
use gdbstub::target::TargetResult;

// Fake path for the ELF that is on the target so GDB can remotely access it.
pub const FAKE_ELF_FILENAME: &[u8; 9] = b"/test.elf";

impl<A: RiscvArch> target::ext::exec_file::ExecFile for Machine<A> {
    fn get_exec_file(
        &self,
        _pid: Option<Pid>,
        offset: u64,
        length: usize,
        buf: &mut [u8],
    ) -> TargetResult<usize, Self> {
        Ok(copy_range_to_buf(FAKE_ELF_FILENAME, offset, length, buf))
    }
}
