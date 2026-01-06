use gdbstub::internal::LeBytes;
use gdbstub::target::{self, TargetResult};
use gdbstub_arch::riscv::reg::id::RiscvRegId;

use crate::{cpu::Privilege, machine::Machine, riscv::RiscvArch};

impl<A: RiscvArch> target::ext::base::single_register_access::SingleRegisterAccess<()>
    for Machine<A>
{
    fn read_register(
        &mut self,
        _tid: (),
        reg_id: RiscvRegId<u64>,
        buf: &mut [u8],
    ) -> TargetResult<usize, Self> {
        match reg_id {
            RiscvRegId::Gpr(reg_id) => {
                if let Some(reg_val) = self.cpu.xregs.get(reg_id as usize) {
                    reg_val.to_le_bytes(buf).ok_or(().into())
                } else {
                    Err(().into())
                }
            }
            RiscvRegId::Fpr(reg_id) => {
                if let Some(reg_val) = self.cpu.fregs.get(reg_id as usize) {
                    reg_val.to_le_bytes(buf).ok_or(().into())
                } else {
                    Err(().into())
                }
            }
            RiscvRegId::Pc => self.cpu.pc.to_le_bytes(buf).ok_or(().into()),
            RiscvRegId::Csr(reg_id) => {
                if let Some(reg_val) = self.cpu.csrs.get(&reg_id) {
                    reg_val.to_le_bytes(buf).ok_or(().into())
                } else {
                    Err(().into())
                }
            }
            RiscvRegId::Priv => {
                // TODO: What's the encoding here?
                let prv: u8 = match self.cpu.privilege {
                    Privilege::Machine => 3,
                    Privilege::Supervisor => 1,
                    Privilege::User => 0,
                };
                buf.copy_from_slice(&prv.to_le_bytes());
                Ok(buf.len())
            }
            _ => Err(().into()),
        }
    }

    fn write_register(
        &mut self,
        _tid: (),
        _reg_id: RiscvRegId<u64>,
        _val: &[u8],
    ) -> TargetResult<(), Self> {
        // Can't modify registers.
        Err(().into())
    }
}
