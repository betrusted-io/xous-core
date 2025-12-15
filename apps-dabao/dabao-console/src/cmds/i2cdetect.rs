use String;
use bao1x_api::I2cApi;
use bao1x_api::I2cResult;
use bao1x_hal::i2c::I2c;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]

enum ProbeStatus {
    Ack,
    Nack,
}

fn probe_address(i2c: &mut I2c, addr: u8) -> ProbeStatus {
    let mut buf = [0u8; 1];
    match i2c.i2c_read(addr, 0x00, &mut buf, false) {
        Ok(I2cResult::Ack(_)) => ProbeStatus::Ack,
        Ok(I2cResult::Nack) => ProbeStatus::Nack,
        Ok(I2cResult::Pending) => panic!("Unexpected I2cResult::Pending: this operation should be blocking."),
        Ok(I2cResult::InternalError) => panic!("Unexpected Ok(I2cResult::InternalError)."),
        Err(e) => panic!("I2C bus error while probing address 0x{:02X}: {:?}", addr, e),
    }
}

pub struct I2cDetect {}

impl<'a> ShellCmdApi<'a> for I2cDetect {
    cmd_api!(i2cdetect);

    fn process(&mut self, args: String, _env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();
        let helpstring = "Usage:
    i2cdetect probe <hex>        - probes a single I2C address (e.g. 34)
    i2cdetect scan               - reads all addresses on the bus (use with caution!)";

        let mut tokens = args.split_whitespace();

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "probe" => {
                    if let Some(addr_str) = tokens.next() {
                        match u8::from_str_radix(addr_str, 16) {
                            Ok(addr) => {
                                let mut i2c = I2c::new();
                                let status = probe_address(&mut i2c, addr);
                                match status {
                                    ProbeStatus::Ack => {
                                        write!(ret, "Device found at address {:02X}\n", addr).unwrap()
                                    }
                                    ProbeStatus::Nack => {
                                        write!(ret, "Device at address {:02X} responded with NACK\n", addr)
                                            .unwrap()
                                    }
                                };
                            }
                            Err(_) => {
                                write!(
                                    ret,
                                    "Invalid hex address '{}'. Example: i2cdetect probe 3C\n",
                                    addr_str
                                )
                                .unwrap();
                            }
                        }
                    } else {
                        write!(ret, "Usage: i2cdetect probe <hexaddr>\n").unwrap();
                    }
                }

                "scan" => {
                    let mut i2c = I2c::new();
                    writeln!(&mut ret, "     0  1  2  3  4  5  6  7  8  9  a  b  c  d  e  f").unwrap();
                    for row in 0..8 {
                        write!(&mut ret, "{:02x}: ", row * 16).unwrap();
                        for col in 0..16 {
                            let addr = (row * 16 + col) as u8;
                            if addr < 0x03 || addr > 0x77 {
                                write!(&mut ret, "   ").unwrap();
                            } else {
                                let status = probe_address(&mut i2c, addr);
                                match status {
                                    ProbeStatus::Ack => write!(&mut ret, "{:02x} ", addr).unwrap(),
                                    ProbeStatus::Nack => write!(&mut ret, "-- ").unwrap(),
                                };
                            }
                        }
                        writeln!(&mut ret).unwrap();
                    }
                }

                _ => {
                    write!(ret, "{}\n", helpstring).unwrap();
                }
            }
        } else {
            write!(ret, "{}\n", helpstring).unwrap();
        }

        Ok(Some(ret))
    }
}
