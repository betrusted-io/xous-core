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
        let helpstring = "test [proc] [freemem] [interrupts] [bootwait]; see code for other test commands.";

        #[cfg(feature = "bmp180")]
        let helpstring = "Usage:
        temp     - reads temperature from bmp180.";

        let mut parts = args.split_whitespace();
        let cmd = parts.next().unwrap_or("").to_string();
        let args: Vec<String> = parts.map(|s| s.to_string()).collect();

        match cmd.as_str() {
            "bootwait" => {
                let keystore = keystore::Keystore::new(&_env.xns);
                if args.len() != 1 {
                    write!(ret, "bootwait [check | enable | disable]").ok();
                }
                if args[0] == "check" {
                    write!(ret, "bootwait is {:?}", keystore.bootwait(None).unwrap()).ok();
                } else if args[0] == "enable" {
                    keystore.bootwait(Some(true)).unwrap();
                    write!(ret, "bootwait enabled").ok();
                    log::info!(
                        "{}BOOTWAIT.ENABLED,{}",
                        bao1x_hal::board::BOOKEND_START,
                        bao1x_hal::board::BOOKEND_END
                    );
                } else if args[0] == "disable" {
                    keystore.bootwait(Some(false)).unwrap();
                    write!(ret, "bootwait disabled").ok();
                } else {
                    write!(ret, "bootwait [check | enable | disable]").ok();
                }
            }
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
                use bao1x_hal::i2c::I2c;
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
            "shipmode" => {
                use bao1x_hal::i2c::I2c;
                let mut i2c = I2c::new();

                let mut axp2101 = bao1x_hal::axp2101::Axp2101::new(&mut i2c).expect("couldn't get AXP2101");
                log::info!("sending shipmode to axp2101 pmic...in four seconds");
                let tt = ticktimer::Ticktimer::new().unwrap();
                tt.sleep_ms(4000).ok();
                // shut down BLDO1 - this should disconnect the battery
                axp2101.set_ldo(&mut i2c, None, bao1x_hal::axp2101::WhichLdo::Bldo1).ok();
                // shut down PMIC
                axp2101.powerdown(&mut i2c).ok();
                log::info!("sent shutdown to axp2101");
            }
            #[cfg(not(feature = "hosted-baosec"))]
            "deepsleep" => {
                let gfx = ux_api::service::gfx::Gfx::new(&_env.xns).unwrap();
                log::info!("turn off display");
                gfx.set_power(false).unwrap();
                log::info!("display off");
                use num_traits::*;
                // monkey patch over the standard xous IP
                let conn = _env
                    .xns
                    .request_connection_blocking(susres::api::SERVER_NAME_SUSRES)
                    .expect("Can't connect to SUSRES");
                match xous::send_message(
                    conn,
                    xous::Message::new_blocking_scalar(
                        susres::api::Opcode::SuspendRequest.to_usize().unwrap(),
                        1, // this is a monkeypatch
                        0,
                        0,
                        0,
                    ),
                ) {
                    Ok(xous::Result::Scalar1(result)) => {
                        if result == 1 {
                            log::info!("Should be in deep sleep!");
                        } else {
                            log::error!("Couldn't initiate deep sleep")
                        }
                    }
                    _ => panic!("Couldn't send deep sleep message to susres"),
                }
            }
            "seed" => {
                let (_, value) = std::env::vars().find(|(key, _value)| key == "SEED").unwrap();
                log::info!("Seed: {:?}", value);
            }
            #[cfg(not(feature = "hosted-baosec"))]
            "wfi" => {
                let gfx = ux_api::service::gfx::Gfx::new(&_env.xns).unwrap();
                log::info!("turn off display");
                gfx.set_power(false).unwrap();
                log::info!("display off");
                let susres = susres::Susres::new_without_hook(&_env.xns).unwrap();
                log::info!("initiating wfi from test shell...");
                susres.initiate_suspend().unwrap();
                log::info!("waiting after WFI return (system will be in WFI)");
                _env.ticktimer.sleep_ms(100).ok();
                log::info!("turn on display");
                gfx.set_power(true).unwrap();
            }
            "proc" => {
                // hard coded - debug feature - if the platform ABI changes its name or opcode map this
                // can break, but also this routine is not meant for public
                // consumption and coding it here avoids breaking dependencies to the Xous API crate.
                let page_buf = xous::PageBuf::new();
                xous::rsyscall(xous::SysCall::PlatformSpecific(2, page_buf.as_ptr(), 0, 0, 0, 0, 0)).unwrap();

                log::info!("Process listing:");
                for line in page_buf.as_str().lines() {
                    log::info!("{}", line);
                }
            }
            "freemem" => {
                // hard coded - debug feature - if the platform ABI changes its name or opcode map this
                // can break, but also this routine is not meant for public
                // consumption and coding it here avoids breaking dependencies to the Xous API crate.
                let page_buf = xous::PageBuf::new();
                xous::rsyscall(xous::SysCall::PlatformSpecific(1, page_buf.as_ptr(), 0, 0, 0, 0, 0)).unwrap();

                log::info!("RAM usage:");
                for line in page_buf.as_str().lines() {
                    log::info!("{}", line);
                }
            }
            "interrupts" => {
                // hard coded - debug feature - if the platform ABI changes its name or opcode map this
                // can break, but also this routine is not meant for public
                // consumption and coding it here avoids breaking dependencies to the Xous API crate.
                let page_buf = xous::PageBuf::new();
                xous::rsyscall(xous::SysCall::PlatformSpecific(3, page_buf.as_ptr(), 0, 0, 0, 0, 0)).unwrap();

                log::info!("Interrupt handlers:");
                for line in page_buf.as_str().lines() {
                    log::info!("{}", line);
                }
            }
            #[cfg(feature = "board-baosec")]
            "qrshow" => {
                // note that 40 bytes gives 320 bits which fits nicely into a version 3 code,
                // which allows 4 pixels per module rendering.
                // if we have to move to 3 pixels per module, the next code up that optimally
                // uses the full screen is version 6. This would give 102 bytes of transfer
                // in a single scan.
                //
                // The equation for capacity is:
                // `binary_bytes = floor((alphanumeric_capacity / 3) × 2)`
                //
                // where `alphameric_capacity` is the capacity of the QR code version
                // per spec lookup table.
                let modals = modals::Modals::new(&_env.xns).unwrap();
                let mut test_data = [0u8; 40];
                for (i, d) in test_data.iter_mut().enumerate() {
                    *d = i as u8;
                }
                let encoded = base45::encode(&test_data);
                modals.show_notification("", Some(&encoded)).ok();
            }
            #[cfg(feature = "board-baosec")]
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
            /* // leave this around in case we have more swap bugs to debug
            // needs to have `swaptest1`` & `swaptest2` added to the image for this to work.
            "swap" => {
                log::info!("starting swap test");
                let swaptest1 = _env.xns.request_connection("swaptest1").unwrap();
                let swaptest2 = _env.xns.request_connection("swaptest2").unwrap();
                for i in 1..4 {
                    log::info!("iter {}", i);
                    xous::send_message(swaptest1, xous::Message::new_scalar(0, i, 0, 0, 0)).unwrap();
                    xous::send_message(swaptest2, xous::Message::new_scalar(0, i + 1, 0, 0, 0)).unwrap();
                }
                log::info!("swaptest done");
            }
            */
            _ => {
                write!(ret, "{}", helpstring).unwrap();
            }
        }

        Ok(Some(ret))
    }
}
