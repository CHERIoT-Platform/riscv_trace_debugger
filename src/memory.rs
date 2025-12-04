use std::collections::HashMap;

pub trait Memory {
    /// Read a 8-bit value from `addr`
    fn r8(&mut self, addr: u64) -> u8;
    /// Read a 16-bit value from `addr`
    fn r16(&mut self, addr: u64) -> u16;
    /// Read a 32-bit value from `addr`
    fn r32(&mut self, addr: u64) -> u32;
    /// Read a 64-bit value from `addr`
    fn r64(&mut self, addr: u64) -> u64;

    /// Write a 8-bit `val` to `addr`
    fn w8(&mut self, addr: u64, val: u8);
    /// Write a 16-bit `val` to `addr`
    fn w16(&mut self, addr: u64, val: u16);
    /// Write a 32-bit `val` to `addr`
    fn w32(&mut self, addr: u64, val: u32);
    /// Write a 64-bit `val` to `addr`
    fn w64(&mut self, addr: u64, val: u64);
}

#[derive(Default, Clone)]
pub struct SimpleMemory(HashMap<u64, u8>);

impl Memory for SimpleMemory {
    fn r8(&mut self, addr: u64) -> u8 {
        *self.0.get(&addr).unwrap_or(&0)
    }

    fn r16(&mut self, addr: u64) -> u16 {
        self.r8(addr) as u16 | (self.r8(addr + 1) as u16) << 8
    }

    fn r32(&mut self, addr: u64) -> u32 {
        self.r16(addr) as u32 | (self.r16(addr + 2) as u32) << 16
    }

    fn r64(&mut self, addr: u64) -> u64 {
        self.r32(addr) as u64 | (self.r32(addr + 4) as u64) << 32
    }

    fn w8(&mut self, addr: u64, val: u8) {
        self.0.insert(addr, val);
    }

    fn w16(&mut self, addr: u64, val: u16) {
        self.w8(addr, val as u8);
        self.w8(addr + 1, (val >> 8) as u8);
    }

    fn w32(&mut self, addr: u64, val: u32) {
        self.w16(addr, val as u16);
        self.w16(addr + 2, (val >> 16) as u16);
    }

    fn w64(&mut self, addr: u64, val: u64) {
        self.w32(addr, val as u32);
        self.w32(addr + 4, (val >> 32) as u32);
    }
}
