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
                "timer" => {
                    let start = _env.ticktimer.elapsed_ms();
                    log::info!("Starting test");
                    let mut seconds = 0;
                    loop {
                        let elapsed = _env.ticktimer.elapsed_ms() - start;
                        if elapsed > seconds * 1000 {
                            log::info!("{} s", seconds);
                            seconds += 1;
                        }
                    }
                }
                #[cfg(feature = "bmp180")]
                "temp" => {
                    use bao1x_hal_service::I2c;
                    use bao1x_hal::bmp180::Bmp180;
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
                "shutdown" => {
                    use bao1x_hal_service::I2c;
                    use bao1x_api::*;
                    let iox = bao1x_api::IoxHal::new();
                    let mut i2c = I2c::new();
                    iox.setup_pin(
                        IoxPort::PF,
                        0,
                        Some(IoxDir::Output),
                        Some(IoxFunction::Gpio),
                        None,
                        Some(IoxEnable::Disable),
                        None,
                        Some(IoxDriveStrength::Drive8mA),
                    );
                    iox.set_gpio_pin_value(IoxPort::PF, 0, IoxValue::Low);
                    log::info!(
                        "shutdown got {:x?}, {:x?}",
                        iox.get_gpio_pin_value(IoxPort::PF, 0),
                        iox.get_gpio_bank_value(IoxPort::PF)
                    );

                    let axp2101 = bao1x_hal::axp2101::Axp2101::new(&mut i2c).expect("couldn't get AXP2101");
                    log::info!("sending shutdown to axp2101 pmic...in four seconds");
                    let tt = ticktimer::Ticktimer::new().unwrap();
                    tt.sleep_ms(4000).ok();
                    axp2101.powerdown(&mut i2c).ok();
                    iox.setup_pin(
                        IoxPort::PF,
                        6,
                        Some(IoxDir::Output),
                        Some(IoxFunction::Gpio),
                        None,
                        Some(IoxEnable::Disable),
                        None,
                        Some(IoxDriveStrength::Drive8mA),
                    );
                    iox.set_gpio_pin_value(IoxPort::PF, 6, IoxValue::Low);
                    log::info!("sent shutdown to axp2101");
                }
                "keepon" => {
                    use bao1x_api::*;
                    let iox = bao1x_api::IoxHal::new();
                    let (port, pin) = bao1x_hal::board::setup_keep_on_pin(&iox);
                    iox.set_gpio_pin_value(port, pin, IoxValue::High);
                    let _ = bao1x_hal::board::setup_kb_pins(&iox);
                    log::info!("keepon got {:x?}", iox.get_gpio_pin_value(IoxPort::PF, 0));
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
