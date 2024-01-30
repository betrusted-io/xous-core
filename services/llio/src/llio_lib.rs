use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

use num_traits::{FromPrimitive, ToPrimitive};
use xous::{msg_scalar_unpack, send_message, Message, CID};
use xous_ipc::Buffer;
use xous_semver::SemVer;

use crate::api::*;
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
#[derive(Debug)]
pub struct Llio {
    conn: CID,
    com_sid: Option<xous::SID>,
    usb_sid: Option<xous::SID>,
    gpio_sid: Option<xous::SID>,
}
impl Llio {
    pub fn new(xns: &xous_names::XousNames) -> Self {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(SERVER_NAME_LLIO).expect("Can't connect to LLIO");
        Llio { conn, com_sid: None, usb_sid: None, gpio_sid: None }
    }

    pub fn vibe(&self, pattern: VibePattern) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(Opcode::Vibe.to_usize().unwrap(), pattern.into(), 0, 0, 0),
        )
        .map(|_| ())
    }

    pub fn allow_power_off(&self, allow: bool) -> Result<(), xous::Error> {
        let arg = if allow { 0 } else { 1 };
        send_message(self.conn, Message::new_scalar(Opcode::PowerSelf.to_usize().unwrap(), arg, 0, 0, 0))
            .map(|_| ())
    }

    pub fn allow_ec_snoop(&self, allow: bool) -> Result<(), xous::Error> {
        let arg = if allow { 1 } else { 0 };
        send_message(self.conn, Message::new_scalar(Opcode::EcSnoopAllow.to_usize().unwrap(), arg, 0, 0, 0))
            .map(|_| ())
    }

    pub fn is_plugged_in(&self) -> bool {
        // 0.005033 * 100_000 against 1.50V * 100_000
        ((self.adc_vbus().unwrap() as u32) * 503) > 150_000
    }

    pub fn adc_vbus(&self) -> Result<u16, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::AdcVbus.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar1(val) = response {
            Ok(val as u16)
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }

    pub fn adc_vccint(&self) -> Result<u16, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::AdcVccInt.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar1(val) = response {
            Ok(val as u16)
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }

    pub fn adc_vccaux(&self) -> Result<u16, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::AdcVccAux.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar1(val) = response {
            Ok(val as u16)
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }

    pub fn adc_vccbram(&self) -> Result<u16, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::AdcVccBram.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar1(val) = response {
            Ok(val as u16)
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }

    pub fn adc_usb_n(&self) -> Result<u16, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::AdcUsbN.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar1(val) = response {
            Ok(val as u16)
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }

    pub fn adc_usb_p(&self) -> Result<u16, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::AdcUsbP.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar1(val) = response {
            Ok(val as u16)
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }

    pub fn adc_temperature(&self) -> Result<u16, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::AdcTemperature.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar1(val) = response {
            Ok(val as u16)
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }

    pub fn adc_gpio5(&self) -> Result<u16, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::AdcGpio5.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar1(val) = response {
            Ok(val as u16)
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }

    pub fn adc_gpio2(&self) -> Result<u16, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::AdcGpio2.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar1(val) = response {
            Ok(val as u16)
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }

    // USB hooks
    pub fn hook_usb_callback(&mut self, id: u32, cid: CID) -> Result<(), xous::Error> {
        if self.usb_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.usb_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(
                usb_cb_server,
                sid_tuple.0 as usize,
                sid_tuple.1 as usize,
                sid_tuple.2 as usize,
                sid_tuple.3 as usize,
            )
            .unwrap();
            let hookdata = ScalarHook { sid: sid_tuple, id, cid };
            let buf = Buffer::into_buf(hookdata).or(Err(xous::Error::InternalError))?;
            buf.lend(self.conn, Opcode::EventUsbAttachSubscribe.to_u32().unwrap()).map(|_| ())
        } else {
            Err(xous::Error::MemoryInUse) // can't hook it twice
        }
    }

    pub fn usb_event_enable(&self, ena: bool) -> Result<(), xous::Error> {
        let arg = if ena { 1 } else { 0 };
        send_message(
            self.conn,
            Message::new_scalar(Opcode::EventUsbAttachEnable.to_usize().unwrap(), arg, 0, 0, 0),
        )
        .map(|_| ())
    }

    // COM IRQ hooks
    pub fn hook_com_event_callback(&mut self, id: u32, cid: CID) -> Result<(), xous::Error> {
        if self.com_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.com_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(
                com_cb_server,
                sid_tuple.0 as usize,
                sid_tuple.1 as usize,
                sid_tuple.2 as usize,
                sid_tuple.3 as usize,
            )
            .unwrap();
            let hookdata = ScalarHook { sid: sid_tuple, id, cid };
            let buf = Buffer::into_buf(hookdata).or(Err(xous::Error::InternalError))?;
            buf.lend(self.conn, Opcode::EventComSubscribe.to_u32().unwrap()).map(|_| ())
        } else {
            Err(xous::Error::MemoryInUse) // can't hook it twice
        }
    }

    pub fn com_event_enable(&self, ena: bool) -> Result<(), xous::Error> {
        let arg = if ena { 1 } else { 0 };
        send_message(self.conn, Message::new_scalar(Opcode::EventComEnable.to_usize().unwrap(), arg, 0, 0, 0))
            .map(|_| ())
    }

    /// Called by the updater routine when EC is ready to run. Note: no authenticity check is done, anyone
    /// could call this and troll the system.
    pub fn set_ec_ready(&self, ready: bool) {
        send_message(
            self.conn,
            Message::new_scalar(
                Opcode::EventEcSetReady.to_usize().unwrap(),
                if ready { 1 } else { 0 },
                0,
                0,
                0,
            ),
        )
        .unwrap();
    }

    /// Used to poll if the EC has been set as ready by the updating routine. Domiciled in the LLIO because
    /// this crate is already depended upon by most crates that need to poll this status, and we'd rather not
    /// expose the COM directly to generic crates.
    pub fn is_ec_ready(&self) -> bool {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::EventEcIsReady.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .unwrap();
        if let xous::Result::Scalar1(val) = response {
            val != 0
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            false
        }
    }

    /// GPIO IRQ hook. When using this, ensure that the WFI power saving mode is turned off.
    /// Otherwise interrupts that hit during power save mode can be missed.
    pub fn hook_gpio_event_callback(&mut self, id: u32, cid: CID) -> Result<(), xous::Error> {
        log::info!(
            "If relying on GPIO interrupts, WFI power saving must be turned off. Interrupts that hit while powersaving can be missed."
        );
        if self.gpio_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.gpio_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(
                gpio_cb_server,
                sid_tuple.0 as usize,
                sid_tuple.1 as usize,
                sid_tuple.2 as usize,
                sid_tuple.3 as usize,
            )
            .unwrap();
            let hookdata = ScalarHook { sid: sid_tuple, id, cid };
            let buf = Buffer::into_buf(hookdata).or(Err(xous::Error::InternalError))?;
            buf.lend(self.conn, Opcode::GpioIntSubscribe.to_u32().unwrap()).map(|_| ())
        } else {
            Err(xous::Error::MemoryInUse) // can't hook it twice
        }
    }

    pub fn ec_reset(&self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::EcReset.to_usize().unwrap(), 0, 0, 0, 0))
            .map(|_| ())
    }

    pub fn self_destruct(&self, code: usize) -> Result<(), xous::Error> {
        // it's up to the caller to know the code sequence, which is:
        // 0x2718_2818
        // followed by
        // 0x3141_5926
        send_message(self.conn, Message::new_scalar(Opcode::SelfDestruct.to_usize().unwrap(), code, 0, 0, 0))
            .map(|_| ())
    }

    pub fn boost_on(&self, ena: bool) -> Result<(), xous::Error> {
        let arg = if ena { 1 } else { 0 };
        send_message(self.conn, Message::new_scalar(Opcode::PowerBoostMode.to_usize().unwrap(), arg, 0, 0, 0))
            .map(|_| ())
    }

    pub fn audio_on(&self, ena: bool) -> Result<(), xous::Error> {
        let arg = if ena { 1 } else { 0 };
        send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::PowerAudio.to_usize().unwrap(), arg, 0, 0, 0),
        )
        .map(|_| ())
    }

    // -149mA @ 4152mV crypto on // -143mA @ 4149mV crypto off
    pub fn crypto_on(&self, ena: bool) -> Result<(), xous::Error> {
        let arg = if ena { 1 } else { 0 };
        send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::PowerCrypto.to_usize().unwrap(), arg, 0, 0, 0),
        )
        .map(|_| ())
    }

    /// setting this to true turns off WFI capabilities, forcing power always on
    pub fn wfi_override(&self, ena: bool) -> Result<(), xous::Error> {
        let arg = if ena { 1 } else { 0 };
        send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::WfiOverride.to_usize().unwrap(), arg, 0, 0, 0),
        )
        .map(|_| ())
    }

    pub fn crypto_power_status(&self) -> Result<(bool, bool, bool), xous::Error> {
        // sha, engine, override status
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::PowerCryptoStatus.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar1(val) = response {
            let sha = if (val & 1) == 0 { false } else { true };
            let engine = if (val & 2) == 0 { false } else { true };
            let force = if (val & 4) == 0 { false } else { true };
            Ok((sha, engine, force))
        } else {
            Err(xous::Error::InternalError)
        }
    }

    pub fn soc_gitrev(&self) -> Result<SemVer, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::InfoGit.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar2(val1, val2) = response {
            Ok(SemVer {
                maj: ((val1 >> 24) & 0xff) as u16,
                min: ((val1 >> 16) & 0xff) as u16,
                rev: ((val1 >> 8) & 0xff) as u16,
                extra: ((val1 >> 0) & 0xff) as u16,
                commit: Some(val2 as u32),
            })
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }

    pub fn soc_dna(&self) -> Result<u64, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::InfoDna.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar2(val1, val2) = response {
            Ok((val1 as u64) | ((val2 as u64) << 32))
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }

    pub fn gpio_data_direction(&self, dir: u8) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(Opcode::GpioDataDrive.to_usize().unwrap(), dir as usize, 0, 0, 0),
        )
        .map(|_| ())
    }

    pub fn gpio_debug_powerdown(&self, ena: bool) -> Result<(), xous::Error> {
        let arg = if ena { 1 } else { 0 };
        send_message(self.conn, Message::new_scalar(Opcode::DebugPowerdown.to_usize().unwrap(), arg, 0, 0, 0))
            .map(|_| ())
    }

    pub fn gpio_debug_wakeup(&self, ena: bool) -> Result<(), xous::Error> {
        let arg = if ena { 1 } else { 0 };
        send_message(self.conn, Message::new_scalar(Opcode::DebugWakeup.to_usize().unwrap(), arg, 0, 0, 0))
            .map(|_| ())
    }

    pub fn activity_instantaneous(&self) -> Result<(u32, u32), xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::GetActivity.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar2(active, total) = response {
            Ok((active as u32, total as u32))
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }

    pub fn set_uart_mux(&self, setting: UartType) -> Result<(), xous::Error> {
        if setting == UartType::Application {
            log::warn!(
                "Application UART has aggressive power settings, so you will have trouble using it for console input."
            );
            log::warn!(
                "If this UART is critictal, recompile the SoC with the app UART in the always-on power domain."
            );
            log::warn!(
                "It will consume more power but it will make this UART suitable for input via serial."
            );
        }
        let arg = setting.into();
        send_message(self.conn, Message::new_scalar(Opcode::UartMux.to_usize().unwrap(), arg, 0, 0, 0))
            .map(|_| ())
    }

    /// wakeup alarm will force the system on if it is off, but does not trigger an interrupt on the CPU
    pub fn set_wakeup_alarm(&self, seconds_from_now: u8) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_blocking_scalar(
                Opcode::SetWakeupAlarm.to_usize().unwrap(),
                seconds_from_now as _,
                0,
                0,
                0,
            ),
        )
        .map(|_| ())
    }

    pub fn clear_wakeup_alarm(&self) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::ClearWakeupAlarm.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .map(|_| ())
    }

    /// This returns the elapsed seconds on the RTC since an arbitrary start point in the past.
    /// The translation of this is handled by `libstd::SystemTime`; you may use this call, but
    /// the interpretation is not terribly meaningful on its own.
    pub fn get_rtc_secs(&self) -> Result<u64, xous::Error> {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::GetRtcValue.to_usize().unwrap(), 0, 0, 0, 0),
        )? {
            xous::Result::Scalar2(hi, lo) => {
                if hi & 0x8000_0000 != 0 {
                    Err(xous::Error::InternalError)
                } else {
                    Ok(((hi as u64) << 32) | lo as u64)
                }
            }
            _ => Err(xous::Error::InternalError),
        }
    }
}

fn drop_conn(sid: xous::SID) {
    let cid = xous::connect(sid).unwrap();
    xous::send_message(cid, Message::new_scalar(EventCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0))
        .unwrap();
    unsafe {
        xous::disconnect(cid).unwrap();
    }
}
impl Drop for Llio {
    fn drop(&mut self) {
        if let Some(sid) = self.usb_sid.take() {
            drop_conn(sid);
        }
        if let Some(sid) = self.com_sid.take() {
            drop_conn(sid);
        }
        if let Some(sid) = self.gpio_sid.take() {
            drop_conn(sid);
        }
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}

/// handles callback messages that indicate a USB interrupt has happened, in the library user's process space.
fn usb_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(EventCallback::Event) => msg_scalar_unpack!(msg, cid, id, _, _, {
                // directly pass the scalar message onto the CID with the ID memorized in the original hook
                send_message(cid as u32, Message::new_scalar(id, 0, 0, 0, 0)).unwrap();
            }),
            Some(EventCallback::Drop) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}

/// handles callback messages that indicate a COM interrupt has happened, in the library user's process space.
fn com_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(EventCallback::Event) => msg_scalar_unpack!(msg, cid, id, _, _, {
                // directly pass the scalar message onto the CID with the ID memorized in the original hook
                send_message(cid as u32, Message::new_scalar(id, 0, 0, 0, 0)).unwrap();
            }),
            Some(EventCallback::Drop) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}

/// handles callback messages that indicate a GPIO interrupt has happened, in the library user's process
/// space.
fn gpio_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(EventCallback::Event) => msg_scalar_unpack!(msg, cid, id, _, _, {
                // directly pass the scalar message onto the CID with the ID memorized in the original hook
                send_message(cid as u32, Message::new_scalar(id, 0, 0, 0, 0)).unwrap();
            }),
            Some(EventCallback::Drop) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}
