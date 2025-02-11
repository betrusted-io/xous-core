use core::fmt::Write;
use std::convert::TryInto;

use locales::t;
use qrcode::{Color, QrCode};

use super::*;
use crate::minigfx::*;

pub(crate) const QUIET_MODULES: i16 = 2;

#[derive(Debug)]
pub struct Notification {
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub is_password: bool,
    pub manual_dismiss: bool,
    pub qrcode: Vec<bool>,
    pub qrwidth: usize,
}
impl Notification {
    pub fn new(action_conn: xous::CID, action_opcode: u32) -> Self {
        Notification {
            action_conn,
            action_opcode,
            is_password: false,
            manual_dismiss: true,
            qrcode: Vec::new(),
            qrwidth: 0,
        }
    }

    pub fn set_is_password(&mut self, setting: bool) {
        // this will cause text to be inverted. Untrusted entities can try to set this,
        // but the GAM should defeat this for dialog boxes outside of the trusted boot
        // set because they can't achieve a high enough trust level.
        self.is_password = setting;
    }

    pub fn set_manual_dismiss(&mut self, setting: bool) { self.manual_dismiss = setting; }

    pub fn set_qrcode(&mut self, setting: Option<&str>) {
        match setting {
            Some(setting) => {
                let qrcode = match QrCode::new(setting) {
                    Ok(code) => code,
                    Err(_e) => QrCode::new(t!("notification.qrcode.error", locales::LANG)).unwrap(),
                };
                self.qrwidth = qrcode.width();
                log::info!("qrcode {}x{} : {} bytes ", self.qrwidth, self.qrwidth, setting.len());
                self.qrcode = Vec::new();
                for color in qrcode.to_colors().iter() {
                    match color {
                        Color::Dark => self.qrcode.push(true),
                        Color::Light => self.qrcode.push(false),
                    }
                }
            }
            None => {
                self.qrcode = Vec::new();
                self.qrwidth = 0;
            }
        }
    }

    fn draw_text(&self, at_height: i16, modal: &Modal) {
        todo!();
    }

    fn draw_qrcode(&self, at_height: i16, modal: &Modal) {
        todo!();
    }
}
