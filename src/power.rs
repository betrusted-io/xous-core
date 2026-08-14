use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use bao1x_api::{IoIrq, IoxHal, IoxValue};
use bao1x_hal::lis2dh12::{InterruptSource, Lis2dh12, Orientation, regs};
use bao1x_hal::{axp2101::VbusIrq, clocks::ClockOp, i2c::I2c};
use bao1x_hal_service::Rtc;
use chrono::Utc;
use dc34_api::*;
use num_traits::ToPrimitive;

const POWER_POLL_INTERVAL_MS: usize = 2500;
const WDT_FEED_INTERVAL_MS: usize = POWER_POLL_INTERVAL_MS * 5;
// this gives some margin for the keypress to "catch up" in case both motion and
// keypress interrupts are simultaneously fired as a wakeup event
const MOTION_IRQ_MARGIN_MS: u64 = 1000;
const WFI_IDLE_SEC_INIT: u64 = 60;
const WFI_MIN_SEC: usize = 5;
#[cfg(not(feature = "uber"))]
const DEEP_SLEEP_SEC: i64 = 25 * 60;
#[cfg(feature = "uber")]
const DEEP_SLEEP_SEC: i64 = 10 * 60 * 60;

fn setup_accel(accel: &mut Lis2dh12, i2c: &mut I2c) -> Result<(), xous::Error> {
    let saved_ctrl3 = accel.read_register(i2c, regs::CTRL_REG3)?;

    // Latch both INT1 (motion) and INT2 (orientation) until their SRC regs are read
    // 0x08 = LIR_INT1, 0x02 = LIR_INT2
    // orientation is level-triggered so no need to latch it
    accel.write_register(i2c, regs::CTRL_REG5, 0x08 /* | 0x02 */)?;

    // -- INT1: motion detection -----------------------------------------------
    // OR combination, all axes high/low enabled
    accel.write_register(i2c, regs::INT1_CFG, 0x7F)?;

    // -- sensitivity tuning for wake ------------------------------------------
    /* // original tuning - fairly sensitive to taps, but misses longer, slower motions
    // Threshold: 16mg/LSB at ±2g - low enough to catch gentle movement
    accel.write_register(i2c, regs::INT1_THS, 10)?;
    // Minimum duration before interrupt fires
    accel.write_register(i2c, regs::INT1_DURATION, 1)?;
    */

    // --- newer tuning ---
    // At 25Hz, DURATION=2 → 80ms minimum - should be better at rejecting
    // brief non-walking transients without missing steps
    // CTRL_REG1: 25Hz, normal mode, XYZ enabled
    // [7:4]=0011 (25Hz), [2:0]=111
    accel.write_register(i2c, regs::CTRL_REG1, 0x37)?;
    accel.write_register(i2c, regs::INT1_THS, 18)?;
    accel.write_register(i2c, regs::INT1_DURATION, 3)?;
    /*
    Tuning loop:
      Start at THS=20, DUR=2
      - If breathing triggers → raise THS to 22, not duration
      - If slow walking misses → lower THS to 18 first, then DUR
      - If chair creaks/fabric rustle triggers it → raise DUR to 4–5, not THS
    Threshold controls amplitude sensitivity, duration controls how sustained the motion
    needs to be.
    */
    // -- end sensitivity tuning for wake --------------------------------------

    // -- INT2: 6D orientation detection ---------------------------------------
    // AOI=0, 6D=1, all six directions enabled - fires on any face change - narrowed on state change
    accel.write_register(i2c, regs::INT2_CFG, 0x7F)?;
    // ~500mg threshold - high enough to stay stable while wearing, low enough
    // to detect a deliberate flip. Tune between 0x10 (250mg) and 0x30 (750mg).
    accel.write_register(i2c, regs::INT2_THS, 0x30)?;
    // Small debounce: at 25Hz, value=8 -> 320ms minimum hold before triggering.
    // Prevents a mid-flip transient from double-firing.
    accel.write_register(i2c, regs::INT2_DURATION, 8)?;

    // -- Shared config ---------------------------------------------------------
    accel.set_interrupt_polarity(i2c, bao1x_hal::lis2dh12::InterruptPolarity::ActiveHigh)?;

    // CTRL_REG2: HPF enabled for INT1 only (HPEN1=1, HPEN2=0).
    // INT2/6D intentionally gets raw gravity - that's its reference signal.
    accel.write_register(i2c, regs::CTRL_REG2, 0b00_00_0001)?;
    // Read REFERENCE to reset the HPF and zero it against current orientation
    accel.read_register(i2c, regs::REFERENCE)?;

    // CTRL_REG3: route both IA1 (motion) and IA2 (orientation) to INT1 pin
    // bit6=I1_IA1, bit5=I1_IA2
    // accel.write_register(i2c, regs::CTRL_REG3, saved_ctrl3 | 0x40 | 0x20)?;
    // initially, only enable orientation accelerometer
    accel.write_register(i2c, regs::CTRL_REG3, saved_ctrl3 | 0x20)?;

    // Clear any pending interrupts on both engines
    let _ = accel.read_register(i2c, regs::INT1_SRC)?;
    let _ = accel.read_register(i2c, regs::INT2_SRC)?;

    Ok(())
}

fn accel_enable_int(accel: &mut Lis2dh12, i2c: &mut I2c, enable: bool) -> Result<(), xous::Error> {
    // Clear any pending interrupts on both engines
    let _ = accel.read_register(i2c, regs::INT1_SRC)?;
    let _ = accel.read_register(i2c, regs::INT2_SRC)?;
    let saved_ctrl3 = accel.read_register(i2c, regs::CTRL_REG3)?;
    if enable {
        log::debug!("enable motion accel");
        // bit6=I1_IA1 (motion), bit5=I1_IA2 (orientation)
        // just enable the motion interrupt but don't arm the tilt (otherwise it'll trigger immediately and
        // wake us up)
        accel.write_register(i2c, regs::CTRL_REG3, saved_ctrl3 | 0x40 | 0x20)?;
        accel.reset_highpass(i2c)?;
    } else {
        // always allow orientation interrupts; disable motion
        log::debug!("disable motion accel");
        accel.write_register(i2c, regs::INT2_CFG, 0x7F)?; // re-arm all faces, we're awake and want to get orientation
        accel.write_register(i2c, regs::CTRL_REG3, saved_ctrl3 & !(0x40/* | 0x20 */))?;
    }
    Ok(())
}

fn accel_pause_int(accel: &mut Lis2dh12, i2c: &mut I2c, pause: bool) -> Result<(), xous::Error> {
    // Clear any pending interrupts on both engines
    let _ = accel.read_register(i2c, regs::INT1_SRC)?;
    let _ = accel.read_register(i2c, regs::INT2_SRC)?;
    let saved_ctrl3 = accel.read_register(i2c, regs::CTRL_REG3)?;
    log::debug!("pause accel irq");
    if pause {
        accel.write_register(i2c, regs::CTRL_REG3, saved_ctrl3 & !(0x40 | 0x20))?;
    } else {
        accel.write_register(i2c, regs::INT2_CFG, 0x7F)?; // re-arm all faces, we're awake and want to get orientation
        accel.write_register(i2c, regs::CTRL_REG3, saved_ctrl3 | 0x20)?;
    }
    Ok(())
}

pub fn power_manager(run_led_fade: Arc<AtomicBool>, plugged_in: Arc<AtomicBool>, wdt: usize) -> ! {
    // safety: this is "moved" from the original outer location where the WDT is accessed to set up
    // an early trigger into this loop, and thus all the safety conditions are met.
    let mut wdt = unsafe { bao1x_hal::wdt::Wdt::from_raw(wdt) };
    let xns = xous_names::XousNames::new().unwrap();
    let tt = ticktimer::Ticktimer::new().unwrap();

    // setup the VBUS/VBAT measurement pins
    let iox = bao1x_api::IoxHal::new();
    let adc = bao1x_hal_service::Adc::new();
    use bao1x_api::IoSetup;
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
    let dummy = adc.read_raw(bao1x_hal::udma::AdcSource::Ext(bao1x_hal::udma::AdcExtChannel::Adc3), Some(8));
    log::info!("ADC pipe-clearing value {}", dummy);

    let mut i2c = I2c::new();
    // Initialize
    let mut accel = Lis2dh12::new(&mut i2c).ok();
    if let Some(a) = &mut accel {
        setup_accel(a, &mut i2c).unwrap();
    } else {
        log::warn!("No accelerometer found!");
    }

    let susres = susres::Susres::new_without_hook(&xns).unwrap();
    let gfx = ux_api::service::gfx::Gfx::new(&xns).unwrap();

    let iox_hal = IoxHal::new();
    let sid = xns.register_name(POWER_MANAGER_SERVER, None).unwrap();

    let kbd = bao1x_api::keyboard::Keyboard::new(&xns).unwrap();
    kbd.register_listener(POWER_MANAGER_SERVER, PowerManagerOp::KeyPress.to_u32().unwrap() as usize);

    let mut orientation = Orientation::FaceUp;
    if let Some(a) = accel.as_mut() {
        let _ = iox_hal.set_irq_pin(
            bao1x_api::IoxPort::PC,
            15,
            bao1x_api::IoxValue::Low,
            POWER_MANAGER_SERVER,
            PowerManagerOp::MotionIrq.to_usize().unwrap(),
        );
        log::info!("Accelerometer interrupt pin setup");
        if let Ok(o) = a.get_orientation(&mut i2c) {
            orientation = o;
        }
    }
    let vbus_io = (bao1x_api::IoxPort::PA, 4u8);
    let mut vbus_state = iox.get_gpio_pin_value(vbus_io.0, vbus_io.1);
    plugged_in.store(vbus_state == IoxValue::High, Ordering::SeqCst);
    let vbus_irq_index = iox_hal
        .set_irq_pin(
            vbus_io.0,
            vbus_io.1,
            !vbus_state,
            POWER_MANAGER_SERVER,
            PowerManagerOp::VbusIrq.to_usize().unwrap(),
        )
        .expect("Couldn't claim Vbus IRQ");

    let cid = xous::connect(sid).unwrap();
    std::thread::spawn({
        let cid = cid;
        move || {
            let tt = ticktimer::Ticktimer::new().unwrap();
            loop {
                tt.sleep_ms(POWER_POLL_INTERVAL_MS).ok();
                xous::try_send_message(
                    cid,
                    xous::Message::new_scalar(PowerManagerOp::Poll.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .ok();
            }
        }
    });

    let usb = usb_bao1x::UsbHid::new();
    let led_conn = xns.request_connection_blocking(dc34_api::LED_SERVER).unwrap();

    let rtc = bao1x_hal_service::Rtc::new();
    let mut alarm_set = false;

    let susres_conn =
        xns.request_connection_blocking(susres::api::SERVER_NAME_SUSRES).expect("Can't connect to SUSRES");
    let pclk_ms = get_wdt_clk_ms(susres_conn);
    // this re-enables the WDT with the system-provided pclk value (previously enabled on boot
    // with a hard-coded assumed value)
    wdt.enable((pclk_ms * WDT_FEED_INTERVAL_MS) as u32, true);

    #[cfg(feature = "wfi-stress-test")]
    // this loop suppresses normal power management so that the stress test can operate
    loop {
        let mut msg_opt = None;
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        let opcode = {
            let msg = msg_opt.as_mut().unwrap();
            num_traits::FromPrimitive::from_usize(msg.body.id()).unwrap_or(PowerManagerOp::Invalid)
        };
        match opcode {
            PowerManagerOp::Invalid => {
                log::info!("breaking out of stress test holding pattern");
                break;
            }
            _ => {
                log::info!("ignoring opcode: {:?}", opcode)
            }
        }
    }
    let vault_conn = xns.request_connection_blocking(SERVER_NAME_VAULT2).unwrap();

    let mut pwr_mgr_enabled = false;
    let mut booted = false;
    let mut wfi_awaiting_keypress = false;
    let mut idle_sec = WFI_IDLE_SEC_INIT;
    let mut force_wfi = false;
    let mut force_deep_sleep = false;
    let mut power_off = false;
    let mut screen_off_requested = false;

    let mut last_action_time_ms = tt.elapsed_ms();
    let mut msg_opt = None;
    loop {
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        let opcode = {
            let msg = msg_opt.as_mut().unwrap();
            num_traits::FromPrimitive::from_usize(msg.body.id()).unwrap_or(PowerManagerOp::Invalid)
        };
        log::debug!("{:?}", opcode);
        match opcode {
            PowerManagerOp::Enable => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    if scalar.arg1 != 0 {
                        pwr_mgr_enabled = true;
                    } else {
                        pwr_mgr_enabled = false;
                    }
                    if scalar.arg2 > WFI_MIN_SEC {
                        idle_sec = scalar.arg2 as u64;
                    }
                }
            }
            PowerManagerOp::Poll => {
                if booted {
                    // keep the system alive, once we've successfully booted; if we don't boot
                    // before the first WDT timeout, this will force a reboot.
                    wdt.feed();
                }
                let now_ms = tt.elapsed_ms();
                // check for consistency and fix any rollover/time-setting bugs
                if last_action_time_ms > now_ms {
                    last_action_time_ms = now_ms;
                }
                // disable power management if VBUS is plugged in
                if !pwr_mgr_enabled || vbus_state == IoxValue::High {
                    // this effectively disables the if statement below by claiming
                    // an action has *always* happened
                    last_action_time_ms = now_ms;

                    if alarm_set {
                        // clear the RTC alarm. The alarm_set flag just makes things a little
                        // more efficient so we're not redundantly clearing the alarm every
                        // poll
                        rtc.clear_wakeup();
                        alarm_set = false;
                    }
                }
                if !wfi_awaiting_keypress && (now_ms - last_action_time_ms > idle_sec * 1000)
                    || wfi_awaiting_keypress && (now_ms - last_action_time_ms > MOTION_IRQ_MARGIN_MS)
                    || force_wfi
                {
                    force_wfi = false; // always reset this here
                    gfx.set_power(false).ok();
                    // try skip one key press - monkey patch/hack
                    xous::try_send_message(vault_conn, xous::Message::new_scalar(1026, 0, 0, 0, 0)).ok();
                    wfi_awaiting_keypress = true; // this tells the KeyPress handler we have to turn on the screen

                    wdt.disable();
                    // don't crash on suspend initiation failure - WDT is off! just report so we can find it
                    // in logs.
                    if susres.initiate_suspend().is_err() {
                        log::error!("**suspend err**");
                    };
                    // we idled, until a button was pressed
                    wdt.enable((pclk_ms * WDT_FEED_INTERVAL_MS) as u32, true);

                    // brief delay for everything to catch up
                    tt.sleep_ms(100).ok();
                    last_action_time_ms = now_ms;
                    // screen wake-up is delegated to KeyPress handler -
                    // this prevents the screen glitch on RTC wake event
                }
            }
            PowerManagerOp::PauseAccel => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    let pause = scalar.arg1 != 0;
                    if let Some(a) = &mut accel {
                        if pause {
                            accel_pause_int(a, &mut i2c, true).ok();
                        } else {
                            accel_pause_int(a, &mut i2c, false).unwrap();
                        }
                    }
                }
            }
            PowerManagerOp::MotionIrq => {
                if let Some(a) = &mut accel {
                    /*
                    // -- Read raw register values BEFORE clearing ----------------------
                    let int2_src_raw = a.read_register(&mut i2c, regs::INT2_SRC).unwrap();
                    let int1_src_raw = a.read_register(&mut i2c, regs::INT1_SRC).unwrap();

                    log::info!(
                        "INT1_SRC raw: 0x{:02X}  (active={}  XL={} XH={} YL={} YH={} ZL={} ZH={})",
                        int1_src_raw,
                        (int1_src_raw & 0x40) != 0,
                        (int1_src_raw & 0x01) != 0,
                        (int1_src_raw & 0x02) != 0,
                        (int1_src_raw & 0x04) != 0,
                        (int1_src_raw & 0x08) != 0,
                        (int1_src_raw & 0x10) != 0,
                        (int1_src_raw & 0x20) != 0,
                    );
                    log::info!(
                        "INT2_SRC raw: 0x{:02X}  (active={}  XL={} XH={} YL={} YH={} ZL={} ZH={})",
                        int2_src_raw,
                        (int2_src_raw & 0x40) != 0,
                        (int2_src_raw & 0x01) != 0,
                        (int2_src_raw & 0x02) != 0,
                        (int2_src_raw & 0x04) != 0,
                        (int2_src_raw & 0x08) != 0,
                        (int2_src_raw & 0x10) != 0,
                        (int2_src_raw & 0x20) != 0,
                    );

                    // -- Immediately re-read INT2_SRC to see if latch actually cleared --
                    // If this is still non-zero, the condition is re-asserting instantly
                    let int2_src_reread = a.read_register(&mut i2c, regs::INT2_SRC).unwrap();
                    log::info!("INT2_SRC re-read (should be 0x00 if cleared): 0x{:02X}", int2_src_reread);

                    // -- Raw accel values to see what gravity looks like right now ------
                    if let Ok((x, y, z)) = a.read_accel_mg(&mut i2c) {
                        log::info!(
                            "Accel mg: x={:6}  y={:6}  z={:6}  magnitude~={:6}",
                            x,
                            y,
                            z,
                            // rough integer magnitude for sanity check
                            (((x * x + y * y + z * z) as f32).sqrt()) as i32,
                        );
                    }

                    // -- Config register dump - verify nothing has drifted -------------
                    let ctrl_reg2 = a.read_register(&mut i2c, regs::CTRL_REG2).unwrap();
                    let ctrl_reg3 = a.read_register(&mut i2c, regs::CTRL_REG3).unwrap();
                    let ctrl_reg5 = a.read_register(&mut i2c, regs::CTRL_REG5).unwrap();
                    let int2_cfg = a.read_register(&mut i2c, regs::INT2_CFG).unwrap();
                    let int2_ths = a.read_register(&mut i2c, regs::INT2_THS).unwrap();
                    let int2_dur = a.read_register(&mut i2c, regs::INT2_DURATION).unwrap();
                    log::info!(
                        "CTRL_REG2=0x{:02X} CTRL_REG3=0x{:02X} CTRL_REG5=0x{:02X}",
                        ctrl_reg2,
                        ctrl_reg3,
                        ctrl_reg5
                    );
                    log::info!(
                        "INT2_CFG=0x{:02X} INT2_THS=0x{:02X} INT2_DUR=0x{:02X}",
                        int2_cfg,
                        int2_ths,
                        int2_dur
                    );
                    */

                    // must read to clear any pending interrupt
                    let int1_src = match a.read_int1_source(&mut i2c) {
                        Ok(i) => i,
                        // on error, retry, and if still an error force handling
                        Err(_) => a.read_int1_source(&mut i2c).unwrap_or(InterruptSource::from(0x40u8)),
                    };
                    let int2_src_raw = match a.read_register(&mut i2c, regs::INT2_SRC) {
                        Ok(i) => i,
                        // on error, retry, and if still an error force handling
                        Err(_) => a.read_register(&mut i2c, regs::INT2_SRC).unwrap_or(0x40u8),
                    };

                    if int1_src.active {
                        log::debug!("motion");
                        /* log::info!("Motion confirmed! {:?}", a.read_accel_mg(&mut i2c).unwrap()); */
                        a.reset_highpass(&mut i2c).unwrap();
                        // only enable deep sleep if we're on battery power
                        if vbus_state == IoxValue::Low {
                            // this pushes the alarm date out by the deep sleep time horizon
                            set_wakeup_alarm(&rtc);
                            alarm_set = true;
                        }
                    }
                    if (int2_src_raw & 0x40) != 0 {
                        // Determine which face just became dominant
                        let active_face_bit: u8 = int2_src_raw & 0x3F; // XL/XH/YL/YH/ZL/ZH

                        // Re-enable all faces EXCEPT the one currently active.
                        // This makes the engine fire only on a *transition away* from
                        // the current face, not continuously while sitting on it.
                        let new_cfg: u8 = 0x40          // AOI=0, 6D=1 - keep 6D mode
                                        | 0x3F          // all directions
                                        & !active_face_bit; // mask out the currently-satisfied direction
                        a.write_register(&mut i2c, regs::INT2_CFG, new_cfg).unwrap();
                        log::debug!(
                            "INT2_SRC=0x{:02X}, masking face bit 0x{:02X}, new INT2_CFG=0x{:02X}",
                            int2_src_raw,
                            active_face_bit,
                            new_cfg
                        );

                        if let Ok(o) = a.get_orientation(&mut i2c) {
                            log::debug!("tilt: {:?}", o);
                            if orientation != o && o != Orientation::Unknown {
                                log::info!("New orientation: {:?}", o);
                                orientation = o;
                                gfx.flip_screen(o == Orientation::FaceDown).ok();
                                kbd.flip_orientation(o == Orientation::FaceDown);
                                // adjust the "eyes" effect
                                xous::send_message(
                                    led_conn,
                                    xous::Message::new_scalar(
                                        LedManagerOp::JackEyes.to_usize().unwrap(),
                                        if orientation == Orientation::FaceDown { 1 } else { 0 },
                                        0,
                                        0,
                                        0,
                                    ),
                                )
                                .ok();

                                if orientation == Orientation::FaceDown {
                                    kbd.inject_key('🔽');
                                } else {
                                    kbd.inject_key('🔼');
                                }
                            }
                        }
                    }
                    if !(int1_src.active || (int2_src_raw & 0x40) != 0) {
                        log::warn!("*** NEITHER ***");
                    }
                }
            }
            PowerManagerOp::VbusIrq => {
                // check the current value, because we can have chatter after interrupts
                vbus_state = iox.get_gpio_pin_value(vbus_io.0, vbus_io.1);
                plugged_in.store(vbus_state == IoxValue::High, Ordering::SeqCst);
                // flip the edge trigger to opposite the current state
                iox_hal.update_irq_pin(POWER_MANAGER_SERVER, vbus_irq_index, Some(!vbus_state), None);

                // notify the USB stack of state changes
                xous::send_message(
                    usb.cid(),
                    xous::Message::new_blocking_scalar(
                        usb_bao1x::api::Opcode::PmicIrq.to_usize().unwrap(),
                        (if vbus_state == IoxValue::High { VbusIrq::Insert } else { VbusIrq::Remove }).into(),
                        0,
                        0,
                        0,
                    ),
                )
                .ok();

                // if not plugged in, make sure the RTC alarm is set so we go into deep sleep
                if vbus_state == IoxValue::Low {
                    set_wakeup_alarm(&rtc);
                }
            }
            PowerManagerOp::KeyPress => {
                if let Some(scalar) = msg_opt.as_ref().unwrap().body.scalar_message() {
                    let k = char::from_u32(scalar.arg1 as u32).unwrap_or('\u{0000}');
                    if k == '⏰' || force_deep_sleep {
                        force_deep_sleep = false; // always reset here
                        log::info!("Deep sleep trigger hit!");

                        if let Some(a) = &mut accel {
                            if !power_off {
                                // ensure accelerometer interrupts are enabled, that's the primary source of
                                // waking
                                accel_enable_int(a, &mut i2c, true).ok();
                            } else {
                                // prepare for extended power-off - disable all motion interrupts
                                // Clear any pending interrupts on both engines
                                let _ = a.read_register(&mut i2c, regs::INT1_SRC).ok();
                                let _ = a.read_register(&mut i2c, regs::INT2_SRC).ok();
                                let saved_ctrl3 = a.read_register(&mut i2c, regs::CTRL_REG3).unwrap();
                                a.write_register(&mut i2c, regs::CTRL_REG3, saved_ctrl3 & !(0x40 | 0x20))
                                    .ok();
                            }
                        }
                        // turn off screen
                        gfx.set_power(false).unwrap();

                        wdt.disable();
                        xous::send_message(
                            susres_conn,
                            xous::Message::new_scalar(
                                susres::api::Opcode::PlatformSpecific.to_usize().unwrap(),
                                ClockOp::DeepSleep.to_usize().unwrap(),
                                0,
                                0,
                                0,
                            ),
                        )
                        .ok();
                        log::error!("should have gone to deep sleep");
                        // -- Execution should have diverged here - system is off --
                    } else {
                        if screen_off_requested {
                            // turn it back on again
                            screen_off_requested = false;
                            gfx.set_power(true).ok();
                            gfx.flip_screen(orientation == Orientation::FaceDown).ok();
                            kbd.flip_orientation(orientation == Orientation::FaceDown);
                        }

                        // delegating this to here prevents the screen from glitching on
                        // during wakeup due to RTC event
                        if wfi_awaiting_keypress {
                            // turn on screen
                            gfx.set_power(true).unwrap();
                            wfi_awaiting_keypress = false;
                        }

                        // update the vbus state in case there was noise or chatter
                        vbus_state = iox.get_gpio_pin_value(vbus_io.0, vbus_io.1);

                        // only enable deep sleep if we're on battery power
                        if vbus_state == IoxValue::Low {
                            // this pushes the alarm date out by the deep sleep time horizon
                            set_wakeup_alarm(&rtc);
                            alarm_set = true;
                        }
                    }
                    if k == '🔽' || k == '🔼' {
                        log::debug!("tilt keep-on");
                    }
                }
                last_action_time_ms = tt.elapsed_ms();
            }
            PowerManagerOp::SetFadeMode => {
                if let Some(scalar) = msg_opt.as_ref().unwrap().body.scalar_message() {
                    // only do the fading effect if we're on battery (it's a power saving feature)
                    if scalar.arg1 != 0 {
                        run_led_fade.store(true, Ordering::SeqCst);
                    } else {
                        run_led_fade.store(false, Ordering::SeqCst);
                    }
                }
            }
            PowerManagerOp::GetAccelId => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    if let Some(a) = accel.as_ref() {
                        scalar.arg1 = 1;
                        let id = a.read_who_am_i(&mut i2c).unwrap_or(0);
                        scalar.arg2 = id as usize;
                    } else {
                        scalar.arg1 = 0;
                    }
                }
            }
            PowerManagerOp::GetVbat => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    let vbat_raw = adc.read_raw(
                        bao1x_hal::udma::AdcSource::Ext(bao1x_hal::udma::AdcExtChannel::Adc3),
                        Some(8),
                    );
                    let vbat_mv = (bao1x_hal::udma::Adc::raw_to_voltage(vbat_raw) * 1000.0f32) as usize;
                    scalar.arg1 = 1;
                    scalar.arg2 = vbat_mv;
                }
            }
            PowerManagerOp::GetVbus => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    scalar.arg1 = 1;
                    scalar.arg2 =
                        if iox.get_gpio_pin_value(bao1x_api::IoxPort::PA, 4) == bao1x_api::IoxValue::High {
                            1
                        } else {
                            0
                        };
                }
            }
            // system has powered on, enable interrupts & management
            PowerManagerOp::Boot => {
                if let Some(_scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    pwr_mgr_enabled = true;
                    booted = true;

                    set_wakeup_alarm(&rtc);
                    alarm_set = true;

                    // check ground truth now that we're settled
                    vbus_state = iox.get_gpio_pin_value(vbus_io.0, vbus_io.1);

                    // set initial orientation
                    if let Some(a) = accel.as_mut() {
                        if let Ok(o) = a.get_orientation(&mut i2c) {
                            log::info!("Initial orientation: {:?}", o);
                            orientation = o;
                            gfx.flip_screen(o == Orientation::FaceDown).ok();
                            kbd.flip_orientation(o == Orientation::FaceDown);
                            if orientation == Orientation::FaceDown {
                                kbd.inject_key('🔽');
                            } else {
                                kbd.inject_key('🔼');
                            }
                        }

                        // enable the interrupts on boot
                        accel_enable_int(a, &mut i2c, false).unwrap();
                    }
                }
            }
            PowerManagerOp::ForceDeepSleep => {
                force_deep_sleep = true;
            }
            PowerManagerOp::ForceWfi => {
                force_wfi = true;
            }
            PowerManagerOp::ScreenOffRequest => {
                screen_off_requested = true;
                gfx.set_power(false).ok();
            }
            PowerManagerOp::Invalid => {
                log::error!("Invalid power manager operation: {:?}", opcode);
            }
            PowerManagerOp::FeedWdt => {
                wdt.feed();
            }
            PowerManagerOp::PowerOff => {
                power_off = true;
                std::thread::spawn(move || {
                    let rtc = bao1x_hal_service::Rtc::new();
                    // sleep 5 seconds, then power off
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    // clears the RTC alarm
                    rtc.clear_wakeup();
                    // emulates the RTC deep-sleep alarm
                    xous::try_send_message(
                        cid,
                        xous::Message::new_scalar(
                            PowerManagerOp::KeyPress.to_usize().unwrap(),
                            '⏰' as usize,
                            0,
                            0,
                            0,
                        ),
                    )
                    .ok();
                });
            }
        }
    }
}

// returns a multiplier suitable for multiplying by the WDT_FEED_INTERVAL_MS
// to arrive at the correct timeout
fn get_wdt_clk_ms(susres_conn: xous::CID) -> usize {
    let pclk_ms = if let Ok(xous::Result::Scalar1(pclk)) = xous::send_message(
        susres_conn,
        xous::Message::new_blocking_scalar(
            susres::api::Opcode::PlatformSpecific.to_usize().unwrap(),
            ClockOp::GetPclk.to_usize().unwrap(),
            0,
            0,
            0,
        ),
    ) {
        pclk / 1000 / 2
    } else {
        panic!("Can't get pclk")
    };
    pclk_ms
}

fn set_wakeup_alarm(rtc: &Rtc) {
    let rovers = rtc.set_wakeup(Utc::now() + chrono::Duration::seconds(DEEP_SLEEP_SEC)).unwrap_or(0);
    if rovers > 1 {
        log::warn!("Rollover case for RTC not handled, wakeup will fail. rovers: {}", rovers);
    }
}
