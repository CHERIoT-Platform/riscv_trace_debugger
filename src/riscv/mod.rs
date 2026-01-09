mod reg;

use core::fmt::Debug;
use gdbstub::arch::Arch;
use gdbstub::internal::{BeBytes, LeBytes};
use num_traits::{FromPrimitive, PrimInt, Unsigned};

/// Extended version of `Arch` with more constraints (Usize: Default + Clone + Debug)
/// and more types (in future probably).
pub trait RiscvArch {
    type Usize: Default + Clone + Debug + FromPrimitive + PrimInt + Unsigned + BeBytes + LeBytes;
    type BaseArch: Arch<Usize = Self::Usize>;
}

pub enum RiscvArch32 {}
pub enum RiscvArch64 {}
pub enum RiscvCheriArch32 {}
pub enum RiscvCheriArch64 {}

impl RiscvArch for RiscvArch32 {
    type Usize = u32;
    type BaseArch = RiscvArch32;
}

impl RiscvArch for RiscvArch64 {
    type Usize = u64;
    type BaseArch = RiscvArch64;
}

impl RiscvArch for RiscvCheriArch32 {
    type Usize = u32;
    type BaseArch = RiscvCheriArch32;
}

impl RiscvArch for RiscvCheriArch64 {
    type Usize = u64;
    type BaseArch = RiscvCheriArch64;
}

impl Arch for RiscvArch32 {
    type Usize = u32;
    type Registers = reg::RiscvCoreRegs<u32>;
    type BreakpointKind = usize;
    type RegId = reg::id::RiscvRegId<u32>;

    fn target_description_xml() -> Option<&'static str> {
        Some(include_str!("rv32i.xml"))
    }
}

impl Arch for RiscvArch64 {
    type Usize = u64;
    type Registers = reg::RiscvCoreRegs<u64>;
    type BreakpointKind = usize;
    type RegId = reg::id::RiscvRegId<u64>;

    fn target_description_xml() -> Option<&'static str> {
        Some(include_str!("rv64i.xml"))
    }
}

impl Arch for RiscvCheriArch32 {
    type Usize = u32;
    type Registers = reg::RiscvCoreRegs<u32>;
    type BreakpointKind = usize;
    type RegId = reg::id::RiscvRegId<u32>;

    fn target_description_xml() -> Option<&'static str> {
        Some(include_str!("rv32y.xml"))
    }
}

impl Arch for RiscvCheriArch64 {
    type Usize = u64;
    type Registers = reg::RiscvCoreRegs<u64>;
    type BreakpointKind = usize;
    type RegId = reg::id::RiscvRegId<u64>;

    fn target_description_xml() -> Option<&'static str> {
        Some(include_str!("rv64y.xml"))
    }
}
