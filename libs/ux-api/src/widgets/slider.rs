use std::fmt::Write;

use super::*;
use crate::minigfx::*;
use crate::service::api::Gid;
use crate::service::gfx::Gfx;

// This structure needs to be "shallow copy capable" so we can use it with
// the enum_actions API to update the progress state in an efficient manner.
// Thus it does not include its own GAM reference; instead we create one on
// the fly when needed.
#[derive(Debug, Copy, Clone)]
pub struct Slider {
    pub min: u32,
    pub max: u32,
    pub step: u32,
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub action_payload: u32,
    pub is_progressbar: bool,
    pub show_legend: bool,
    pub units: [u8; 8],
    pub units_len: usize,
}
impl Slider {
    pub fn new(
        action_conn: xous::CID,
        action_opcode: u32,
        min: u32,
        max: u32,
        step: u32,
        units: Option<&str>,
        initial_setting: u32,
        is_progressbar: bool,
        show_legend: bool,
    ) -> Self {
        let mut units_storage = [0u8; 8];
        let mut units_len = 0;
        if let Some(unit_str) = units {
            if unit_str.as_bytes().len() <= 8 {
                units_storage[..unit_str.as_bytes().len()].copy_from_slice(unit_str.as_bytes());
                units_len = unit_str.as_bytes().len();
            } else {
                log::error!(
                    "Unit string must be less than 8 *bytes* long (are you using unicode?), ignoring length {} string",
                    unit_str.as_bytes().len()
                );
            }
        }

        Slider {
            action_conn,
            action_opcode,
            is_progressbar,
            min,
            max,
            step,
            action_payload: initial_setting,
            units: units_storage,
            units_len,
            show_legend,
        }
    }

    pub fn set_is_progressbar(&mut self, setting: bool) { self.is_progressbar = setting; }

    pub fn set_state(&mut self, state: u32) {
        if state < self.min {
            self.action_payload = self.min;
        } else if state > self.max {
            self.action_payload = self.max;
        } else {
            self.action_payload = state;
        }
    }
}

use crate::{minigfx::PixelColor, widgets::ActionApi};
impl ActionApi for Slider {
    fn height(&self, glyph_height: isize, margin: isize, _modal: &Modal) -> isize {
        /*
        margin
            min            max    <- glyph height
             -----O----------     <- glyph height
                 legend
        margin
        */
        if self.show_legend { glyph_height * 3 + margin * 2 } else { glyph_height * 2 + margin * 2 }
    }

    fn set_action_opcode(&mut self, op: u32) { self.action_opcode = op }

    fn redraw(&self, at_height: isize, modal: &Modal) {
        log::debug!("    slider @ {}", at_height);
        let color = PixelColor::Light;
        let fill_color = PixelColor::Dark;

        // prime a textview with the correct general style parameters
        let mut tv = TextView::new(Gid::dummy(), TextBounds::BoundingBox(Rectangle::new_coords(0, 0, 1, 1)));
        tv.ellipsis = true;
        tv.style = modal.style;
        tv.invert = true;
        tv.draw_border = false;
        tv.margin = Point::new(0, 0);
        tv.insertion = None;

        let maxwidth = (modal.canvas_width - modal.margin * 2) as u16;
        if self.show_legend {
            log::info!(
                "{}{}",
                self.action_payload,
                String::from_utf8(self.units[..self.units_len].to_vec()).unwrap_or("UTF8-Err".to_string())
            );
            // estimate width of current setting
            tv.bounds_computed = None;
            tv.bounds_hint = TextBounds::GrowableFromTl(Point::new(0, 0), maxwidth);
            write!(
                tv,
                "{}{}",
                self.action_payload,
                String::from_utf8(self.units[..self.units_len].to_vec()).unwrap_or("UTF8-Err".to_string())
            )
            .unwrap();
            modal.gfx.bounds_compute_textview(&mut tv).expect("couldn't simulate text size");
            let textwidth = if let Some(bounds) = tv.bounds_computed {
                bounds.br.x - bounds.tl.x
            } else {
                maxwidth as isize
            };
            let offset = (modal.canvas_width - textwidth) / 2;
            // render current setting
            tv.bounds_computed = None;
            tv.bounds_hint = TextBounds::GrowableFromTl(
                Point::new(offset, at_height + modal.margin + modal.line_height * 1),
                maxwidth,
            );
            modal.gfx.draw_textview(&mut tv).expect("couldn't draw legend");
        }

        // the actual slider
        let mut draw_list = ObjectList::new();
        let outer_rect = Rectangle::new_with_style(
            Point::new(modal.margin * 2, modal.margin + modal.line_height * 0 + at_height),
            Point::new(
                modal.canvas_width - modal.margin * 2,
                modal.margin + modal.line_height * 1 + at_height,
            ),
            DrawStyle::new(fill_color, color, 2),
        );
        draw_list.push(ClipObjectType::Rect(outer_rect)).unwrap();
        let total_width = modal.canvas_width - modal.margin * 4;
        let slider_point =
            (total_width * (self.action_payload - self.min) as isize) / (self.max - self.min) as isize;
        let inner_rect = Rectangle::new_with_style(
            Point::new(modal.margin * 2, modal.margin + modal.line_height * 0 + at_height),
            Point::new(modal.margin * 2 + slider_point, modal.margin + modal.line_height * 1 + at_height),
            DrawStyle::new(color, color, 1),
        );
        draw_list.push(ClipObjectType::Rect(inner_rect)).unwrap();
        modal.gfx.draw_object_list(draw_list).expect("couldn't execute draw list");
    }

    fn key_action(&mut self, k: char) -> Option<ValidatorErr> {
        log::trace!("key_action: {}", k);
        if !self.is_progressbar {
            match k {
                '←' => {
                    if self.action_payload >= self.min + self.step {
                        self.action_payload -= self.step;
                    } else if self.action_payload >= self.min && self.action_payload < self.min + self.step {
                        self.action_payload = self.min
                    }
                }
                '→' => {
                    if self.action_payload <= self.max - self.step {
                        self.action_payload += self.step;
                    } else if self.action_payload < self.max && self.action_payload > self.max - self.step {
                        self.action_payload = self.max
                    }
                }
                '\u{0}' => {
                    // ignore null messages
                }
                '∴' | '\u{d}' | '🔥' => {
                    // relinquish focus before returning the result
                    let xns = xous_names::XousNames::new().unwrap();
                    let gfx = Gfx::new(&xns).unwrap();
                    gfx.release_modal().ok();
                    xous::yield_slice();

                    let ret_payload = SliderPayload(self.action_payload);

                    let buf =
                        xous_ipc::Buffer::into_buf(ret_payload).expect("couldn't convert message to payload");
                    buf.send(self.action_conn, self.action_opcode)
                        .map(|_| ())
                        .expect("couldn't send action message");

                    return None;
                }
                _ => {
                    // ignore all other messages
                }
            }
            None
        } else {
            if k == '🛑' {
                // use the "stop" emoji as a signal that we should close the progress bar
                // relinquish focus on stop
                let xns = xous_names::XousNames::new().unwrap();
                let gfx = Gfx::new(&xns).unwrap();
                log::info!("bef release modal");
                gfx.release_modal().ok();
                xous::yield_slice();
                log::info!("release modal");
                None
            } else {
                None
            }
        }
    }
}
