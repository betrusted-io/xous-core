use String;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Test {}

impl<'a> ShellCmdApi<'a> for Test {
    cmd_api!(test);

    // inserts boilerplate for command API

    fn process(&mut self, args: String, _env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();

        #[allow(unused_variables)]
        let helpstring = "Test commands. See code for options.";

        #[cfg(feature = "bmp180")]
        let helpstring = "Usage:
        temp     - reads temperature from bmp180.";

        let mut tokens = args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                #[cfg(feature = "bmp180")]
                "temp" => {
                    use cram_hal_service::I2c;
                    use cramium_hal::bmp180::Bmp180;
                    let mut i2c = I2c::new();

                    match Bmp180::new(&mut i2c) {
                        Ok(sensor) => match sensor.read_temperature(&mut i2c) {
                            Ok(temp) => {
                                write!(ret, "BMP180 Temperature: {:.1}°C", temp).unwrap();
                            }
                            Err(e) => {
                                write!(ret, "Failed to read temperature: {:?}", e).unwrap();
                            }
                        },
                        Err(e) => {
                            write!(ret, "Failed to initialize BMP180 sensor: {:?}", e).unwrap();
                        }
                    }
                }

                _ => {
                    write!(ret, "{}", helpstring).unwrap();
                }
            }
        } else {
            write!(ret, "{}", helpstring).unwrap();
        }
        Ok(Some(ret))
    }
}
