pub struct RetireEvent<Usize> {
    pub time: u64,
    pub cycle: u64,
    pub pc: Usize,
    pub instruction: u32,
    pub assembly_mnemonic: String,
    pub assembly_args: String,
    pub xwrite: Option<XRegWrite<Usize>>,
    pub store: Option<MemWrite>,
}

pub struct XRegWrite<Usize> {
    pub index: u8,
    pub value: Usize,
    pub prev_value: Option<Usize>,
}

pub struct MemWrite {
    pub phys_addr: u64,
    pub value: Data,
    pub prev_value: Option<Data>,
}

pub enum Data {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
}
