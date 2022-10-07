#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;
mod i2c;
#[cfg(any(feature="precursor", feature="renode"))]
mod llio_hw;
#[cfg(any(feature="precursor", feature="renode"))]
use llio_hw::*;

#[cfg(any(feature="hosted"))]
mod llio_hosted;
#[cfg(any(feature="hosted"))]
use llio_hosted::*;

use num_traits::*;
use xous_ipc::Buffer;
use xous::{CID, msg_scalar_unpack, msg_blocking_scalar_unpack};

use std::thread;

fn i2c_thread(i2c_sid: xous::SID) {
    let xns = xous_names::XousNames::new().unwrap();

    let handler_conn = xous::connect(i2c_sid).expect("couldn't make handler connection for i2c");
    let mut i2c = i2c::I2cStateMachine::new(handler_conn);

    // register a suspend/resume listener
    let sr_cid = xous::connect(i2c_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(Some(susres::SuspendOrder::Later), &xns, I2cOpcode::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");

    let mut suspend_pending_token: Option<usize> = None;
    log::trace!("starting i2c main loop");
    loop {
        let msg = xous::receive_message(i2c_sid).unwrap();
        log::trace!("i2c message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(I2cOpcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                if !i2c.is_busy() {
                    i2c.suspend();
                    susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                    i2c.resume();
                } else {
                    // stash the token, and we'll do the suspend once the I2C transaction is done.
                    suspend_pending_token = Some(token);
                }
            }),
            Some(I2cOpcode::IrqI2cTxrxWriteDone) => msg_scalar_unpack!(msg, _, _, _, _, {
                if let Some(token) = suspend_pending_token.take() {
                    i2c.suspend();
                    susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                    i2c.resume();
                }
                // I2C state machine handler irq result
                i2c.report_write_done();
            }),
            Some(I2cOpcode::IrqI2cTxrxReadDone) => msg_scalar_unpack!(msg, _, _, _, _, {
                if let Some(token) = suspend_pending_token.take() {
                    i2c.suspend();
                    susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                    i2c.resume();
                }
                // I2C state machine handler irq result
                i2c.report_read_done();
            }),
            Some(I2cOpcode::IrqI2cTrace) => {
                i2c.trace();
            },
            Some(I2cOpcode::I2cTxRx) => {
                i2c.initiate(msg);
            },
            Some(I2cOpcode::I2cIsBusy) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let busy = if i2c.is_busy() {1} else {0};
                xous::return_scalar(msg.sender, busy as _).expect("couldn't return I2cIsBusy");
            }),
            Some(I2cOpcode::Quit) => {
                log::info!("Received quit opcode, exiting!");
                break;
            }
            None => {
                log::error!("Received unknown opcode: {:?}", msg);
            }
        }
    }
    xns.unregister_server(i2c_sid).unwrap();
    xous::destroy_server(i2c_sid).unwrap();
}


#[derive(Copy, Clone, Debug)]
struct ScalarCallback {
    server_to_cb_cid: CID,
    cb_to_client_cid: CID,
    cb_to_client_id: u32,
}

fn main() -> ! {
    // very early on map in the GPIO base so we can have the right logging enabled
    let gpio_base = crate::log_init();

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // connections expected:
    // - codec
    // - GAM
    // - keyboard
    // - shellchat/sleep
    // - shellchat/environment
    // - shellchat/autoupdater
    // - spinor (for turning off wfi during writes)
    // - rootkeys (for reboots)
    // - oqc-test (for testing the vibe motor)
    // - net (for COM interrupt dispatch)
    // - pddb also allocates a connection, but then releases it, to read the DNA field.
    // We've migrated the I2C function out (which is arguably the most sensitive bit), so we can now set this more safely to unrestriced connection counts.
    let llio_sid = xns.register_name(api::SERVER_NAME_LLIO, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", llio_sid);

    // create the I2C handler thread
    // - codec
    // - time server
    // - llio
    // I2C can be used to set time, which can have security implications; we are more strict on counting who can have access to this resource.
    #[cfg(any(feature="precursor", feature="renode"))]
    let i2c_sid = xns.register_name(api::SERVER_NAME_I2C, Some(3)).expect("can't register I2C thread");
    #[cfg(any(feature="hosted"))]
    let i2c_sid = xns.register_name(api::SERVER_NAME_I2C, Some(1)).expect("can't register I2C thread");
    log::trace!("registered I2C thread with NS -- {:?}", i2c_sid);
    let _ = thread::spawn({
        let i2c_sid = i2c_sid.clone();
        move || {
            i2c_thread(i2c_sid);
        }
    });

    // Create a new llio object
    let handler_conn = xous::connect(llio_sid).expect("can't create IRQ handler connection");
    let mut llio = Llio::new(handler_conn, gpio_base);
    llio.ec_power_on(); // ensure this is set correctly; if we're on, we always want the EC on.

    if cfg!(feature = "wfi_off") {
        log::warn!("WFI is overridden at boot -- automatic power savings is OFF!");
        llio.wfi_override(true);
    }

    // register a suspend/resume listener
    let sr_cid = xous::connect(llio_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(Some(susres::SuspendOrder::Late), &xns, Opcode::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");
    let mut latest_activity = 0;

    let mut usb_cb_conns: [Option<ScalarCallback>; 32] = [None; 32];
    let mut com_cb_conns: [Option<ScalarCallback>; 32] = [None; 32];
    let mut gpio_cb_conns: [Option<ScalarCallback>; 32] = [None; 32];

    // create a self-connection to I2C to handle the public, non-security sensitive RTC API calls
    let mut i2c = llio::I2c::new(&xns);
    let tt = ticktimer_server::Ticktimer::new().unwrap();

    log::trace!("starting main loop");
    loop {
        let msg = xous::receive_message(llio_sid).unwrap();
        log::trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                llio.suspend();
                #[cfg(feature="tts")]
                llio.tts_sleep_indicate(); // this happens after the suspend call because we don't want the sleep indicator to be restored on resume
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                llio.resume();
                #[cfg(feature="tts")]
                llio.vibe(VibePattern::Double);
            }),
            Some(Opcode::CrgMode) => msg_scalar_unpack!(msg, _mode, _, _, _, {
                todo!("CrgMode opcode not yet implemented.");
            }),
            Some(Opcode::GpioDataOut) => msg_scalar_unpack!(msg, d, _, _, _, {
                llio.gpio_dout(d as u32);
            }),
            Some(Opcode::GpioDataIn) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.gpio_din() as usize).expect("couldn't return gpio data in");
            }),
            Some(Opcode::GpioDataDrive) => msg_scalar_unpack!(msg, d, _, _, _, {
                llio.gpio_drive(d as u32);
            }),
            Some(Opcode::GpioIntMask) => msg_scalar_unpack!(msg, d, _, _, _, {
                llio.gpio_int_mask(d as u32);
            }),
            Some(Opcode::GpioIntAsFalling) => msg_scalar_unpack!(msg, d, _, _, _, {
                llio.gpio_int_as_falling(d as u32);
            }),
            Some(Opcode::GpioIntPending) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.gpio_int_pending() as usize).expect("couldn't return gpio pending vector");
            }),
            Some(Opcode::GpioIntEna) => msg_scalar_unpack!(msg, d, _, _, _, {
                llio.gpio_int_ena(d as u32);
            }),
            Some(Opcode::DebugPowerdown) => msg_scalar_unpack!(msg, arg, _, _, _, {
                let ena = if arg == 0 {false} else {true};
                llio.debug_powerdown(ena);
            }),
            Some(Opcode::DebugWakeup) => msg_scalar_unpack!(msg, arg, _, _, _, {
                let ena = if arg == 0 {false} else {true};
                llio.debug_wakeup(ena);
            }),
            Some(Opcode::UartMux) => msg_scalar_unpack!(msg, mux, _, _, _, {
                llio.set_uart_mux(mux.into());
            }),
            Some(Opcode::InfoDna) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let (val1, val2) = llio.get_info_dna();
                xous::return_scalar2(msg.sender, val1, val2).expect("couldn't return DNA");
            }),
            Some(Opcode::InfoGit) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let (val1, val2) = llio.get_info_git();
                xous::return_scalar2(msg.sender, val1, val2).expect("couldn't return Git");
            }),
            Some(Opcode::InfoPlatform) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let (val1, val2) = llio.get_info_platform();
                xous::return_scalar2(msg.sender, val1, val2).expect("couldn't return Platform");
            }),
            Some(Opcode::InfoTarget) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let (val1, val2) = llio.get_info_target();
                xous::return_scalar2(msg.sender, val1, val2).expect("couldn't return Target");
            }),
            Some(Opcode::PowerAudio) => msg_blocking_scalar_unpack!(msg, power_on, _, _, _, {
                if power_on == 0 {
                    llio.power_audio(false);
                } else {
                    llio.power_audio(true);
                }
                xous::return_scalar(msg.sender, 0).expect("couldn't confirm audio power was set");
            }),
            Some(Opcode::PowerCrypto) => msg_blocking_scalar_unpack!(msg, power_on, _, _, _, {
                if power_on == 0 {
                    llio.power_crypto(false);
                } else {
                    llio.power_crypto(true);
                }
                xous::return_scalar(msg.sender, 0).expect("couldn't confirm crypto power was set");
            }),
            Some(Opcode::WfiOverride) => msg_blocking_scalar_unpack!(msg, override_, _, _, _, {
                if override_ == 0 {
                    llio.wfi_override(false);
                } else {
                    llio.wfi_override(true);
                }
                xous::return_scalar(msg.sender, 0).expect("couldn't confirm wfi override was updated");
            }),
            Some(Opcode::PowerCryptoStatus) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let (_, sha, engine, force) = llio.power_crypto_status();
                let mut ret = 0;
                if sha { ret |= 1 };
                if engine { ret |= 2 };
                if force { ret |= 4 };
                xous::return_scalar(msg.sender, ret).expect("couldn't return crypto unit power status");
            }),
            Some(Opcode::PowerSelf) => msg_scalar_unpack!(msg, power_on, _, _, _, {
                if power_on == 0 {
                    llio.power_self(false);
                } else {
                    llio.power_self(true);
                }
            }),
            Some(Opcode::PowerBoostMode) => msg_scalar_unpack!(msg, power_on, _, _, _, {
                if power_on == 0 {
                    llio.power_boost_mode(false);
                } else {
                    llio.power_boost_mode(true);
                }
            }),
            Some(Opcode::EcSnoopAllow) => msg_scalar_unpack!(msg, power_on, _, _, _, {
                if power_on == 0 {
                    llio.ec_snoop_allow(false);
                } else {
                    llio.ec_snoop_allow(true);
                }
            }),
            Some(Opcode::EcReset) => msg_scalar_unpack!(msg, _, _, _, _, {
                llio.ec_reset();
            }),
            Some(Opcode::EcPowerOn) => msg_scalar_unpack!(msg, _, _, _, _, {
                llio.ec_power_on();
            }),
            Some(Opcode::SelfDestruct) => msg_scalar_unpack!(msg, code, _, _, _, {
                llio.self_destruct(code as u32);
            }),
            Some(Opcode::Vibe) => msg_scalar_unpack!(msg, pattern, _, _, _, {
                llio.vibe(pattern.into());
            }),
            Some(Opcode::AdcVbus) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.xadc_vbus() as _).expect("couldn't return Xadc");
            }),
            Some(Opcode::AdcVccInt) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.xadc_vccint() as _).expect("couldn't return Xadc");
            }),
            Some(Opcode::AdcVccAux) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.xadc_vccaux() as _).expect("couldn't return Xadc");
            }),
            Some(Opcode::AdcVccBram) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.xadc_vccbram() as _).expect("couldn't return Xadc");
            }),
            Some(Opcode::AdcUsbN) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.xadc_usbn() as _).expect("couldn't return Xadc");
            }),
            Some(Opcode::AdcUsbP) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.xadc_usbp() as _).expect("couldn't return Xadc");
            }),
            Some(Opcode::AdcTemperature) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.xadc_temperature() as _).expect("couldn't return Xadc");
            }),
            Some(Opcode::AdcGpio5) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.xadc_gpio5() as _).expect("couldn't return Xadc");
            }),
            Some(Opcode::AdcGpio2) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.xadc_gpio2() as _).expect("couldn't return Xadc");
            }),
            Some(Opcode::EventUsbAttachSubscribe) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let hookdata = buffer.to_original::<ScalarHook, _>().unwrap();
                do_hook(hookdata, &mut usb_cb_conns);
            }
            Some(Opcode::EventComSubscribe) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let hookdata = buffer.to_original::<ScalarHook, _>().unwrap();
                do_hook(hookdata, &mut com_cb_conns);
            }
            Some(Opcode::GpioIntSubscribe) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let hookdata = buffer.to_original::<ScalarHook, _>().unwrap();
                do_hook(hookdata, &mut gpio_cb_conns);
            }
            Some(Opcode::EventComEnable) => msg_scalar_unpack!(msg, ena, _, _, _, {
                if ena == 0 {
                    llio.com_int_ena(false);
                } else {
                    llio.com_int_ena(true);
                }
            }),
            Some(Opcode::EventUsbAttachEnable) => msg_scalar_unpack!(msg, ena, _, _, _, {
                if ena == 0 {
                    llio.usb_int_ena(false);
                } else {
                    llio.usb_int_ena(true);
                }
            }),
            Some(Opcode::EventComHappened) => {
                send_event(&com_cb_conns, 0);
            },
            Some(Opcode::EventUsbHappened) => {
                send_event(&usb_cb_conns, 0);
            },
            Some(Opcode::GpioIntHappened) => msg_scalar_unpack!(msg, channel, _, _, _, {
                send_event(&gpio_cb_conns, channel as usize);
            }),
            Some(Opcode::EventActivityHappened) => msg_scalar_unpack!(msg, activity, _, _, _, {
                log::debug!("activity: {}", activity);
                latest_activity = activity as u32;
            }),
            Some(Opcode::GetActivity) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                #[cfg(any(feature="precursor", feature="renode"))]
                {
                    let period = llio.activity_get_period() as u32;
                    // log::debug!("activity/period: {}/{}, {:.2}%", latest_activity, period, (latest_activity as f32 / period as f32) * 100.0);
                    xous::return_scalar2(msg.sender, latest_activity as usize, period as usize).expect("couldn't return activity");
                }
                #[cfg(any(feature="hosted"))] // fake an activity
                {
                    let period = 12_000;
                    xous::return_scalar2(msg.sender, latest_activity as usize, period as usize).expect("couldn't return activity");
                    latest_activity += period / 20;
                    latest_activity %= period;
                }
            }),
            Some(Opcode::SetWakeupAlarm) => msg_blocking_scalar_unpack!(msg, delay, _, _, _, {
                if delay > u8::MAX as usize {
                    log::error!("Wakeup must be no longer than {} secs in the future", u8::MAX);
                    xous::return_scalar(msg.sender, 1).expect("couldn't return to caller");
                    continue;
                }
                let seconds = delay as u8;
                // make sure battery switchover is enabled, otherwise we won't keep time when power goes off
                i2c.i2c_write(ABRTCMC_I2C_ADR, ABRTCMC_CONTROL3, &[(Control3::BATT_STD_BL_EN).bits()]).expect("RTC access error");
                // set clock units to 1 second, output pulse length to ~218ms
                i2c.i2c_write(ABRTCMC_I2C_ADR, ABRTCMC_TIMERB_CLK, &[(TimerClk::CLK_1_S | TimerClk::PULSE_218_MS).bits()]).expect("RTC access error");
                // program elapsed time
                i2c.i2c_write(ABRTCMC_I2C_ADR, ABRTCMC_TIMERB, &[seconds]).expect("RTC access error");
                // enable timerb countdown interrupt, also clears any prior interrupt flag
                let control2 = (Control2::COUNTDOWN_B_INT).bits();
                i2c.i2c_write(ABRTCMC_I2C_ADR, ABRTCMC_CONTROL2, &[control2]).expect("RTC access error");
                // turn on the timer proper -- the system will wakeup in 5..4..3....
                let config = (Config::CLKOUT_DISABLE | Config::TIMER_B_ENABLE).bits();
                i2c.i2c_write(ABRTCMC_I2C_ADR, ABRTCMC_CONFIG, &[config]).expect("RTC access error");
                xous::return_scalar(msg.sender, 0).expect("couldn't return to caller");
            }),
            Some(Opcode::ClearWakeupAlarm) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                // make sure battery switchover is enabled, otherwise we won't keep time when power goes off
                i2c.i2c_write(ABRTCMC_I2C_ADR, ABRTCMC_CONTROL3, &[(Control3::BATT_STD_BL_EN).bits()]).expect("RTC access error");
                let config = Config::CLKOUT_DISABLE.bits();
                // turn off RTC wakeup timer, in case previously set
                i2c.i2c_write(ABRTCMC_I2C_ADR, ABRTCMC_CONFIG, &[config]).expect("RTC access error");
                // clear my interrupts and flags
                let control2 = 0;
                i2c.i2c_write(ABRTCMC_I2C_ADR, ABRTCMC_CONTROL2, &[control2]).expect("RTC access error");
                xous::return_scalar(msg.sender, 0).expect("couldn't return to caller");
            }),
            #[cfg(any(feature="precursor", feature="renode"))]
            Some(Opcode::GetRtcValue) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                // There is a possibility that the RTC hardware is actually in an invalid state.
                // Thus, this will return a u64 which is formatted as follows:
                // [63] - invalid (0 for valid, 1 for invalid)
                // [62:0] - time in seconds
                // This is okay because 2^63 is much larger than the total number of seconds trackable by the RTC hardware.
                // The RTC hardware can only count up to 100 years before rolling over, which is 3.1*10^9 seconds.
                // Note that we start the RTC at somewhere between 0-10 years, so in practice, a user can expect between 90-100 years
                // of continuous uptime service out of the RTC.
                let mut settings = [0u8; 8];
                let mut success = false;
                while !success {
                    // retry loop is necessary because this function can get called during "congested" periods
                    match i2c.i2c_read(ABRTCMC_I2C_ADR, ABRTCMC_CONTROL3, &mut settings) {
                        Ok(llio::I2cStatus::ResponseReadOk) => success = true,
                        Err(xous::Error::ServerQueueFull) => {
                            success = false;
                            // give it a short pause before trying again, to avoid hammering the I2C bus at busy times
                            tt.sleep_ms(38).unwrap();
                        },
                        _ => {
                            log::error!("Couldn't read seconds from RTC!");
                            xous::return_scalar2(msg.sender, 0x8000_0000, 0).expect("couldn't return to caller");
                            break;
                        },
                    };
                }
                if !success {
                    continue
                }
                log::debug!("GetRtcValue regs: {:?}", settings);
                let total_secs = match rtc_to_seconds(&settings) {
                    Some(s) => s,
                    None => {
                        xous::return_scalar2(msg.sender, 0x8000_0000, 0).expect("couldn't return to caller");
                        continue;
                    }
                };
                xous::return_scalar2(msg.sender,
                    ((total_secs >> 32) & 0xFFFF_FFFF) as usize,
                    (total_secs & 0xFFFF_FFFF) as usize,
                ).expect("couldn't return to caller");
            }),
            #[cfg(any(feature="hosted"))]
            Some(Opcode::GetRtcValue) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                use chrono::prelude::*;
                let now = Local::now();
                let total_secs = now.timestamp_millis() / 1000 - 148409348; // sets the offset to something like 1974, which is roughly where an RTC value ends up in reality
                xous::return_scalar2(msg.sender,
                    ((total_secs >> 32) & 0xFFFF_FFFF) as usize,
                    (total_secs & 0xFFFF_FFFF) as usize,
                ).expect("couldn't return to caller");
                // use the tt variable so we don't get a warning
                let _ = tt.elapsed_ms();
            }),
            Some(Opcode::Quit) => {
                log::info!("Received quit opcode, exiting.");
                let dropconn = xous::connect(i2c_sid).unwrap();
                xous::send_message(dropconn,
                    xous::Message::new_scalar(I2cOpcode::Quit.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
                unsafe{xous::disconnect(dropconn).unwrap();}
                break;
            }
            None => {
                log::error!("couldn't convert opcode: {:?}", msg);
            }
        }
    }
    log::trace!("main loop exit, destroying servers");
    unhook(&mut com_cb_conns);
    unhook(&mut usb_cb_conns);
    unhook(&mut gpio_cb_conns);
    xns.unregister_server(llio_sid).unwrap();
    xous::destroy_server(llio_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}

fn do_hook(hookdata: ScalarHook, cb_conns: &mut [Option<ScalarCallback>; 32]) {
    let (s0, s1, s2, s3) = hookdata.sid;
    let sid = xous::SID::from_u32(s0, s1, s2, s3);
    let server_to_cb_cid = xous::connect(sid).unwrap();
    let cb_dat = Some(ScalarCallback {
        server_to_cb_cid,
        cb_to_client_cid: hookdata.cid,
        cb_to_client_id: hookdata.id,
    });
    let mut found = false;
    for entry in cb_conns.iter_mut() {
        if entry.is_none() {
            *entry = cb_dat;
            found = true;
            break;
        }
    }
    if !found {
        log::error!("ran out of space registering callback");
    }
}
fn unhook(cb_conns: &mut [Option<ScalarCallback>; 32]) {
    for entry in cb_conns.iter_mut() {
        if let Some(scb) = entry {
            xous::send_message(scb.server_to_cb_cid,
                xous::Message::new_blocking_scalar(EventCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0)
            ).unwrap();
            unsafe{xous::disconnect(scb.server_to_cb_cid).unwrap();}
        }
        *entry = None;
    }
}
fn send_event(cb_conns: &[Option<ScalarCallback>; 32], which: usize) {
    for entry in cb_conns.iter() {
        if let Some(scb) = entry {
            // note that the "which" argument is only used for GPIO events, to indicate which pin had the event
            match xous::try_send_message(scb.server_to_cb_cid,
                xous::Message::new_scalar(EventCallback::Event.to_usize().unwrap(),
                   scb.cb_to_client_cid as usize, scb.cb_to_client_id as usize, which, 0)
            ) {
                Ok(_) => {},
                Err(e) => {
                    match e {
                        xous::Error::ServerQueueFull => {
                            // this triggers if an interrupt storm happens. This could be perfectly natural and/or
                            // "expected", and the "best" behavior is probably to drop the events, but leave a warning.
                            // Examples of this would be a ping flood overwhelming the network stack.
                            log::warn!("Attempted to send event, but destination queue is full. Event was dropped: {:?}", scb);
                        }
                        xous::Error::ServerNotFound => {
                            log::warn!("Event callback subscriber has died. Event was dropped: {:?}", scb);
                        }
                        _ => {
                            log::error!("Callback error {:?}: {:?}", e, scb);
                        }
                    }
                }
            }
        };
    }
}

// run with `cargo test -- --nocapture --test-threads=1`:
//   --nocapture to see the print output (while debugging)
//   --test-threads=1 so we can see the output in sequence
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use llio::rtc_to_seconds;
    use rand::Rng;

    fn to_bcd(binary: u8) -> u8 {
        let mut lsd: u8 = binary % 10;
        if lsd > 9 {
            lsd = 9;
        }
        let mut msd: u8 = binary / 10;
        if msd > 9 {
            msd = 9;
        }
        (msd << 4) | lsd
    }

    #[test]
    fn test_rtc_to_secs() {
        let mut rng = rand::thread_rng();
        let rtc_base = DateTime::<Utc>::from_utc(chrono::NaiveDate::from_ymd(2000, 1, 1)
        .and_hms(0, 0, 0), Utc);

        // test every year, every month, every day, with a random time stamp
        for year in 0..99 {
            for month in 1..=12 {
                let days = match month {
                    1 => 1..=31,
                    2 => if (year % 4) == 0 {
                        1..=29
                    } else {
                        1..=28
                    }
                    3 => 1..=31,
                    4 => 1..=30,
                    5 => 1..=31,
                    6 => 1..=30,
                    7 => 1..=31,
                    8 => 1..=31,
                    9 => 1..=30,
                    10 => 1..=31,
                    11 => 1..=30,
                    12 => 1..=31,
                    _ => {panic!("invalid month")},
                };
                for day in days {
                    let h = rng.gen_range(0..24);
                    let m = rng.gen_range(0..60);
                    let s = rng.gen_range(0..60);
                    let rtc_test = DateTime::<Utc>::from_utc(
                        chrono::NaiveDate::from_ymd(2000 + year, month, day)
                    .and_hms(h, m, s), Utc);

                    let diff = rtc_test.signed_duration_since(rtc_base);
                    let settings = [
                        (Control3::BATT_STD_BL_EN).bits(),
                        to_bcd(s as u8),
                        to_bcd(m as u8),
                        to_bcd(h as u8),
                        to_bcd(day as u8),
                        0,
                        to_bcd(month as u8),
                        to_bcd(year as u8),
                    ];
                    if diff.num_seconds() != rtc_to_seconds(&settings).unwrap() as i64 {
                        println!("{} vs {}", diff.num_seconds(), rtc_to_seconds(&settings).unwrap());
                        println!("Duration to {}/{}/{}-{}:{}:{} -- {}",
                            2000 + year,
                            month,
                            day,
                            h, m, s,
                            diff.num_seconds()
                        );
                    }
                    assert!(diff.num_seconds() == rtc_to_seconds(&settings).unwrap() as i64);
                }
            }
        }
    }
}
