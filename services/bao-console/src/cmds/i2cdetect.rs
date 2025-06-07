use String;
use cramium_api::I2cApi;
use cram_hal_service::I2c;
use std::thread::sleep;
use std::time::Duration;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]

pub struct I2cDetect {}
struct Bmp180Calibration {
    ac1: i16, ac2: i16, ac3: i16,
    ac4: u16, ac5: u16, ac6: u16,
    b1: i16,  b2: i16,
    mb: i16,  mc: i16,  md: i16,
}

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
                    const REG_CALIB_START: u8 = 0xAA;
                    const REG_CTRL: u8 = 0xF4;
                    const REG_DATA_START: u8 = 0xF6;
                    const CMD_READ_TEMP: u8 = 0x2E;

                    let mut i2c = I2c::new();

                    let mut cal_buf = [0u8; 22];
                    if let Err(e) = i2c.i2c_read(BMP180_ADDR, REG_CALIB_START, &mut cal_buf, true) {
                        write!(ret, "Failed to read calibration data: {:?}\n", e).unwrap();
                        return Ok(Some(ret));
                    }

                    // note: calibration data is Big Endian, hence the from_be_bytes
                    let cal = Bmp180Calibration {
                        ac1: i16::from_be_bytes([cal_buf[0], cal_buf[1]]),
                        ac2: i16::from_be_bytes([cal_buf[2], cal_buf[3]]),
                        ac3: i16::from_be_bytes([cal_buf[4], cal_buf[5]]),
                        ac4: u16::from_be_bytes([cal_buf[6], cal_buf[7]]),
                        ac5: u16::from_be_bytes([cal_buf[8], cal_buf[9]]),
                        ac6: u16::from_be_bytes([cal_buf[10], cal_buf[11]]),
                        b1:  i16::from_be_bytes([cal_buf[12], cal_buf[13]]),
                        b2:  i16::from_be_bytes([cal_buf[14], cal_buf[15]]),
                        mb:  i16::from_be_bytes([cal_buf[16], cal_buf[17]]),
                        mc:  i16::from_be_bytes([cal_buf[18], cal_buf[19]]),
                        md:  i16::from_be_bytes([cal_buf[20], cal_buf[21]]),
                    };

    
                    if let Err(e) = i2c.i2c_write(BMP180_ADDR, REG_CTRL, &[CMD_READ_TEMP]) {
                        write!(ret, "Failed to start temp measurement: {:?}\n", e).unwrap();
                        return Ok(Some(ret));
                    }

                    sleep(Duration::from_millis(5));

                    let mut temp_buffer = [0u8; 2];
                    if let Err(e) = i2c.i2c_read(BMP180_ADDR, REG_DATA_START, &mut temp_buffer, true) {
                        write!(ret, "Failed to read temp data: {:?}\n", e).unwrap();
                        return Ok(Some(ret));
                    }
                    let ut = i16::from_be_bytes(temp_buffer) as i32;

                    let x1 = (ut - cal.ac6 as i32) * cal.ac5 as i32 >> 15;
                    let x2 = (cal.mc as i32 * 2048) / (x1 + cal.md as i32);
                    let b5 = x1 + x2;
                    let temp = ((b5 + 8) >> 4) as f32 / 10.0;

                    write!(ret, "BMP180 Temperature: {:.1}°C\n", temp).unwrap();
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