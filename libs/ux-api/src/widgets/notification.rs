use core::fmt::Write;

use locales::t;
use qrcode::{Color, QrCode};

use super::*;
use crate::minigfx::op::HEIGHT;
use crate::minigfx::*;
use crate::service::api::*;
use crate::service::gfx::Gfx;

pub(crate) const QUIET_MODULES: isize = 2;

#[derive(Debug)]
pub struct Notification {
    pub gfx: Gfx,
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub is_password: bool,
    pub manual_dismiss: bool,
    pub qrcode: Vec<bool>,
    pub qrwidth: usize,
}
impl Notification {
    pub fn new(action_conn: xous::CID, action_opcode: u32) -> Self {
        let xns = xous_names::XousNames::new().unwrap();
        Notification {
            gfx: Gfx::new(&xns).unwrap(),
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
                self.qrcode = qrcode.into_colors().into_iter().map(|c| c != Color::Light).collect();
                log::info!(
                    "qrcode {}x{} : {} bytes, {} modules",
                    self.qrwidth,
                    self.qrwidth,
                    setting.len(),
                    self.qrcode.len()
                );
            }
            None => {
                self.qrcode = Vec::new();
                self.qrwidth = 0;
            }
        }
    }

    fn draw_text(&self, at_height: isize, modal: &Modal) {
        // prime a textview with the correct general style parameters
        let mut tv = TextView::new(Gid::dummy(), TextBounds::BoundingBox(Rectangle::new_coords(0, 0, 1, 1)));
        tv.ellipsis = true;
        tv.style = modal.style;
        tv.invert = true;
        tv.draw_border = false;
        tv.margin = Point::new(0, 0);
        tv.insertion = None;

        tv.bounds_computed = None;
        tv.bounds_hint = TextBounds::GrowableFromTl(
            Point::new(modal.margin, at_height + modal.margin * 2),
            (modal.canvas_width - modal.margin * 2) as u16,
        );
        write!(tv, "{}", t!("notification.dismiss", locales::LANG)).unwrap();
        self.gfx.bounds_compute_textview(&mut tv).expect("couldn't simulate text size");
        let textwidth = if let Some(bounds) = tv.bounds_computed {
            bounds.br.x - bounds.tl.x
        } else {
            modal.canvas_width - modal.margin * 2
        };
        let offset = (modal.canvas_width - textwidth) / 2;
        tv.bounds_computed = None;
        tv.bounds_hint = TextBounds::BoundingBox(Rectangle::new(
            Point::new(offset, at_height + modal.margin * 2),
            Point::new(modal.canvas_width - modal.margin, at_height + modal.line_height + modal.margin * 2),
        ));
        self.gfx.draw_textview(&mut tv).expect("couldn't post tv");
    }

    fn draw_qrcode(&self, at_height: isize, _modal: &Modal) {
        self.gfx
            .render_qr(&self.qrcode, self.qrwidth, Point { x: 0, y: at_height })
            .expect("couldn't render QR");
    }
}

use crate::widgets::ActionApi;
impl ActionApi for Notification {
    fn redraw(&self, at_height: isize, modal: &Modal) {
        if self.qrwidth > 0 {
            self.draw_qrcode(at_height, modal);
        } else {
            if self.manual_dismiss {
                self.draw_text(at_height, modal);
                modal
                    .gfx
                    .draw_line(Line::new_with_style(
                        Point::new(modal.margin, at_height + modal.margin),
                        Point::new(modal.canvas_width - modal.margin, at_height + modal.margin),
                        DrawStyle::new(PixelColor::Light, PixelColor::Light, 1),
                    ))
                    .expect("couldn't draw entry line");
            }
        }
    }

    fn key_action(&mut self, k: char) -> Option<ValidatorErr> {
        log::trace!("key_action: {}", k);
        match k {
            '\u{0}' => {
                // ignore null messages
            }
            _ => {
                // relinquish focus before returning the result
                if self.manual_dismiss {
                    self.gfx.release_modal().unwrap();
                    xous::yield_slice();
                }

                xous::send_message(
                    self.action_conn,
                    xous::Message::new_scalar(self.action_opcode as usize, k as u32 as usize, 0, 0, 0),
                )
                .expect("couldn't pass on dismissal");
                if self.manual_dismiss {
                    return None;
                }
            }
        }
        None
    }

    fn height(&self, _glyph_height: isize, margin: isize, _modal: &Modal) -> isize {
        if self.qrwidth > 0 { HEIGHT } else { margin }
    }

    fn set_action_opcode(&mut self, op: u32) { self.action_opcode = op }
}
