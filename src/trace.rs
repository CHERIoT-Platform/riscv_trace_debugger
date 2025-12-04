use std::{
    fs::File,
    io::{BufRead as _, BufReader},
    path::Path,
};

use anyhow::{Context, Result, anyhow, bail};
use num_traits::Num;

// Based on Ibex trace.
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

pub fn read_trace<Usize: Num>(file_path: &Path) -> Result<Vec<RetireEvent<Usize>>> {
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);

    let mut events = Vec::new();

    for (line_number, line) in reader.lines().enumerate() {
        let line = line?;
        let line_number_plus_one = line_number + 1;

        if line.starts_with("Time") {
            // Skip header.
            continue;
        }

        let parts: Vec<&str> = line.split('\t').collect();

        if parts.len() < 4 {
            bail!(
                "Invalid line; expected at least 4 tab-separated values, got {} in line {line_number_plus_one}: {line:?}",
                parts.len()
            );
        }

        let time_str = parts[0].trim();
        let cycle_str = parts[1].trim();
        let pc_str = parts[2];
        let instruction_str = parts[3];

        let time = u64::from_str_radix(time_str, 10).with_context(|| {
            format!("parsing {time_str:?} in line {line_number_plus_one}: {line:?}")
        })?;
        let cycle = u64::from_str_radix(cycle_str, 10).with_context(|| {
            format!("parsing {cycle_str:?} in line {line_number_plus_one}: {line:?}")
        })?;
        let pc = Usize::from_str_radix(pc_str, 16)
            .map_err(|_| anyhow!("parsing {pc_str:?} in line {line_number_plus_one}: {line:?}"))?;
        let instruction = u32::from_str_radix(instruction_str, 16).with_context(|| {
            format!("parsing {instruction_str:?} in line {line_number_plus_one}: {line:?}")
        })?;

        let assembly_mnemonic = parts.get(4).map(|s| s.to_owned());
        let assembly_args = parts.get(5).map(|s| s.to_owned());

        let accesses = parts.get(6).map(|s| s.to_owned());

        let mut phys_addr = None;
        let mut store_val = None;
        let mut xwrite = None;

        if let Some(accesses) = accesses {
            let access_parts = accesses.split_ascii_whitespace();

            for part in access_parts {
                if let Some(val) = part.strip_prefix("store:0x") {
                    // TODO: There's no way to get the size of the store but in the example they're all 32-bit.
                    if store_val.is_some() {
                        bail!("Multiple stores found");
                    }
                    store_val = Some(u32::from_str_radix(val, 16).with_context(|| {
                        format!("parsing {val:?} in line {line_number_plus_one}: {line:?}")
                    })?);
                } else if let Some(val) = part.strip_prefix("PA:0x") {
                    if phys_addr.is_some() {
                        bail!("Multiple PAs found");
                    }
                    phys_addr = Some(u64::from_str_radix(val, 16).with_context(|| {
                        format!("parsing {val:?} in line {line_number_plus_one}: {line:?}")
                    })?);
                } else {
                    for index in 1..32 {
                        if let Some(val) = part.strip_prefix(&format!("x{index}=0x")) {
                            if xwrite.is_some() {
                                bail!("Multiple X writes found");
                            }
                            let value = Usize::from_str_radix(val, 16).map_err(|_| {
                                anyhow!("parsing {val:?} in line {line_number_plus_one}: {line:?}")
                            })?;
                            xwrite = Some(XRegWrite {
                                index,
                                value,
                                prev_value: None,
                            });
                        }
                    }
                }
            }
        }

        let store = match (store_val, phys_addr) {
            // TODO: Get the store size from the trace.
            (Some(val), Some(phys_addr)) => Some(MemWrite {
                phys_addr,
                value: Data::U32(val),
                prev_value: None,
            }),
            (None, _) => None,
            (Some(_), None) => bail!("Store without PA in line {line_number_plus_one}: {line:?}"),
        };

        events.push(RetireEvent {
            time,
            cycle,
            pc,
            instruction,
            assembly_mnemonic: assembly_mnemonic.unwrap_or_default().to_owned(),
            assembly_args: assembly_args.unwrap_or_default().to_owned(),
            xwrite,
            store,
        });
    }

    Ok(events)
}
