#![cfg_attr(target_os = "none", no_std)]

pub mod api;
pub use api::*;
use trng::api::TrngTestMode;
use xous::{CID, send_message, Message};
use num_traits::*;
pub use usb_device::device::UsbDeviceState;
pub use xous_usb_hid::device::keyboard::KeyboardLedsReport;
pub use xous_usb_hid::page::Keyboard as UsbKeyCode;
use packed_struct::PackedStruct;
use xous_ipc::Buffer;
pub use xous_usb_hid::device::fido::RawFidoReport;

#[derive(Debug)]
pub struct UsbHid {
    conn: CID,
}
impl UsbHid {
    pub fn new() -> Self {
        let xns = xous_names::XousNames::new().expect("couldn't connect to XousNames");
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_USB_DEVICE).expect("Can't connect to USB device server");
        UsbHid {
            conn
        }
    }
    #[cfg(feature="mass-storage")]
    pub fn set_block_device(&self, read_id: usize, write_id: usize, max_lba_id: usize) {
        send_message(self.conn,
            Message::new_blocking_scalar(
                Opcode::SetBlockDevice.to_usize().unwrap(),
                read_id,
                write_id,
                max_lba_id,
                0,
            )
        ).unwrap();
    }
    #[cfg(feature="mass-storage")]
    pub fn set_block_device_sid(&self, app_sid: xous::SID) {
        let sid = app_sid.to_u32();
        send_message(self.conn,
            Message::new_blocking_scalar(
                Opcode::SetBlockDeviceSID.to_usize().unwrap(),
                sid.0 as usize,
                sid.1 as usize,
                sid.2 as usize,
                sid.3 as usize,
            )
        ).unwrap();
    }
    #[cfg(feature="mass-storage")]
    pub fn reset_block_device(&self) {
        send_message(self.conn,
            Message::new_blocking_scalar(
                Opcode::ResetBlockDevice.to_usize().unwrap(),
                0,
                0,
                0,
                0,
            )
        ).unwrap();
    }

    /// used to query if the HID core was able to start. Mainly to handle edge cases between updates.
    pub fn is_soc_compatible(&self) -> bool {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(
                Opcode::IsSocCompatible.to_usize().unwrap(),
                0, 0, 0, 0
            )
        ) {
            Ok(xous::Result::Scalar1(code)) => {
                match code {
                    0 => false,
                    _ => true
                }
            }
            _ => panic!("Internal error: illegal return type"),
        }
    }
    /// this will always trigger a reset, even if it's the same core we're switching to
    pub fn switch_to_core(&self, core: UsbDeviceType) -> Result<(), xous::Error> {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(
                Opcode::SwitchCores.to_usize().unwrap(),
                core as usize,
                0, 0, 0
            )
        ) {
            Ok(xous::Result::Scalar1(code)) => {
                match code {
                    0 => Ok(()),
                    _ => Err(xous::Error::InternalError)
                }
            }
            _ => panic!("Internal error: illegal return type"),
        }
    }
    /// this will not trigger a reset if we're already on the requested core
    pub fn ensure_core(&self, core: UsbDeviceType) -> Result<(), xous::Error> {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(
                Opcode::EnsureCore.to_usize().unwrap(),
                core as usize,
                0, 0, 0
            )
        ) {
            Ok(xous::Result::Scalar1(code)) => {
                match code {
                    0 => Ok(()),
                    _ => Err(xous::Error::InternalError)
                }
            }
            _ => panic!("Internal error: illegal return type"),
        }
    }
    pub fn get_current_core(&self) -> Result<UsbDeviceType, xous::Error> {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(
                Opcode::WhichCore.to_usize().unwrap(),
                0, 0, 0, 0
            )
        ) {
            Ok(xous::Result::Scalar1(code)) => {
                match code {
                    0 => Ok(UsbDeviceType::Debug),
                    1 => Ok(UsbDeviceType::FidoKbd),
                    2 => Ok(UsbDeviceType::Fido),
                    #[cfg(feature="mass-storage")]
                    3 => Ok(UsbDeviceType::MassStorage),
                    4 => Ok(UsbDeviceType::Serial),
                    _ => Err(xous::Error::InternalError)
                }
            }
            _ => panic!("Internal error: illegal return type"),
        }
    }
    pub fn restrict_debug_access(&self, restrict: bool) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(
                Opcode::RestrictDebugAccess.to_usize().unwrap(),
                if restrict {1} else {0},
                0, 0, 0
            )
        ).map(|_| ())
    }
    pub fn is_debug_restricted(&self) -> Result<bool, xous::Error> {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(
                Opcode::IsRestricted.to_usize().unwrap(),
                0, 0, 0, 0
            )
        ) {
            Ok(xous::Result::Scalar1(code)) => {
                if code == 1 {
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            _ => Err(xous::Error::InternalError),
        }
    }
    // if do_lock is Some(), set the debug USB lock status to locked if true, unlocked if false
    // returns a tuple of (bool, bool) -> (is_locked, force_update)
    // needs_update is so that the polling function knows to redraw the UX after a resume-from-suspend
    pub fn debug_usb(&self, do_lock: Option<bool>) -> Result<(bool, bool), xous::Error> {
        // arg1 indicates if an update to the state is requested
        // arg2 is the new state update
        let (arg1, arg2) = if let Some(lock) = do_lock {
            if lock {
                (1, 1)
            } else {
                (1, 0)
            }
         } else {
            (0, 0)
        };
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::DebugUsbOp.to_usize().unwrap(), arg1, arg2, 0, 0))?;
        if let xous::Result::Scalar2(is_locked, force_update) = response {
            let il = if is_locked != 0 {true} else {false};
            let fu = if force_update != 0 {true} else {false};
            Ok(
                (il, fu)
            )
        } else {
            Err(xous::Error::InternalError)
        }
    }
    pub fn status(&self) -> UsbDeviceState {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(
                Opcode::LinkStatus.to_usize().unwrap(),
                0, 0, 0, 0
            )
        ) {
            Ok(xous::Result::Scalar1(code)) => {
                match code {
                    0 => UsbDeviceState::Default,
                    1 => UsbDeviceState::Addressed,
                    2 => UsbDeviceState::Configured,
                    3 => UsbDeviceState::Suspend,
                    _ => panic!("Internal error: illegal status code")
                }
            }
            _ => panic!("Internal error: illegal return type"),
        }
    }
    /// Sends up to three keyboard codes at once as defined by USB HID usage tables;
    /// see See [Universal Serial Bus (USB) HID Usage Tables Version 1.12](<https://www.usb.org/sites/default/files/documents/hut1_12v2.pdf>):
    /// If the vector is empty, you get an all-key-up situation
    pub fn send_keycode(&self, code: Vec<UsbKeyCode>, auto_keyup: bool) -> Result<(), xous::Error> {
        if code.len() > 3 {
            log::warn!("Excess keycodes ignored");
        }
        match send_message(
            self.conn,
            Message::new_blocking_scalar(
                Opcode::SendKeyCode.to_usize().unwrap(),
                if code.len() >= 1 {code[0] as usize} else {0},
                if code.len() >= 2 {code[1] as usize} else {0},
                if code.len() >= 3 {code[2] as usize} else {0},
                if auto_keyup { 1 } else { 0 }
            )
        ) {
            Ok(xous::Result::Scalar1(code)) => {
                match code {
                    0 => Ok(()),
                    // indicates that we aren't connected to a host to send characters
                    _ => Err(xous::Error::UseBeforeInit),
                }
            }
            _ => Err(xous::Error::UseBeforeInit),
        }
    }
    /// This will attempt to send a string using an API based on the currently connected device
    /// If it's a Keyboard, it will "type" it; if it's a UART, it will just blast it out the Tx.
    pub fn send_str(&self, s: &str) -> Result<usize, xous::Error> {
        let serializer = UsbString {
            s: xous_ipc::String::<4000>::from_str(s),
            sent: None
        };
        let mut buf = Buffer::into_buf(serializer).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::SendString.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        let returned = buf.to_original::<UsbString, _>().or(Err(xous::Error::InternalError))?;
        match returned.sent {
            Some(sent) => Ok(sent as usize),
            // indicate that probably the USB was not connected
            None => Err(xous::Error::UseBeforeInit),
        }
    }
    /// Sets the autotype delay. Defaults to 30ms on boot, must be reset every time on reboot.
    pub fn set_autotype_delay_ms(&self, rate: usize) {
        send_message(
            self.conn,
            Message::new_scalar(Opcode::SetAutotypeRate.to_usize().unwrap(), rate, 0, 0, 0)
        ).unwrap(); // just unwrap it. If the send fails, we want to see the panic at this spot!
    }
    pub fn get_led_state(&self) -> Result<KeyboardLedsReport, xous::Error> {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(
                Opcode::GetLedState.to_usize().unwrap(),
                0, 0, 0, 0
            )
        ) {
            Ok(xous::Result::Scalar1(code)) => {
                match KeyboardLedsReport::unpack(&[code as u8]) {
                    Ok(r) => Ok(r),
                    Err(_) => Err(xous::Error::InternalError),
                }
            }
            _ => panic!("Internal error: illegal return type"),
        }
    }
    pub fn u2f_wait_incoming(&self) -> Result<RawFidoReport, xous::Error> {
        let req = U2fMsgIpc {
            data: [0; 64],
            code: U2fCode::RxWait,
            timeout_ms: None,
        };
        let mut buf = Buffer::into_buf(req).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::U2fRxDeferred.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        let ack = buf.to_original::<U2fMsgIpc, _>().unwrap();
        match ack.code {
            U2fCode::RxAck => {
                let mut u2fmsg = RawFidoReport::default();
                u2fmsg.packet.copy_from_slice(&ack.data);
                Ok(u2fmsg)
            },
            U2fCode::Hangup => {
                Err(xous::Error::ProcessTerminated)
            },
            U2fCode::RxTimeout => {
                Err(xous::Error::Timeout)
            },
            _ => Err(xous::Error::InternalError)
        }
    }
    /// Note: this variant is not tested.
    pub fn u2f_wait_incoming_timeout(&self, timeout_ms: u64) -> Result<RawFidoReport, xous::Error> {
        let req = U2fMsgIpc {
            data: [0; 64],
            code: U2fCode::RxWait,
            timeout_ms: Some(timeout_ms),
        };
        let mut buf = Buffer::into_buf(req).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::U2fRxDeferred.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        let ack = buf.to_original::<U2fMsgIpc, _>().unwrap();
        match ack.code {
            U2fCode::RxAck => {
                let mut u2fmsg = RawFidoReport::default();
                u2fmsg.packet.copy_from_slice(&ack.data);
                Ok(u2fmsg)
            },
            U2fCode::Hangup => {
                Err(xous::Error::ProcessTerminated)
            },
            U2fCode::RxTimeout => {
                Err(xous::Error::Timeout)
            },
            _ => Err(xous::Error::InternalError)
        }
    }
    pub fn u2f_send(&self, msg: RawFidoReport) -> Result<(), xous::Error> {
        let mut req = U2fMsgIpc {
            data: [0; 64],
            code: U2fCode::Tx,
            timeout_ms: None,
        };
        req.data.copy_from_slice(&msg.packet);
        let mut buf = Buffer::into_buf(req).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::U2fTx.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        let ack = buf.to_original::<U2fMsgIpc, _>().unwrap();
        match ack.code {
            U2fCode::TxAck => Ok(()),
            U2fCode::Denied => Err(xous::Error::AccessDenied),
            _ => Err(xous::Error::InternalError),
        }
    }
    /// Blocks until an ASCII string terminated by `delimiter` is received on serial; if `None`, it
    /// will return as soon as a character (or series of characters) have been received (thus the return
    /// `String` will be piecemeal)
    pub fn serial_wait_ascii(&self, delimiter: Option<char>) -> String {
        let req = UsbSerialAscii {
            s: xous_ipc::String::new(),
            delimiter
        };
        let mut buf = Buffer::into_buf(req).or(Err(xous::Error::InternalError)).expect("Internal error");
        buf.lend_mut(self.conn, Opcode::SerialHookAscii.to_u32().unwrap()).or(Err(xous::Error::InternalError)).expect("Internal error");
        let resp = buf.to_original::<UsbSerialAscii, _>().unwrap();
        resp.s.to_str().to_string()
    }
    /// Blocks until enough binary data has been received to fill the buffer
    /// Another thread can be used to call serial_flush() if we don't want to
    /// block forever and we're receiving small amounts of binary data.
    pub fn serial_wait_binary(&self) -> Vec::<u8> {
        let req = UsbSerialBinary {
            d: [0u8; SERIAL_BINARY_BUFLEN],
            len: 0,
        };
        let mut buf = Buffer::into_buf(req).or(Err(xous::Error::InternalError)).expect("Internal error");
        buf.lend_mut(self.conn, Opcode::SerialHookBinary.to_u32().unwrap()).or(Err(xous::Error::InternalError)).expect("Internal error");
        let resp = buf.to_original::<UsbSerialBinary, _>().unwrap();
        resp.d[..resp.len].to_vec()
    }
    /// Non-blocking call that issues a serial flush command to the USB stack
    pub fn serial_flush(&self) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(
                Opcode::SerialFlush.to_usize().unwrap(),
                0, 0, 0, 0
            )
        ).map(|_| ())
    }
    /// Inject serial input over USB to the debug console. Dangerous!
    /// This will also override/discard any existing hooked listeners.
    pub fn serial_console_input_injection(&self) {
        send_message(
            self.conn,
            Message::new_scalar(
                Opcode::SerialHookConsole.to_usize().unwrap(), 0, 0, 0, 0
            )
        ).unwrap();
    }
    pub fn serial_clear_input_hooks(&self) {
        send_message(
            self.conn,
            Message::new_scalar(
                Opcode::SerialClearHooks.to_usize().unwrap(), 0, 0, 0, 0
            )
        ).unwrap();
    }
    /// Tries to set the serial port in TRNG mode. Will silently fail if already in console mode.
    pub fn serial_set_trng_mode(&self, mode: TrngTestMode) {
        send_message(
            self.conn,
            Message::new_scalar(
                Opcode::SerialHookTrngSender.to_usize().unwrap(), mode.to_usize().unwrap(), 0, 0, 0
            )
        ).unwrap();
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for UsbHid {
    fn drop(&mut self) {
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
    }
}