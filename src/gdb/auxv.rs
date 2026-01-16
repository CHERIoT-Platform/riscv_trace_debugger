use crate::machine::Machine;
use crate::riscv::RiscvArch;

use super::copy_range_to_buf;
use gdbstub::internal::LeBytes;
use gdbstub::target;
use gdbstub::target::TargetResult;
use num_traits::{FromPrimitive, Zero};

// Copied from LLVM. There are more but we don't need them.
const AUXV_AT_NULL: u8 = 0; // End of auxv.
// const AUXV_AT_IGNORE: u8 = 1;  // Ignore entry.
// const AUXV_AT_EXECFD: u8 = 2;  // File descriptor of program.
// const AUXV_AT_PHDR: u8 = 3;    // Program headers.
// const AUXV_AT_PHENT: u8 = 4;   // Size of program header.
// const AUXV_AT_PHNUM: u8 = 5;   // Number of program headers.
// const AUXV_AT_PAGESZ: u8 = 6;  // Page size.
// const AUXV_AT_BASE: u8 = 7;    // Interpreter base address.
// const AUXV_AT_FLAGS: u8 = 8;   // Flags.
const AUXV_AT_ENTRY: u8 = 9; // Program entry point.
// const AUXV_AT_NOTELF: u8 = 10; // Set if program is not an ELF.
// const AUXV_AT_UID: u8 = 11;    // UID.
// const AUXV_AT_EUID: u8 = 12;   // Effective UID.
// const AUXV_AT_GID: u8 = 13;    // GID.
// const AUXV_AT_EGID: u8 = 14;   // Effective GID.

fn append_auxv<Usize: FromPrimitive + LeBytes>(auxv: &mut Vec<u8>, typ: u8, val: Usize) {
    let sz = std::mem::size_of::<Usize>();
    let mut bytes = [0; 8];
    assert!(bytes.len() <= sz);

    // TODO: I *think* this should be host byte order but it's not totally
    // clear. Well hopefully nobody is mad enough to still be using Big Endian.
    Usize::from_u8(typ).unwrap().to_le_bytes(&mut bytes);
    auxv.extend_from_slice(&bytes[0..sz]);
    val.to_le_bytes(&mut bytes);
    auxv.extend_from_slice(&bytes[0..sz]);
}

impl<A: RiscvArch> target::ext::auxv::Auxv for Machine<A> {
    /// Get auxilliary data. See lldb/source/Plugins/Process/Utility/AuxVector.h in LLVM.
    ///
    /// The data is an array of (Usize, Usize) type/value pairs, ending with AUXV_AT_NULL.
    ///
    /// We just set the entry point to match the ELF because CHERIoT incorrectly sets
    /// the ELF EI_OSABI to ELFOSABI_LINUX and that causes LLDB to read AuxV to
    /// try to find the entry point and then calculate a load offset of the
    /// process entry point from the ELF entry point. If this process fails
    /// it doesn't load any segments at all.
    fn get_auxv(&self, offset: u64, length: usize, buf: &mut [u8]) -> TargetResult<usize, Self> {
        let mut auxv: Vec<u8> = Vec::new();

        append_auxv(&mut auxv, AUXV_AT_ENTRY, self.entry);
        append_auxv(&mut auxv, AUXV_AT_NULL, A::Usize::zero());

        Ok(copy_range_to_buf(&auxv, offset, length, buf))
    }
}
