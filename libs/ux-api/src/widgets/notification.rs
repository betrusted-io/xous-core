use core::fmt::Write;

use locales::t;
use qrcode::{Color, QrCode};

use super::*;
use crate::minigfx::op::{HEIGHT, WIDTH};
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

    fn draw_qrcode(&self, at_height: isize, modal: &Modal) {
        // calculate pixel size of each module in the qrcode
        let qrcode_modules: isize = self.qrwidth.try_into().unwrap();
        let modules: isize = qrcode_modules + 2 * QUIET_MODULES;
        let canvas_width = modal.canvas_width - 2 * modal.margin;
        let mod_size_px: isize = canvas_width / modules;
        let qrcode_width_px = qrcode_modules * mod_size_px;
        let quiet_px: isize = (canvas_width - qrcode_width_px) / 2;

        // Iterate thru qrcode and stamp each square module like a typewriter
        let black = DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1);
        let top = at_height + quiet_px;

        // draw a white background over the whole screen
        self.gfx
            .draw_rectangle(Rectangle::new_with_style(
                Point::new(0, at_height),
                Point::new(WIDTH, HEIGHT),
                DrawStyle::new(PixelColor::Light, PixelColor::Light, 1),
            ))
            .unwrap();

        let left = modal.margin + quiet_px;
        let right = left + qrcode_modules * mod_size_px;
        let mut module =
            Rectangle::new_with_style(Point::new(0, 0), Point::new(mod_size_px - 1, mod_size_px - 1), black);
        let step = Point::new(mod_size_px, 0);
        let cr_lf = Point::new(-qrcode_modules * mod_size_px, mod_size_px);
        let mut j: isize;
        let mut obj_list = ObjectList::new();
        module.translate(Point::new(right, top));
        for (i, stamp) in self.qrcode.iter().enumerate() {
            j = i.try_into().unwrap();
            if j % qrcode_modules == 0 {
                module.translate(cr_lf);
            }
            if *stamp {
                if obj_list.push(ClipObjectType::Rect(module.clone())).is_err() {
                    // the capacity of the list has exceeded the capacity of the buffer, send it and make a
                    // new one
                    self.gfx.draw_object_list(obj_list).expect("couldn't draw qrcode module");
                    obj_list = ObjectList::new();
                    obj_list.push(ClipObjectType::Rect(module.clone())).unwrap();
                };
            }
            module.translate(step);
        }
        self.gfx.draw_object_list(obj_list).expect("couldn't draw qrcode module");
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
