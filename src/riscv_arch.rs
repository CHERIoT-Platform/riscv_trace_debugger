use core::fmt::Debug;
use gdbstub::arch::Arch;
use gdbstub::internal::{BeBytes, LeBytes};
use gdbstub_arch::riscv::{Riscv32, Riscv64};
use num_traits::{FromPrimitive, PrimInt, Unsigned};

/// Extended version of `Arch` with more constraints and more types (in future probably).
pub trait RiscvArch {
    type Usize: Default + Clone + Debug + FromPrimitive + PrimInt + Unsigned + BeBytes + LeBytes;
    type BaseArch: Arch<Usize = Self::Usize>;
}

pub enum RiscvArch32 {}
pub enum RiscvArch64 {}

impl RiscvArch for RiscvArch32 {
    type Usize = u32;
    type BaseArch = Riscv32;
}

impl RiscvArch for RiscvArch64 {
    type Usize = u64;
    type BaseArch = Riscv64;
}
