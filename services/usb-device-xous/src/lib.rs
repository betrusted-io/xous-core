#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use api::*;
use xous::{CID, send_message, Message};
use num_traits::*;
pub use usb_device::device::UsbDeviceState;
pub use usbd_human_interface_device::device::keyboard::KeyboardLedsReport;
pub use usbd_human_interface_device::page::Keyboard as UsbKeyCode;
use packed_struct::PackedStruct;

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
    pub fn send_str(&self, s: &str) -> Result<usize, xous::Error> {
        let mut sent = 0;
        for ch in s.chars() {
            self.send_keycode(
                self.char_to_hid_code_us101(ch),
                true
            )?;
            sent += 1;
        }
        Ok(sent)
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
    pub fn char_to_hid_code_us101(&self, key: char) -> Vec<UsbKeyCode> {
        let mut code = vec![];
        match key {
            'a' => code.push(UsbKeyCode::A),
            'b' => code.push(UsbKeyCode::B),
            'c' => code.push(UsbKeyCode::C),
            'd' => code.push(UsbKeyCode::D),
            'e' => code.push(UsbKeyCode::E),
            'f' => code.push(UsbKeyCode::F),
            'g' => code.push(UsbKeyCode::G),
            'h' => code.push(UsbKeyCode::H),
            'i' => code.push(UsbKeyCode::I),
            'j' => code.push(UsbKeyCode::J),
            'k' => code.push(UsbKeyCode::K),
            'l' => code.push(UsbKeyCode::L),
            'm' => code.push(UsbKeyCode::M),
            'n' => code.push(UsbKeyCode::N),
            'o' => code.push(UsbKeyCode::O),
            'p' => code.push(UsbKeyCode::P),
            'q' => code.push(UsbKeyCode::Q),
            'r' => code.push(UsbKeyCode::R),
            's' => code.push(UsbKeyCode::S),
            't' => code.push(UsbKeyCode::T),
            'u' => code.push(UsbKeyCode::U),
            'v' => code.push(UsbKeyCode::V),
            'w' => code.push(UsbKeyCode::W),
            'x' => code.push(UsbKeyCode::X),
            'y' => code.push(UsbKeyCode::Y),
            'z' => code.push(UsbKeyCode::Z),

            'A' => {code.push(UsbKeyCode::A); code.push(UsbKeyCode::LeftShift)},
            'B' => {code.push(UsbKeyCode::B); code.push(UsbKeyCode::LeftShift)},
            'C' => {code.push(UsbKeyCode::C); code.push(UsbKeyCode::LeftShift)},
            'D' => {code.push(UsbKeyCode::D); code.push(UsbKeyCode::LeftShift)},
            'E' => {code.push(UsbKeyCode::E); code.push(UsbKeyCode::LeftShift)},
            'F' => {code.push(UsbKeyCode::F); code.push(UsbKeyCode::LeftShift)},
            'G' => {code.push(UsbKeyCode::G); code.push(UsbKeyCode::LeftShift)},
            'H' => {code.push(UsbKeyCode::H); code.push(UsbKeyCode::LeftShift)},
            'I' => {code.push(UsbKeyCode::I); code.push(UsbKeyCode::LeftShift)},
            'J' => {code.push(UsbKeyCode::J); code.push(UsbKeyCode::LeftShift)},
            'K' => {code.push(UsbKeyCode::K); code.push(UsbKeyCode::LeftShift)},
            'L' => {code.push(UsbKeyCode::L); code.push(UsbKeyCode::LeftShift)},
            'M' => {code.push(UsbKeyCode::M); code.push(UsbKeyCode::LeftShift)},
            'N' => {code.push(UsbKeyCode::N); code.push(UsbKeyCode::LeftShift)},
            'O' => {code.push(UsbKeyCode::O); code.push(UsbKeyCode::LeftShift)},
            'P' => {code.push(UsbKeyCode::P); code.push(UsbKeyCode::LeftShift)},
            'Q' => {code.push(UsbKeyCode::Q); code.push(UsbKeyCode::LeftShift)},
            'R' => {code.push(UsbKeyCode::R); code.push(UsbKeyCode::LeftShift)},
            'S' => {code.push(UsbKeyCode::S); code.push(UsbKeyCode::LeftShift)},
            'T' => {code.push(UsbKeyCode::T); code.push(UsbKeyCode::LeftShift)},
            'U' => {code.push(UsbKeyCode::U); code.push(UsbKeyCode::LeftShift)},
            'V' => {code.push(UsbKeyCode::V); code.push(UsbKeyCode::LeftShift)},
            'W' => {code.push(UsbKeyCode::W); code.push(UsbKeyCode::LeftShift)},
            'X' => {code.push(UsbKeyCode::X); code.push(UsbKeyCode::LeftShift)},
            'Y' => {code.push(UsbKeyCode::Y); code.push(UsbKeyCode::LeftShift)},
            'Z' => {code.push(UsbKeyCode::Z); code.push(UsbKeyCode::LeftShift)},

            '0' => code.push(UsbKeyCode::Keyboard0),
            '1' => code.push(UsbKeyCode::Keyboard1),
            '2' => code.push(UsbKeyCode::Keyboard2),
            '3' => code.push(UsbKeyCode::Keyboard3),
            '4' => code.push(UsbKeyCode::Keyboard4),
            '5' => code.push(UsbKeyCode::Keyboard5),
            '6' => code.push(UsbKeyCode::Keyboard6),
            '7' => code.push(UsbKeyCode::Keyboard7),
            '8' => code.push(UsbKeyCode::Keyboard8),
            '9' => code.push(UsbKeyCode::Keyboard9),
            '!' => {code.push(UsbKeyCode::Keyboard1); code.push(UsbKeyCode::LeftShift)},
            '@' => {code.push(UsbKeyCode::Keyboard2); code.push(UsbKeyCode::LeftShift)},
            '#' => {code.push(UsbKeyCode::Keyboard3); code.push(UsbKeyCode::LeftShift)},
            '$' => {code.push(UsbKeyCode::Keyboard4); code.push(UsbKeyCode::LeftShift)},
            '%' => {code.push(UsbKeyCode::Keyboard5); code.push(UsbKeyCode::LeftShift)},
            '^' => {code.push(UsbKeyCode::Keyboard6); code.push(UsbKeyCode::LeftShift)},
            '&' => {code.push(UsbKeyCode::Keyboard7); code.push(UsbKeyCode::LeftShift)},
            '*' => {code.push(UsbKeyCode::Keyboard8); code.push(UsbKeyCode::LeftShift)},
            '(' => {code.push(UsbKeyCode::Keyboard9); code.push(UsbKeyCode::LeftShift)},
            ')' => {code.push(UsbKeyCode::Keyboard0); code.push(UsbKeyCode::LeftShift)},

            '-' => code.push(UsbKeyCode::Minus),
            '_' => {code.push(UsbKeyCode::Minus); code.push(UsbKeyCode::LeftShift)},
            '[' => code.push(UsbKeyCode::LeftBrace),
            '{' => {code.push(UsbKeyCode::LeftBrace); code.push(UsbKeyCode::LeftShift)},
            ']' => code.push(UsbKeyCode::RightBrace),
            '}' => {code.push(UsbKeyCode::RightBrace); code.push(UsbKeyCode::LeftShift)},
            '/' => code.push(UsbKeyCode::ForwardSlash),
            '?' => {code.push(UsbKeyCode::ForwardSlash); code.push(UsbKeyCode::LeftShift)},
            '\\' => code.push(UsbKeyCode::Backslash),
            '|' => {code.push(UsbKeyCode::Backslash); code.push(UsbKeyCode::LeftShift)},
            '=' => code.push(UsbKeyCode::Equal),
            '+' => {code.push(UsbKeyCode::Equal); code.push(UsbKeyCode::LeftShift)},
            '\'' => code.push(UsbKeyCode::Apostrophe),
            '"' => {code.push(UsbKeyCode::Apostrophe); code.push(UsbKeyCode::LeftShift)},
            ';' => {code.push(UsbKeyCode::Semicolon)},
            ':' => {code.push(UsbKeyCode::Semicolon); code.push(UsbKeyCode::LeftShift)},
            '`' => {code.push(UsbKeyCode::Grave)},
            '~' => {code.push(UsbKeyCode::Grave); code.push(UsbKeyCode::LeftShift)},

            '←' => code.push(UsbKeyCode::LeftArrow),
            '→' => code.push(UsbKeyCode::RightArrow),
            '↑' => code.push(UsbKeyCode::UpArrow),
            '↓' => code.push(UsbKeyCode::DownArrow),

            ',' => code.push(UsbKeyCode::Comma),
            '<' => {code.push(UsbKeyCode::Comma); code.push(UsbKeyCode::LeftShift)},
            '.' => code.push(UsbKeyCode::Dot),
            '>' => {code.push(UsbKeyCode::Dot); code.push(UsbKeyCode::LeftShift)},

            '\u{000d}' => code.push(UsbKeyCode::ReturnEnter),
            ' ' => code.push(UsbKeyCode::Space),
            '\u{0008}' => code.push(UsbKeyCode::DeleteBackspace),
            _ => log::warn!("Ignoring unhandled character: {}", key),
        };
        code
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