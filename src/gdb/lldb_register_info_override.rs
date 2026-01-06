use std::num::NonZeroUsize;

use crate::gdb::Machine;
use crate::riscv::RiscvArch;
use gdbstub::arch::lldb::Encoding;
use gdbstub::arch::lldb::Format;
use gdbstub::arch::lldb::Register;
use gdbstub::target;
use gdbstub::target::ext::lldb_register_info_override::Callback;
use gdbstub::target::ext::lldb_register_info_override::CallbackToken;
use gdbstub_arch::riscv::reg::id::RiscvRegId;

fn riscv_regid_from_raw_id<U>(id: usize) -> Option<(RiscvRegId<U>, Option<NonZeroUsize>)> {
    let size = core::mem::size_of::<U>();

    let (id, size) = match id {
        0..=31 => (RiscvRegId::<U>::Gpr(id as u8), size),
        32 => (RiscvRegId::<U>::Pc, size),
        33..=64 => (RiscvRegId::<U>::Fpr((id - 33) as u8), size),
        65..=4160 => (RiscvRegId::<U>::Csr((id - 65) as u16), size),
        4161 => (RiscvRegId::<U>::Priv, 1),
        _ => return None,
    };

    Some((id, Some(NonZeroUsize::new(size)?)))
}

impl<A: RiscvArch> target::ext::lldb_register_info_override::LldbRegisterInfoOverride
    for Machine<A>
{
    fn lldb_register_info<'a>(
        &mut self,
        reg_id: usize,
        reg_info: Callback<'a>,
    ) -> Result<CallbackToken<'a>, Self::Error> {
        match riscv_regid_from_raw_id::<A::Usize>(reg_id) {
            Some((_, None)) | None => Ok(reg_info.done()),
            Some((r, Some(size))) => {
                let name: String = match r {
                    // For the purpose of demonstration, we end the qRegisterInfo packet exchange
                    // when reaching the Time register id, so that this register can only be
                    // explicitly queried via the single-register read packet.
                    RiscvRegId::Gpr(i) => format!("x{i}"),
                    _ => "unknown".into(),
                };
                let encoding = Encoding::Uint;
                let format = Format::Hex;
                let set = match r {
                    RiscvRegId::Gpr(_) => "General Purpose Registers",
                    RiscvRegId::Fpr(_) => "Floating Point Registers",
                    RiscvRegId::Pc => "Program Counter",
                    RiscvRegId::Csr(_) => "Control and Status Registers",
                    RiscvRegId::Priv => "Privilege Mode",
                    _ => "Unknown Registers",
                };
                let generic = match r {
                    // TODO
                    _ => None,
                };
                let reg = Register {
                    name: &name,
                    alt_name: None,
                    bitsize: (usize::from(size)) * 8,
                    offset: reg_id * (usize::from(size)),
                    encoding,
                    format,
                    set,
                    gcc: None,
                    dwarf: Some(reg_id),
                    generic,
                    container_regs: None,
                    invalidate_regs: None,
                };
                Ok(reg_info.write(reg))
            }
        }
    }
}
