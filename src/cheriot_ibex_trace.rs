use std::{
    fs::File,
    io::{BufRead as _, BufReader},
    path::Path,
};

use anyhow::{Context, Result, anyhow, bail};
use num_traits::Num;

use crate::trace::{Data, MemWrite, TraceEvent, XRegWrite};

/// Strip the '0x' hex prefix or return an error if it isn't present.
fn strip_hex_prefix(val: &str) -> Result<&str> {
    val.strip_prefix("0x")
        .ok_or_else(|| anyhow!("hex integer missing 0x prefix {val:?}"))
}

/// Parse a hex integer without an '0x' prefix.
fn parse_hex<U: Num>(val: &str) -> Result<U> {
    U::from_str_radix(val, 16).map_err(|_| anyhow!("invalid hex integer {val:?}"))
}

fn read_line<Usize: Num>(line: &str) -> Result<TraceEvent<Usize>> {
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
    let instruction_str = parts[3].trim();

    let time =
        u64::from_str_radix(time_str, 10).with_context(|| format!("parsing {time_str:?}"))?;
    let cycle =
        u64::from_str_radix(cycle_str, 10).with_context(|| format!("parsing {cycle_str:?}"))?;
    let pc = parse_hex(pc_str)?;
    let instruction = parse_hex(instruction_str)?;

    let assembly_mnemonic = parts.get(4).map(|s| s.to_owned());
    let assembly_args = parts.get(5).map(|s| s.to_owned());

    let accesses = parts.get(6).map(|s| s.to_owned());

    let mut phys_addr = None;
    let mut store_val = None;
    let mut xwrite = None;

    if let Some(accesses) = accesses {
        let access_parts = accesses.split_ascii_whitespace();

        for part in access_parts {
            if let Some(val) = part.strip_prefix("store:") {
                if store_val.is_some() {
                    bail!("Multiple stores found");
                }
                // For Cheriot-Ibex stores are like 0x????1234 for half
                // and if it's a capability store it's like 0x12345678+0x112345678
                // The second part is the metadata including the tag!
                store_val = Some(match val.split_once('+') {
                    // Capability stores are always XLEN, so we don't have to
                    // worry about ?s.
                    Some((data, metadata)) => {
                        let data = strip_hex_prefix(data)?;
                        let metadata = strip_hex_prefix(metadata)?;

                        // Metadata starts with an extra 0 or 1 for the tag.
                        let (metadata, tag) = if let Some(metadata) = metadata.strip_prefix('0') {
                            (metadata, false)
                        } else if let Some(metadata) = metadata.strip_prefix('1') {
                            (metadata, true)
                        } else {
                            bail!("Invalid metadata, doesn't start 0x1 or 0x0");
                        };
                        match size_of::<Usize>() {
                            4 => Data::U64(
                                ((parse_hex::<u32>(metadata)? as u64) << 32)
                                    | parse_hex::<u32>(data)? as u64,
                            ),
                            8 => Data::U128(
                                ((parse_hex::<u64>(metadata)? as u128) << 64)
                                    | parse_hex::<u64>(data)? as u128,
                            ),
                            _ => bail!("Unsupport XLEN"),
                        }
                    }
                    None => {
                        let val = strip_hex_prefix(val)?;
                        let val = val.trim_start_matches('?');

                        match val.len() {
                            2 => Data::U8(parse_hex(val)?),
                            4 => Data::U16(parse_hex(val)?),
                            8 => Data::U32(parse_hex(val)?),
                            16 => Data::U64(parse_hex(val)?),
                            32 => Data::U128(parse_hex(val)?),
                            _ => bail!("Invalid hex length: {val:?}"),
                        }
                    }
                });
            } else if let Some(val) = part.strip_prefix("PA:") {
                if phys_addr.is_some() {
                    bail!("Multiple PAs found");
                }
                phys_addr = Some(parse_hex(strip_hex_prefix(val)?)?);
            } else {
                for index in 1..32 {
                    if let Some(val) = part.strip_prefix(&format!("x{index}=")) {
                        if xwrite.is_some() {
                            bail!("Multiple X writes found");
                        }
                        // We ignore the metadata for register writes because I haven't
                        // found a way to display it yet.
                        let value = match val.split_once('+') {
                            Some((data, _metadata)) => parse_hex(strip_hex_prefix(data)?)?,
                            None => parse_hex(strip_hex_prefix(val)?)?,
                        };
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
        (Some(value), Some(phys_addr)) => Some(MemWrite {
            phys_addr,
            value,
            prev_value: None,
        }),
        (None, _) => None,
        (Some(_), None) => bail!("Store without PA"),
    };

    Ok(TraceEvent {
        time,
        cycle,
        pc,
        trap: assembly_mnemonic.is_some_and(|s| s.starts_with("-->")),
        instruction: Some(instruction),
        assembly_mnemonic: assembly_mnemonic.unwrap_or_default().to_owned(),
        assembly_args: assembly_args.unwrap_or_default().to_owned(),
        xwrite,
        store,
    })
}

pub fn read_trace<Usize: Num>(file_path: &Path) -> Result<Vec<TraceEvent<Usize>>> {
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
