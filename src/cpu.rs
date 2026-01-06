use std::collections::HashMap;

use num_traits::Num;

use crate::{
    memory::Memory,
    trace::{Data, RetireEvent},
};

#[derive(Debug, Default, Clone)]
pub enum Privilege {
    #[default]
    Machine,
    Supervisor,
    User,
}

/// RISC-V CPU state
#[derive(Default, Debug, Clone)]
pub struct Cpu<Usize: Num> {
    pub pc: Usize,
    pub privilege: Privilege,

    pub xregs: [Usize; 32],
    // TODO: But float registers could be larger.
    pub fregs: [Usize; 32],
    // TODO: Vector regs.
    pub csrs: HashMap<u16, Usize>,
}

impl<Usize: Num + Copy> Cpu<Usize> {
    // Perform a trace step, and fill in the previous values in the event.
    pub fn step(&mut self, mem: &mut impl Memory, event: &mut RetireEvent<Usize>) {
        self.pc = event.pc;

        // X register write.
        if let Some(xwrite) = &mut event.xwrite {
            xwrite.prev_value = Some(self.xregs[xwrite.index as usize]);
            self.xregs[xwrite.index as usize] = xwrite.value;
        }

        // Memory store.
        if let Some(store) = &mut event.store {
            match store.value {
                Data::U8(val) => {
                    store.prev_value = Some(Data::U8(mem.r8(store.phys_addr)));
                    mem.w8(store.phys_addr, val);
                }
                Data::U16(val) => {
                    store.prev_value = Some(Data::U16(mem.r16(store.phys_addr)));
                    mem.w16(store.phys_addr, val);
                }
                Data::U32(val) => {
                    store.prev_value = Some(Data::U32(mem.r32(store.phys_addr)));
                    mem.w32(store.phys_addr, val);
                }
                Data::U64(val) => {
                    store.prev_value = Some(Data::U64(mem.r64(store.phys_addr)));
                    mem.w64(store.phys_addr, val);
                }
                Data::U128(val) => {
                    store.prev_value = Some(Data::U128(mem.r128(store.phys_addr)));
                    mem.w128(store.phys_addr, val);
                }
            }
        }
    }

    // Undo a step (i.e. step backwards).
    pub fn step_undo(
        &mut self,
        mem: &mut impl Memory,
        event: &RetireEvent<Usize>,
        prev_event: Option<&RetireEvent<Usize>>,
    ) {
        if let Some(prev_event) = prev_event {
            self.pc = prev_event.pc;
        }

        // X register write.
        if let Some(xwrite) = &event.xwrite
            && let Some(prev_val) = xwrite.prev_value
        {
            self.xregs[xwrite.index as usize] = prev_val;
        }

        // Memory store.
        if let Some(store) = &event.store
            && let Some(prev_val) = &store.prev_value
        {
            match prev_val {
                Data::U8(val) => {
                    mem.w8(store.phys_addr, *val);
                }
                Data::U16(val) => {
                    mem.w16(store.phys_addr, *val);
                }
                Data::U32(val) => {
                    mem.w32(store.phys_addr, *val);
                }
                Data::U64(val) => {
                    mem.w64(store.phys_addr, *val);
                }
                Data::U128(val) => {
                    mem.w128(store.phys_addr, *val);
                }
            }
        }
    }
}
