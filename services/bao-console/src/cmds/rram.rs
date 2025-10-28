use String;
use xous::MemoryFlags;

use crate::{CommonEnv, ShellCmdApi};

pub struct Rram {
    pub rram: bao1x_hal::rram::Reram,
}

// offsets into RRAM array where we've mapped pages
const RANGES: [usize; 4] = [0x30_0000, 0x30_8000, 0x30_4000, 0x30_7000];

impl Rram {
    pub fn new(_xns: &xous_names::XousNames, _env: &mut CommonEnv) -> Self {
        let mut rram = bao1x_hal::rram::Reram::new();
        for range in RANGES {
            let r = xous::map_memory(
                xous::MemoryAddress::new(range + utralib::HW_RERAM_MEM),
                None,
                4096,
                MemoryFlags::R | MemoryFlags::W,
            )
            .unwrap();
            rram.add_range(range, r);
        }
        Self { rram }
    }
}
impl<'a> ShellCmdApi<'a> for Rram {
    cmd_api!(rram);

    // inserts boilerplate for command API

    fn process(&mut self, args: String, _env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();

        #[allow(unused_variables)]
        let helpstring = "rram [read] [write] [mapped]";

        let mut parts = args.split_whitespace();
        let cmd = parts.next().unwrap_or("").to_string();
        let args: Vec<String> = parts.map(|s| s.to_string()).collect();

        match cmd.as_str() {
            "mapped" => {
                write!(ret, "Valid page offsets for access in this test: {:x?}", RANGES).ok();
            }
            "read" => {
                const COLUMNS: usize = 16;
                if args.len() == 1 || args.len() == 2 {
                    let addr = usize::from_str_radix(&args[0], 16).map_err(|_| {
                        write!(ret, "Read offset is in hex, no leading 0x").ok();
                        xous::Error::BadAddress
                    })?;

                    let count = if args.len() == 2 {
                        if let Ok(count) = u32::from_str_radix(&args[1], 10) { count } else { 32 }
                    } else {
                        1
                    };
                    for offset in (0..count).step_by(32) {
                        match self.rram.array(addr + offset as usize) {
                            Ok(peek) => {
                                for (i, &d) in peek.iter().enumerate() {
                                    if (i % COLUMNS) == 0 {
                                        print!("\n\r{:08x}: ", addr + i + offset as usize);
                                    }
                                    print!("{:02x} ", d);
                                }
                            }
                            Err(e) => {
                                write!(ret, "Couldn't read from rram array: {:?}", e).ok();
                            }
                        }
                    }
                    println!("");
                } else {
                    write!(
                        ret,
                        "{}",
                        "Help: read <offset> [count], offset into RRAM area is in hex, count in decimal"
                    )
                    .ok();
                }
            }
            "write" => {
                if args.len() == 2 || args.len() == 3 {
                    let offset = u32::from_str_radix(&args[0], 16).map_err(|_| {
                        write!(ret, "Write offset is in hex, no leading 0x").ok();
                        xous::Error::BadAddress
                    })?;

                    let value = u32::from_str_radix(&args[1], 16).map_err(|_| {
                        write!(ret, "Write value is in hex, no leading 0x").ok();
                        xous::Error::BadAddress
                    })?;
                    let count = if args.len() == 3 {
                        if let Ok(count) = u32::from_str_radix(&args[2], 10) { count } else { 1 }
                    } else {
                        1
                    };
                    let mut poke = Vec::new();
                    poke.resize(count as usize, value);
                    let poke_u8: &[u8] = bytemuck::cast_slice(&poke);

                    match self.rram.write_slice(offset as usize, &poke_u8) {
                        Ok(len) => {
                            write!(
                                ret,
                                "Poked {:x} into {:x}, {} times",
                                value,
                                offset,
                                len / size_of::<u32>()
                            )
                            .ok();
                        }
                        Err(e) => {
                            write!(ret, "Couldn't write: {:?}", e).ok();
                        }
                    }
                } else {
                    write!(ret, "{}",
                            "Help: write <offset> <value> [count], offset into RRAM/value is in hex, count in decimal",
                        ).ok();
                }
            }
            _ => {
                write!(ret, "{}", helpstring).unwrap();
            }
        }

        Ok(Some(ret))
    }
}
