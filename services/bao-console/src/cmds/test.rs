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
                "time" => {
                    use chrono::{Local, Utc};
                    let systime = std::time::SystemTime::now();
                    let epoch = systime.duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap();
                    log::info!("Seconds since epoch: {}", epoch.as_secs());
                    log::info!("Systime: {:?}", systime);
                    let now = Utc::now();
                    log::info!("UTC now {}", now.format("%Y-%m-%d %H:%M:%S UTC"));
                    let local_now = Local::now();
                    log::info!("Local time: {}", local_now.format("%Y-%m-%d %H:%M:%S %Z"));

                    log::info!("Waiting 3 seconds");
                    std::thread::sleep(std::time::Duration::from_secs(3));

                    let systime = std::time::SystemTime::now();
                    let epoch = systime.duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap();
                    log::info!("Seconds since epoch: {}", epoch.as_secs());
                    log::info!("Systime: {:?}", systime);
                    let now = Utc::now();
                    log::info!("UTC now {}", now.format("%Y-%m-%d %H:%M:%S UTC"));
                    let local_now = Local::now();
                    log::info!("Local time: {}", local_now.format("%Y-%m-%d %H:%M:%S %Z"));
                }
                #[cfg(feature = "bmp180")]
                "temp" => {
                    use bao1x_hal::bmp180::Bmp180;
                    use bao1x_hal_service::I2c;
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
                #[cfg(not(feature = "hosted-baosec"))]
                "shutdown" => {
                    use bao1x_api::*;
                    use bao1x_hal_service::I2c;
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
                "seed" => {
                    let (_, value) = std::env::vars().find(|(key, _value)| key == "SEED").unwrap();
                    log::info!("Seed: {:?}", value);
                }
                "keepon" => {
                    todo!("Fix this to use DCDC2 for keepon (as per baosec v2)");
                }
                "qrshow" => {
                    let modals = modals::Modals::new(&_env.xns).unwrap();
                    let mut test_data = [0u8; 40];
                    for (i, d) in test_data.iter_mut().enumerate() {
                        *d = i as u8;
                    }
                    let encoded = base45::encode(&test_data);
                    modals.show_notification("", Some(&encoded)).ok();
                }
                "qrget" => {
                    let gfx = ux_api::service::gfx::Gfx::new(&_env.xns).unwrap();
                    match gfx.acquire_qr() {
                        Ok(qr_data) => {
                            if let Some(meta) = qr_data.meta {
                                log::info!("QR code metadata: {}", meta);
                            }
                            if let Some(coded) = qr_data.content {
                                match base45::decode(&coded) {
                                    Ok(data) => {
                                        log::info!("Recovered: {:x?}", data);
                                    }
                                    Err(e) => {
                                        log::info!("Base45 decode err: {:?}", e);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            log::info!("QR error: {:?}", e);
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
