use crate::*;

use graphics_server::api::*;

use locales::t;
use core::fmt::Write;

#[derive(Debug, Copy, Clone)]
pub struct Notification {
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub is_password: bool,
    pub manual_dismiss: bool,
}
impl Notification {
    pub fn new(action_conn: xous::CID, action_opcode: u32) -> Self {
        Notification {
            action_conn,
            action_opcode,
            is_password: false,
            manual_dismiss: true,
        }
    }
    pub fn set_is_password(&mut self, setting: bool) {
        // this will cause text to be inverted. Untrusted entities can try to set this,
        // but the GAM should defeat this for dialog boxes outside of the trusted boot
        // set because they can't achieve a high enough trust level.
        self.is_password = setting;
    }
    pub fn set_manual_dismiss(&mut self, setting: bool) {
        self.manual_dismiss = setting;
    }
}
impl ActionApi for Notification {
    fn set_action_opcode(&mut self, op: u32) {self.action_opcode = op}
    fn height(&self, glyph_height: i16, margin: i16) -> i16 {
        if self.manual_dismiss {
            glyph_height + margin * 2 + 5
        } else {
            margin + 5
        }
    }
    fn redraw(&self, at_height: i16, modal: &Modal) {
        if self.manual_dismiss {
            // prime a textview with the correct general style parameters
            let mut tv = TextView::new(
                modal.canvas,
                TextBounds::BoundingBox(Rectangle::new_coords(0, 0, 1, 1))
            );
            tv.ellipsis = true;
            tv.style = modal.style;
            tv.invert = self.is_password;
            tv.draw_border= false;
            tv.margin = Point::new(0, 0,);
            tv.insertion = None;

            tv.bounds_computed = None;
            tv.bounds_hint = TextBounds::GrowableFromTl(
                Point::new(modal.margin, at_height + modal.margin * 2),
                (modal.canvas_width - modal.margin * 2) as u16
            );
            write!(tv, "{}", t!("notification.dismiss", xous::LANG)).unwrap();
            modal.gam.bounds_compute_textview(&mut tv).expect("couldn't simulate text size");
            let textwidth = if let Some(bounds) = tv.bounds_computed {
                bounds.br.x - bounds.tl.x
            } else {
                modal.canvas_width - modal.margin * 2
            };
            let offset = (modal.canvas_width - textwidth) / 2;
            tv.bounds_computed = None;
            tv.bounds_hint = TextBounds::BoundingBox(Rectangle::new(
                Point::new(offset, at_height + modal.margin * 2),
                Point::new(modal.canvas_width - modal.margin, at_height + modal.line_height + modal.margin * 2)
            ));
            modal.gam.post_textview(&mut tv).expect("couldn't post tv");

            // divider lines
            let color = if self.is_password {
                PixelColor::Light
            } else {
                PixelColor::Dark
            };

            modal.gam.draw_line(modal.canvas, Line::new_with_style(
                Point::new(modal.margin, at_height + modal.margin),
                Point::new(modal.canvas_width - modal.margin, at_height + modal.margin),
                DrawStyle::new(color, color, 1))
            ).expect("couldn't draw entry line");
        }
    }
    fn key_action(&mut self, k: char) -> (Option<xous_ipc::String::<512>>, bool) {
        log::trace!("key_action: {}", k);
        match k {
            '\u{0}' => {
                // ignore null messages
            }
            _ => {
                if self.manual_dismiss {
                    send_message(self.action_conn, xous::Message::new_scalar(self.action_opcode as usize, 0, 0, 0, 0)).expect("couldn't pass on dismissal");
                    return(None, true)
                }
            }
        }
        (None, false)
    }
}