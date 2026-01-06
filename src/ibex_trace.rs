use std::{
    fs::File,
    io::{BufRead as _, BufReader},
    path::Path,
};

use anyhow::{Context, Result, anyhow, bail};
use num_traits::Num;

use crate::trace::{Data, MemWrite, RetireEvent, XRegWrite};

fn read_line<Usize: Num>(line: &str) -> Result<RetireEvent<Usize>> {
    let parts: Vec<&str> = line.split('\t').collect();

    if parts.len() < 4 {
        bail!(
            "expected at least 4 tab-separated values, got {}",
            parts.len()
        );
    }

    let time_str = parts[0].trim();
    let cycle_str = parts[1].trim();
    let pc_str = parts[2];
    let instruction_str = parts[3];

    let time =
        u64::from_str_radix(time_str, 10).with_context(|| format!("parsing {time_str:?}"))?;
    let cycle =
        u64::from_str_radix(cycle_str, 10).with_context(|| format!("parsing {cycle_str:?}"))?;
    let pc = Usize::from_str_radix(pc_str, 16).map_err(|_| anyhow!("parsing {pc_str:?}"))?;
    let instruction = u32::from_str_radix(instruction_str, 16)
        .with_context(|| format!("parsing {instruction_str:?}"))?;

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
                if store_val.is_some() {
                    bail!("Multiple stores found");
                }
                store_val =
                    Some(u64::from_str_radix(val, 16).with_context(|| format!("parsing {val:?}"))?);
            } else if let Some(val) = part.strip_prefix("PA:0x") {
                if phys_addr.is_some() {
                    bail!("Multiple PAs found");
                }
                phys_addr =
                    Some(u64::from_str_radix(val, 16).with_context(|| format!("parsing {val:?}"))?);
            } else {
                for index in 1..32 {
                    if let Some(val) = part.strip_prefix(&format!("x{index}=0x")) {
                        if xwrite.is_some() {
                            bail!("Multiple X writes found");
                        }
                        let value = Usize::from_str_radix(val, 16)
                            .map_err(|_| anyhow!("parsing {val:?}"))?;
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
        (Some(val), Some(phys_addr)) => {
            // Ibex uses the same number format for all stores so the only
            // way to get the size is by checking the instruction.
            // Fortunately all loads and stores supported by Ibex have
            // their width in the same place - bits [12,13].

            let value = match (instruction >> 12) & 0b11 {
                0 => Data::U8(
                    val.try_into()
                        .with_context(|| format!("parsing {val:#x} into 8 bits"))?,
                ),
                1 => Data::U16(
                    val.try_into()
                        .with_context(|| format!("parsing {val:#x} into 16 bits"))?,
                ),
                2 => Data::U32(
                    val.try_into()
                        .with_context(|| format!("parsing {val:#x} into 32 bits"))?,
                ),
                3 => Data::U64(
                    val.try_into()
                        .with_context(|| format!("parsing {val:#x} into 64 bits"))?,
                ),
                _ => unreachable!(),
            };

            Some(MemWrite {
                phys_addr,
                value,
                prev_value: None,
            })
        }
        (None, _) => None,
        (Some(_), None) => bail!("Store without PA"),
    };

    Ok(RetireEvent {
        time,
        cycle,
        pc,
        instruction,
        assembly_mnemonic: assembly_mnemonic.unwrap_or_default().to_owned(),
        assembly_args: assembly_args.unwrap_or_default().to_owned(),
        xwrite,
        store,
    })
}

pub fn read_trace<Usize: Num>(file_path: &Path) -> Result<Vec<RetireEvent<Usize>>> {
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);

    let mut events = Vec::new();

    for (line_number, line) in reader.lines().enumerate() {
        let line_number_plus_one = line_number + 1;
        let line = line.with_context(|| {
            format!(
                "reading line {}:{line_number_plus_one}",
                file_path.display()
            )
        })?;

        if line.starts_with("Time") {
            // Skip header.
            continue;
        }

        events.push(read_line(&line).with_context(|| {
            format!(
                "processing line {}:{line_number_plus_one}",
                file_path.display()
            )
        })?);
    }

    Ok(events)
}
