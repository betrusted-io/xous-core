use std::collections::BTreeMap;
use std::fs::File;
use std::io;

pub struct CsrMemoryRegion {
    pub start: u32,
    pub length: u32,
}

pub struct CsrConfig {
    pub regions: BTreeMap<String, CsrMemoryRegion>,
}

const PAGE_SIZE: u32 = 4096;

#[derive(Debug)]
pub enum ConfigError {
    /// Couldn't parse string as number
    NumberParseError(String, std::num::ParseIntError),

    /// Generic IO Error
    IoError(io::Error),
}

impl std::convert::From<io::Error> for ConfigError {
    fn from(e: io::Error) -> ConfigError { ConfigError::IoError(e) }
}

pub fn get_base(value: &str) -> (&str, u32) {
    if value.starts_with("0x") {
        (value.trim_start_matches("0x"), 16)
    } else if value.starts_with("0X") {
        (value.trim_start_matches("0X"), 16)
    } else if value.starts_with("0b") {
        (value.trim_start_matches("0b"), 2)
    } else if value.starts_with("0B") {
        (value.trim_start_matches("0B"), 2)
    } else if value.starts_with('0') && value != "0" {
        (value.trim_start_matches('0'), 8)
    } else {
        (value, 10)
    }
}

pub fn parse_u32(value: &str) -> Result<u32, ConfigError> {
    let (value, base) = get_base(value);
    u32::from_str_radix(value, base).map_err(|e| ConfigError::NumberParseError(value.to_owned(), e))
}

pub fn parse_csr_csv(filename: &str) -> Result<CsrConfig, ConfigError> {
    let mut map = BTreeMap::new();
    let file = File::open(filename)?;

    let mut csr_base = 0;
    let mut csr_top = 0;

    let mut rdr = csv::ReaderBuilder::new().flexible(true).from_reader(file);
    for result in rdr.records() {
        if let Ok(r) = result {
            if r.is_empty() {
                eprintln!("csv: ignoring blank line");
                continue;
            }
            match &r[0] {
                "csr_base" => {
                    if r.len() < 3 {
                        eprintln!("csv: found csr_base entry, but entry was short");
                        continue;
                    }
                    let base_addr = parse_u32(&r[2])?;
                    if base_addr > csr_top {
                        // println!("csv: increasing csr top: {:08x} -> {:08x}", csr_top, base_addr);
                        csr_top = base_addr;
                    }
                }
                "memory_region" => {
                    if r.len() < 4 {
                        eprintln!("csv: found memory_region entry, but entry was short");
                        continue;
                    }
                    let region_name = &r[1];
                    let base_addr = parse_u32(&r[2])?;
                    let length = parse_u32(&r[3])?;

                    if region_name == "csr" {
                        csr_base = base_addr;
                    } else {
                        map.insert(region_name.to_string().to_lowercase(), CsrMemoryRegion {
                            start: base_addr,
                            length,
                        });
                    }
                }
                _ => (),
            };
        }
    }

    if csr_base != 0 && csr_top != 0 {
        csr_top += 1;
        csr_top = (csr_top + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        map.insert("csr".to_string(), CsrMemoryRegion { start: csr_base, length: csr_top - csr_base });
    }
    Ok(CsrConfig { regions: map })
}
