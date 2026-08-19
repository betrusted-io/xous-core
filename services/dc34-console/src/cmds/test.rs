use String;
#[cfg(feature = "misc-test")]
use bao1x_api::IoSetup;
use dc34_api::*;
use num_traits::ToPrimitive;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Test {
    pwr_mgr: xous::CID,
    #[cfg(feature = "qa-test")]
    mutation_rate: u8,
}
impl Test {
    pub fn new() -> Self {
        let xns = xous_names::XousNames::new().unwrap();
        Self {
            pwr_mgr: xns.request_connection_blocking(POWER_MANAGER_SERVER).unwrap(),
            #[cfg(feature = "qa-test")]
            mutation_rate: 1,
        }
    }
}

impl<'a> ShellCmdApi<'a> for Test {
    cmd_api!(test);

    // inserts boilerplate for command API

    fn process(&mut self, args: String, _env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();

        #[allow(unused_variables)]
        let helpstring = "test [proc] [freemem] [interrupts] [bootwait]; see code for other test commands.";

        let mut parts = args.split_whitespace();
        let cmd = parts.next().unwrap_or("").to_string();
        let args: Vec<String> = parts.map(|s| s.to_string()).collect();

        match cmd.as_str() {
            "bootwait" => {
                let keystore = keystore::Keystore::new(&_env.xns);
                if args.len() != 1 {
                    write!(ret, "bootwait [check | enable | disable]").ok();
                    return Ok(Some(ret));
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
            #[cfg(feature = "misc-test")]
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
            "wfi" => {
                xous::send_message(
                    self.pwr_mgr,
                    xous::Message::new_scalar(PowerManagerOp::ForceWfi.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .ok();
            }
            "deep" => {
                xous::send_message(
                    self.pwr_mgr,
                    xous::Message::new_scalar(PowerManagerOp::ForceDeepSleep.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .ok();
                log::info!("press any key now to trigger deep sleep");
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
            #[cfg(feature = "misc-test")]
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
            #[cfg(feature = "misc-test")]
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
            #[cfg(feature = "misc-test")]
            "cam" => {
                use bao1x_api::I2cApi;
                let mut i2c = bao1x_hal::i2c::I2c::new();
                if args.len() < 1 {
                    write!(ret, "cam [poke adr data]").ok();
                    return Ok(Some(ret));
                }
                if args[0] == "poke" {
                    if args.len() != 3 {
                        write!(ret, "cam poke adr(hex) data(hex)").ok();
                        return Ok(Some(ret));
                    }
                    let adr = u8::from_str_radix(&args[1], 16).map_err(|_| xous::Error::InvalidArgument)?;
                    let data = u8::from_str_radix(&args[2], 16).map_err(|_| xous::Error::InvalidArgument)?;
                    i2c.i2c_write(0x3C, adr, &[data]).expect("write failed");
                    write!(ret, "Wrote {:x} into {:x}", data, adr).ok();
                }
            }
            #[cfg(feature = "misc-test")]
            "accel" => {
                use std::time::{Duration, Instant};

                use bao1x_hal::i2c::I2c;
                use bao1x_hal::lis2dh12::{Lis2dh12, Orientation};

                let mut i2c = I2c::new();
                // Initialize
                let accel = Lis2dh12::new(&mut i2c).unwrap();

                // Check orientation
                let duration = Duration::from_secs(2);
                let start_time = Instant::now();
                while Instant::now().duration_since(start_time).lt(&duration) {
                    let orientation = accel.get_orientation(&mut i2c).unwrap();
                    match orientation {
                        Orientation::FaceUp => println!("Face up"),
                        Orientation::FaceDown => println!("Face down"),
                        Orientation::Unknown => println!("Tilted/transitioning"),
                    }
                    let raw_data = accel.read_accel_mg(&mut i2c).unwrap();
                    log::info!("mg data: {:?}", raw_data);
                    _env.ticktimer.sleep_ms(50).ok();
                }

                // Set up motion interrupt (200mg threshold, 5 samples duration)
                // accel.set_int1_latch(&mut i2c, true)?;
                // accel.setup_motion_interrupt(&mut i2c, 100, 2)?;
                log::info!("result: {:?}", accel.debug_test_int1_simple(&mut i2c)?);

                let debug = accel.debug_dump_int_config(&mut i2c)?;
                debug.interpret();
                log::info!("debug: {:?}", debug);

                // Later, when INT1 fires, read and clear the interrupt source
                let duration = Duration::from_secs(30);
                let start_time = Instant::now();
                while Instant::now().duration_since(start_time).lt(&duration) {
                    let source = accel.read_int1_source(&mut i2c)?;
                    if source.active {
                        log::info!(
                            "Motion detected! {:?} {:?}",
                            source,
                            accel.read_accel_mg(&mut i2c).unwrap()
                        );
                        accel.reset_highpass(&mut i2c)?;
                    }
                    _env.ticktimer.sleep_ms(150).ok();
                    /*
                    if accel.poll_int1_active(&mut i2c)? {
                        log::info!("int active");
                        let raw_data = accel.read_accel_mg(&mut i2c).unwrap();
                        log::info!("mg data: {:?}", raw_data);
                    }*/
                }
            }
            "temp" => {
                let adc = bao1x_hal_service::Adc::new();
                let raw_temp = adc.read_raw(bao1x_hal::udma::AdcSource::Temperature, Some(8));
                log::info!(
                    "raw: {}, temp: {}",
                    raw_temp,
                    bao1x_hal::udma::Adc::raw_to_temp_celsius(raw_temp)
                );
            }
            #[cfg(feature = "misc-test")]
            "adc" => {
                let iox = bao1x_api::IoxHal::new();
                let adc = bao1x_hal_service::Adc::new();

                iox.setup_pin(
                    bao1x_api::IoxPort::PA,
                    4,
                    Some(bao1x_api::IoxDir::Input),
                    Some(bao1x_api::IoxFunction::Gpio),
                    Some(bao1x_api::IoxEnable::Enable),
                    Some(bao1x_api::IoxEnable::Disable),
                    None,
                    None,
                );
                // safety - we have manually checked there are no conflicts with this mapping
                unsafe { adc.enable_channel(bao1x_hal::udma::AdcExtChannel::Adc3) };
                // unsafe { adc.enable_channel(bao1x_hal::udma::AdcExtChannel::Adc3) };
                loop {
                    let vbat_raw = adc.read_raw(
                        bao1x_hal::udma::AdcSource::Ext(bao1x_hal::udma::AdcExtChannel::Adc3),
                        Some(8),
                    );
                    log::info!(
                        "vbus: {:?}, vbat: {:.3}V",
                        iox.get_gpio_pin_value(bao1x_api::IoxPort::PA, 4),
                        bao1x_hal::udma::Adc::raw_to_voltage(vbat_raw) / 0.31884f32
                    );
                    /*
                    let vbat_raw = adc.read_raw(
                        bao1x_hal::udma::AdcSource::Ext(bao1x_hal::udma::AdcExtChannel::Adc3),
                        Some(8),
                    );
                    log::info!(
                        "vbat: {:.3}V",
                        bao1x_hal::udma::Adc::raw_to_voltage(vbat_raw)
                    );
                    */
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }
            #[cfg(feature = "misc-test")]
            "autogamy" => {
                let conn = _env.xns.request_connection_blocking(dc34_api::LED_SERVER).unwrap();
                xous::send_message(
                    conn,
                    xous::Message::new_scalar(
                        dc34_api::LedManagerOp::Autogamy.to_usize().unwrap(),
                        0,
                        0,
                        0,
                        0,
                    ),
                )
                .ok();
                log::info!("autogamy sent");
            }
            #[cfg(feature = "misc-test")]
            "hue" => {
                if args.len() != 1 {
                    write!(ret, "{}", "Usage: hue <val>").ok();
                } else {
                    const INCREMENT: u8 = 32;
                    let base = u8::from_str_radix(&args[0], 10)
                        .inspect_err(|_| log::warn!("Invalid argument, replacing with 0"))
                        .unwrap_or(0);
                    let phenotype = Haploid {
                        cd_period: 128,
                        cd_rate: 128,
                        cd_dir: 128,
                        sat: 255,
                        hue_ratedir: 128,
                        hue_base: base,
                        hue_bound: base + INCREMENT,
                        chaser: 128,
                        nonlin: 128,
                    };
                    let conn = _env.xns.request_connection_blocking(dc34_api::LED_SERVER).unwrap();
                    log::info!("sending hue {}-{}", base, base + INCREMENT);
                    let args = phenotype.serialize_u32();
                    xous::send_message(
                        conn,
                        xous::Message::new_blocking_scalar(
                            dc34_api::LedManagerOp::Force.to_usize().unwrap(),
                            args[0] as usize,
                            args[1] as usize,
                            args[2] as usize,
                            args[3] as usize,
                        ),
                    )
                    .ok();
                }
            }
            #[cfg(feature = "misc-test")]
            "reset" => {
                let pddb = pddb::Pddb::new();
                pddb.delete_dict(DC34_DICT, None).ok();
            }
            // used by factory test to provision a k0 value
            // test cases:
            //   - JN1JoLLZdw2uR5FNxci8V5SHKZcaZ/Vv8TUGaCukIVFXu6Zi
            //   - R4uPrdM4ZlKxgvQprePOQnlqxhRShMDu8QlCwwTDqcFSKdhk
            "k0" => {
                use base64::{Engine as _, engine::general_purpose};
                if args.len() == 1 {
                    match general_purpose::STANDARD.decode(&args[0]) {
                        Ok(k0_data) => {
                            if k0_data.len() == 32 + 4 {
                                let k0 = &k0_data[..32];
                                if crc32fast::hash(&k0)
                                    == u32::from_le_bytes(k0_data[32..36].try_into().unwrap())
                                {
                                    log::info!("_|TT|_K0.SUCCESS,_|TE|_");
                                    save_k0(k0.try_into().unwrap());
                                } else {
                                    log::info!("_|TT|_K0.FAIL,_|TE|_");
                                    log::error!(
                                        "k0 checksum failed {:x?}:{:x?}",
                                        &k0_data[32..36],
                                        &k0[..32]
                                    );
                                }
                            } else {
                                log::info!("_|TT|_K0.FAIL,_|TE|_");
                                log::error!("k0 data length wrong {}", k0_data.len())
                            }
                        }
                        Err(e) => {
                            log::info!("_|TT|_K0.FAIL,_|TE|_");
                            log::error!("Couldn't decode k0! {:?}", e)
                        }
                    }
                }
            }
            #[cfg(feature = "misc-test")]
            "fakek0" => {
                if get_k0().is_none() {
                    save_k0(&[1u8; 32]);
                } else {
                    log::info!("k0 already set, refusing to overwrite!")
                }
            }
            #[cfg(feature = "hazardous-test")]
            "k0check" => {
                let k0 = get_k0();
                log::info!("k0: {:x?}", k0);
            }
            #[cfg(feature = "qa-test")]
            "rate" => {
                if args.len() != 1 {
                    write!(ret, "test rate <rate>, where rate is 0-255").ok();
                }
                let conn = _env.xns.request_connection_blocking(dc34_api::LED_SERVER).unwrap();
                let rate = u8::from_str_radix(&args[0], 10).unwrap_or(0);
                self.mutation_rate = rate;
                log::info!("Fixing rate to {}", self.mutation_rate);
                xous::send_message(
                    conn,
                    xous::Message::new_blocking_scalar(
                        dc34_api::LedManagerOp::SetTestRate.to_usize().unwrap(),
                        self.mutation_rate as usize,
                        0,
                        0,
                        0,
                    ),
                )
                .ok();
            }
            #[cfg(feature = "qa-test")]
            "transmute" => {
                let conn = _env.xns.request_connection_blocking(dc34_api::LED_SERVER).unwrap();
                if args.len() != 1 {
                    write!(ret, "{}", "Usage: transmute [human|goon|comm|village|ctf|other|uber]").ok();
                } else {
                    let badge_type = match args[0].as_str() {
                        "human" => BadgeType::Human,
                        "goon" => BadgeType::Goon,
                        "comm" => BadgeType::Community,
                        "ctf" => BadgeType::CtfContest,
                        "other" => BadgeType::Other,
                        "uber" => BadgeType::Uber,
                        "village" => BadgeType::Village,
                        _ => BadgeType::None,
                    };
                    log::info!("Transmuting BadgeType: {:?}", badge_type);
                    init_light_gene(badge_type);
                    let gene = get_light_gene();
                    if let Some(gene) = gene {
                        gene.send(conn, LedManagerOp::SetGene.to_usize().unwrap());
                    }
                }
            }
            #[cfg(feature = "qa-test")]
            "bt" => {
                let conn = _env.xns.request_connection_blocking(dc34_api::LED_SERVER).unwrap();
                if args.len() != 1 {
                    write!(ret, "{}", "Usage: bt [human|goon|comm|village|ctf|other|uber]").ok();
                } else {
                    let badge_type = match args[0].as_str() {
                        "human" => BadgeType::Human,
                        "goon" => BadgeType::Goon,
                        "comm" => BadgeType::Community,
                        "ctf" => BadgeType::CtfContest,
                        "other" => BadgeType::Other,
                        "uber" => BadgeType::Uber,
                        "village" => BadgeType::Village,
                        _ => BadgeType::None,
                    };
                    log::info!("Setting BadgeType: {:?}", badge_type);
                    xous::send_message(
                        conn,
                        xous::Message::new_blocking_scalar(
                            dc34_api::LedManagerOp::GeneTest.to_usize().unwrap(),
                            badge_type as u8 as usize,
                            0,
                            0,
                            0,
                        ),
                    )
                    .ok();
                }
            }
            #[cfg(feature = "qa-test")]
            "mate" => {
                let conn = _env.xns.request_connection_blocking(dc34_api::LED_SERVER).unwrap();
                if args.len() < 1 {
                    write!(ret, "{}", "Usage: bt [human|goon|comm|village|ctf|other|uber]").ok();
                }
                let badge_type = match args[0].as_str() {
                    "human" => BadgeType::Human,
                    "goon" => BadgeType::Goon,
                    "comm" => BadgeType::Community,
                    "ctf" => BadgeType::CtfContest,
                    "other" => BadgeType::Other,
                    "uber" => BadgeType::Uber,
                    "village" => BadgeType::Village,
                    _ => BadgeType::None,
                };
                let mut sperm = Haploid::from_type(&badge_type);
                let rate = MutationRate::from_param(self.mutation_rate);
                if args.len() > 1 {
                    log::info!("elevated sperm mutation rate {}/{:?}", self.mutation_rate, rate);
                    mutate(&mut sperm, rate);
                } else {
                    log::info!("baseline mutation rate");
                    mutate(&mut sperm, MutationRate::Baseline); // use the "standard" rate of a donor
                }
                let args = sperm.serialize_u32();
                log::info!("Mating with fictional {:?}", badge_type);
                xous::send_message(
                    conn,
                    xous::Message::new_blocking_scalar(
                        dc34_api::LedManagerOp::Syngamy.to_usize().unwrap(),
                        args[0] as usize,
                        args[1] as usize,
                        args[2] as usize,
                        args[3] as usize,
                    ),
                )
                .ok();
            }
            "jig" => {
                let conn = _env.xns.request_connection_blocking("_Vault2_").unwrap();
                xous::send_message(conn, xous::Message::new_scalar(1025, 0, 0, 0, 0)).ok();
                write!(ret, "jig mode set").ok();
            }
            "hw" => {
                write!(ret, "Xous version: {}", _env.ticktimer.get_version()).unwrap();
                log::info!(
                    "{}VER.XOUS,{},{}",
                    bao1x_hal::board::BOOKEND_START,
                    _env.ticktimer.get_version(),
                    bao1x_hal::board::BOOKEND_END
                );
                let conn = _env.xns.request_connection_blocking(dc34_api::POWER_MANAGER_SERVER).unwrap();
                let mut passing = true;
                match xous::send_message(
                    conn,
                    xous::Message::new_blocking_scalar(
                        PowerManagerOp::GetAccelId.to_usize().unwrap(),
                        0,
                        0,
                        0,
                        0,
                    ),
                ) {
                    Ok(xous::Result::Scalar5(_id, exists, val, _, _)) => {
                        if exists != 0 {
                            if val != 0x33 {
                                passing = false;
                                log::info!("bad accel ID: {:x}", val);
                                log::error!("_|TT|_HW.FAIL,ACCEL,{:x}_|TE|_", val);
                            }
                        } else {
                            // TODO: uncomment this for production
                            // passing = false;
                            log::info!("accel missing");
                            log::error!("_|TT|_HW.FAIL,ACCEL,_|TE|_");
                        }
                    }
                    _ => {
                        passing = false;
                        log::error!("_|TT|_HW.ERROR,_|TE|_");
                    }
                };
                // skip the first readings as they are bogus
                for _ in 0..2 {
                    xous::send_message(
                        conn,
                        xous::Message::new_blocking_scalar(
                            PowerManagerOp::GetVbat.to_usize().unwrap(),
                            0,
                            0,
                            0,
                            0,
                        ),
                    )
                    .ok();
                    _env.ticktimer.sleep_ms(100).ok();
                }
                match xous::send_message(
                    conn,
                    xous::Message::new_blocking_scalar(
                        PowerManagerOp::GetVbat.to_usize().unwrap(),
                        0,
                        0,
                        0,
                        0,
                    ),
                ) {
                    Ok(xous::Result::Scalar5(_id, ok, val, _, _)) => {
                        if ok != 0 {
                            // set point of tester is 2410 volts
                            // voltages are scaled 22 / (22 + 47) = 0.3188
                            // target code is thus 768
                            // +/- 15% = 652 - 883
                            log::info!("_|TT|_HW.VBAT,{},_|TE|_", val);
                        } else {
                            log::info!("vbat missing");
                            passing = false;
                            log::error!("_|TT|_HW.FAIL,VBAT,_|TE|_");
                        }
                    }
                    _ => {
                        passing = false;
                        log::error!("_|TT|_HW.ERROR,_|TE|_")
                    }
                };
                match xous::send_message(
                    conn,
                    xous::Message::new_blocking_scalar(
                        PowerManagerOp::GetVbus.to_usize().unwrap(),
                        0,
                        0,
                        0,
                        0,
                    ),
                ) {
                    Ok(xous::Result::Scalar5(_id, _ok, vbus, _, _)) => {
                        // correct state depends on configuration of tester. Just report the value.
                        log::info!("_|TT|_HW.VBUS,{},_|TE|_", vbus);
                    }
                    _ => {
                        passing = false;
                        log::error!("_|TT|_HW.ERROR,_|TE|_")
                    }
                };
                if passing {
                    log::info!("_|TT|_HW.PASS,_|TE|_");
                } else {
                    log::info!("_|TT|_HW.FAIL,_|TE|_");
                }
            }
            #[cfg(feature = "misc-test")]
            "wdt" => {
                let mut wdt = bao1x_hal::wdt::Wdt::new();
                wdt.enable((43750000 / 2) * 10, true);
                log::info!("wdt enabled, 10 seconds to reboot?");
            }
            #[cfg(feature = "misc-test")]
            "wup" => {
                let rtc = bao1x_hal_service::Rtc::new();
                use chrono::{Duration, Utc};
                let rovers = rtc.set_wakeup(Utc::now() + Duration::seconds(30));
                log::info!("{:?}", rovers);
                for i in 0..30 {
                    log::info!("waiting {}", i);
                    _env.ticktimer.sleep_ms(1000).ok();
                }
            }
            #[cfg(feature = "owc-test")]
            "owc" => {
                let keystore = keystore::Keystore::new(&_env.xns);
                log::info!("{:?}", keystore.get_owc_decoded::<bao1x_api::offsets::common::BoardTypeCoding>());

                log::info!("{:?}", keystore.get_owc_decoded::<bao1x_api::offsets::common::BootWaitCoding>());
                log::info!("{:?}", keystore.inc_owc_coded::<bao1x_api::offsets::common::BootWaitCoding>());
                log::info!("{:?}", keystore.get_owc_decoded::<bao1x_api::offsets::common::BootWaitCoding>());
            }
            #[cfg(feature = "wfi-stress-test")]
            "wfistress" => {
                use chrono::{Duration, Utc};
                use rand::Rng;
                let _ = std::thread::spawn({
                    move || {
                        log::info!("suspend/resume stress test active");

                        let xns = xous_names::XousNames::new().unwrap();
                        let susres = susres::Susres::new_without_hook(&xns).unwrap();
                        let tt = ticktimer::Ticktimer::new().unwrap();
                        tt.sleep_ms(1500).unwrap();
                        let mut iters = 0;
                        let mut timeouts = 0;
                        let rtc = bao1x_hal_service::Rtc::new();
                        let gfx = ux_api::service::gfx::Gfx::new(&xns).unwrap();
                        loop {
                            log::info!("suspend/resume cycle: {} ({} timeouts)", iters, timeouts);
                            let _rovers = rtc.set_wakeup(Utc::now() + Duration::seconds(6));
                            tt.sleep_ms(1000).ok();
                            gfx.set_power(false).unwrap();
                            match susres.initiate_suspend() {
                                Err(xous::Error::Timeout) => {
                                    timeouts += 1;
                                    log::warn!(
                                        "Couldn't suspend, a server was blocking suspend. ({}/{})\n",
                                        timeouts,
                                        iters
                                    );
                                    // wait enough time for the wakeup alarm to have happened before
                                    // resuming the cycle
                                    tt.sleep_ms(6000).unwrap();
                                }
                                Ok(_) => {}
                                Err(e) => {
                                    log::error!("Unknown error on suspend: {:?}", e);
                                }
                            }
                            gfx.set_power(true).unwrap();
                            tt.sleep_ms(4000 + (rand::thread_rng().gen::<u32>() % 7000) as usize).unwrap();
                            iters += 1;
                        }
                    }
                });
                write!(ret, "Starting suspend/resume stress test. Hard reboot required to exit.").unwrap();
            }
            _ => {
                write!(ret, "{}", helpstring).unwrap();
            }
        }

        Ok(Some(ret))
    }
}
