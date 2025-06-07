use String;
use cramium_api::I2cApi;
use cram_hal_service::I2c;
use std::thread::sleep;
use std::time::Duration;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct I2cDetect {}

impl<'a> ShellCmdApi<'a> for I2cDetect {
    cmd_api!(i2cdetect);

    fn process(&mut self, args: String, _env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
    use core::fmt::Write;
    let mut ret = String::new();
    let helpstring = "Usage:
    i2cdetect cat       - prints cat ascii art
    i2cdetect bread     - prints bread ascii art
    i2cdetect breadcat  - prints breadcat ascii art
    i2cdetect probe <hex> - probes a single I2C address (e.g. 34)
    i2cdetect bmptemp        - reads raw temperature from BMP180";

    let mut tokens = args.split_whitespace();

    if let Some(sub_cmd) = tokens.next() {
        match sub_cmd {
            "cat" => {
                let cat_art = r#"f
                ⠀⠀⠀⠀⢀⠠⠤⠀⢀⣿⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
                ⠀⠀⠐⠀⠐⠀⠀⢀⣾⣿⡇⠀⠀⠀⠀⠀⢀⣼⡇⠀⠀⠀⠀
                ⠀⠀⠀⠀⠀⠀⠀⣸⣿⣿⣿⠀⠀⠀⠀⣴⣿⣿⠇⠀⠀⠀⠀
                ⠀⠀⠀⠀⠀⠀⢠⣿⣿⣿⣇⠀⠀⢀⣾⣿⣿⣿⠀⠀⠀⠀⠀
                ⠀⠀⠀⠀⠀⣴⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⡟⠀⠀⠐⠀⡀
                ⠀⠀⠀⠀⢰⡿⠉⠀⡜⣿⣿⣿⡿⠿⢿⣿⣿⡃⠀⠀⠂⠄⠀
                ⠀⠀⠒⠒⠸⣿⣄⡘⣃⣿⣿⡟⢰⠃⠀⢹⣿⡇⠀⠀⠀⠀⠀
                ⠀⠀⠚⠉⠀⠊⠻⣿⣿⣿⣿⣿⣮⣤⣤⣿⡟⠁⠘⠠⠁⠀⠀
                ⠀⠀⠀⠀⠀⠠⠀⠀⠈⠙⠛⠛⠛⠛⠛⠁⠀⠒⠤⠀⠀⠀⠀
                ⠨⠠⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠑⠀⠀⠀⠀⠀⠀
                ⠁⠃⠉⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
                "#;
                write!(ret, "{}", cat_art).unwrap();
            }
            "bread" => {
                let bread_art = r#"
                ⠀⠀⠀⠀⣀⣠⣤⣤⣤⣤⣤⣄⣀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
                ⠀⠀⣴⣿⣿⠿⠛⣉⣁⣠⣤⣤⣤⣬⣀⣀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
                ⠀⢸⣿⣿⣿⣿⣿⣿⡿⠛⣉⣁⣠⣤⣤⣤⣬⣀⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
                ⠀⢸⣿⣿⣿⣿⣿⣵⣾⣿⡿⠟⠛⠉⣉⣉⣉⣉⠉⠓⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
                ⠀⢸⣿⣿⣿⣿⣿⣿⡟⢁⣤⣶⣿⣿⣿⣿⣿⣿⣿⣿⣶⣄⠀⠀⠀⠀⠀⠀⠀⠀
                ⠀⢸⣿⣿⣿⣿⣿⣿⠀⣾⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⠇⠀⠀⠀⠀⠀⠀⠀
                ⠀⢿⣿⣿⣿⣿⣿⣿⡄⢹⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⠀⠀⠀⠀⠀⠀⠀⠀
                ⠀⠈⠻⣿⣿⣿⣿⣿⣷⠀⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣇⠀⠀⠀⠀⠀⠀⠀⠀
                ⠀⠀⠀⠈⠻⣿⣿⣿⣿⠀⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⠀⠀⠀⠀⠀⠀⠀⠀
                ⠀⠀⠀⠀⠀⠙⢿⣿⠇⢰⣿⣿⣿⣿⡿⠛⢉⣩⣉⡉⠛⠻⠇⠀⠀⠀⠀⠀⠀⠀
                ⠀⠀⠀⠀⠀⠀⠀⠙⠀⠸⠿⠿⠟⢉⣠⣾⣿⣿⣿⣿⣷⣶⣦⣤⣄⣀⣀⡀⠀⠀
                ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣴⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⡷⠀
                ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢈⣉⣉⠉⠛⠛⠻⠿⣿⣿⣿⣿⣿⣿⣿⠿⠋⣀⠀
                ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠛⠛⠛⠛⠻⠶⣶⣤⣈⣉⣉⣉⣁⣤⣴⠾⠛⠀
                ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠉⠙⠛⠉⠉⠉⠀⠀⠀⠀
                "#;
                write!(ret, "{}", bread_art).unwrap();
            }
            "breadcat" => {
                let breadcat_art = r#"
                ⠀⠀⠀⠰⣦⣤⣤⣀⠀⠀⡀⠀⠀⠀⠀⣀⣀⠀⠠⢤⣶⣶⣿⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
                ⠀⠀⠀⠀⢿⣿⢋⣵⣾⣿⣿⡿⣥⣮⢻⣿⣿⣿⣿⣶⣌⢿⣿⢰⣶⣶⣶⣶⣶⣶⣶⣶⣶⣶⣦⣤⡀⠀⠀⠀⠀⠀⠀⠀⠀
                ⠀⠀⠀⠀⠘⢡⣿⣿⣿⣿⣿⣷⡹⠟⣵⣿⣿⣿⣿⣿⣿⣦⢃⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⠀⠀⠀⠀⠀⠀⠀⠀
                ⠀⠀⠀⠀⠀⢺⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⢨⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⠀⠀⠀⠀⠀⠀⠀⠀
                ⠀⠀⠀⠀⠀⠸⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⡿⣿⣿⣿⡟⣸⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⠀⠀⠀⠀⠀⠀⠀⠀
                ⠀⠀⠀⠀⠀⠀⠙⣿⣷⣬⣿⣿⣿⣿⣿⣿⣿⡼⣿⣿⡟⣱⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⠀⠀⠀⠀⠀⠀⠀⠀
                ⠀⠀⠀⠀⠀⠀⠀⣿⣧⣠⣿⣿⣿⣿⣿⣿⣿⣀⣾⣿⡇⢿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⡀⠀⠀⠀⠀⠀⠀⠀
                ⠀⠀⠀⠀⠀⠀⠀⣿⣿⣿⣿⣿⣅⣽⣿⣿⣿⣿⣿⣿⡇⢿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣦⠀⠀⠀⠀⠀⠀
                ⠀⠀⠀⠀⠀⠀⠀⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⡇⢿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣷⡀⠀⠀⠀⠀
                ⠀⠀⠀⠀⠀⠀⠀⠛⠙⠋⠋⠋⠋⠋⠋⠋⠋⠛⠙⠙⠁⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⢿⣿⣿⣿⣿⣿⣧⠀⠀⠀⠀
                ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢈⣿⣿⣿⣿⣿⣿⠀⠀⠀⠀
                ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡠⣴⣶⣉⢿⣿⣿⣿⡟⠀⠀⠀⠀
                ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣤⣆⡻⣿⣷⠜⣿⣏⣾⣿⣿⡿⠁⠀⠀⠀⠀
                ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢠⣿⣿⣿⣇⣿⣿⣽⣿⣿⣿⠿⠋⠀⠀⠀⠀⠀⠀
                ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⢿⣿⣿⣿⣿⣿⠿⠟⠋⠁⠀⠀⠀⠀⠀⠀⠀⠀
                ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠉⠉⠉⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
                "#;
                write!(ret, "{}", breadcat_art).unwrap();
            }

           "probe" => {
                if let Some(addr_str) = tokens.next() {
                    match u8::from_str_radix(addr_str, 16) {
                        Ok(addr) => {
                            let mut i2c = I2c::new();
                            let mut buf = [0u8; 1];
                            match i2c.i2c_read(addr, 0x00, &mut buf, false) {
                                Ok(_) => {
                                    write!(ret, "Device found at address {:02X} (via read)\n", addr).unwrap();
                                }
                                Err(xous::Error::InternalError) => {
                                    write!(ret, "No device found at address {:02X}\n", addr).unwrap();
                                }
                                Err(e) => {
                                    write!(ret, "Error probing address {:02X}: {:?}\n", addr, e).unwrap();
                                }
                            }
                        }
                        Err(_) => {
                            write!(ret, "Invalid hex address '{}'. Example: i2cdetect probe 3C\n", addr_str).unwrap();
                        }
                    }
                } else {
                    write!(ret, "Usage: i2cdetect probe <hexaddr>\n").unwrap();
                }
            }

            "bmptemp" => {
                    const BMP180_ADDR: u8 = 0x77;
                    const REG_CTRL: u8 = 0xF4;
                    const REG_DATA_START: u8 = 0xF6;
                    const CMD_READ_TEMP: u8 = 0x2E;

                    let mut i2c = I2c::new();

                    match i2c.i2c_write(BMP180_ADDR, REG_CTRL, &[CMD_READ_TEMP]) {
                        Ok(_) => {
                            sleep(Duration::from_millis(5));

                            
                            let mut temp_buffer = [0u8; 2];
                            match i2c.i2c_read(BMP180_ADDR, REG_DATA_START, &mut temp_buffer, true) {
                                Ok(_) => {
                                    let uncalibrated_temp = i16::from_be_bytes(temp_buffer);
                                    write!(ret, "BMP180 raw temp: {}\n", uncalibrated_temp).unwrap();
                                }
                                Err(e) => {
                                    write!(ret, "Failed to read temp data: {:?}\n", e).unwrap();
                                }
                            }
                        }
                        Err(e) => {
                            write!(ret, "Failed to write command to BMP180 (is it connected @ 0x77?): {:?}\n", e).unwrap();
                        }
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