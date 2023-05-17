use crate::*;

use graphics_server::api::*;

use core::fmt::Write;

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
    pub is_password: bool,
    pub show_legend: bool,
    pub units: xous_ipc::String::<8>,
}
impl Slider {
    pub fn new(action_conn: xous::CID, action_opcode: u32, min: u32, max: u32, step: u32, units: Option<&str>, initial_setting: u32, is_progressbar: bool, show_legend: bool) -> Self {
        let checked_units = if let Some(unit_str) = units {
            if unit_str.len() < 8 {
                String::<8>::from_str(unit_str)
            } else {
                log::error!("Unit string must be less than 8 *bytes* long (are you using unicode?), ignoring length {} string", unit_str.len());
                String::<8>::new()
            }
        } else {
            String::<8>::new() // just populate with a blank string, easier than checking Some/None later on everywhere
        };

        Slider {
            action_conn,
            action_opcode,
            is_password: false,
            is_progressbar,
            min,
            max,
            step,
            action_payload: initial_setting,
            units: checked_units,
            show_legend,
        }
    }

    pub fn set_is_progressbar(&mut self, setting: bool) {
        self.is_progressbar = setting;
    } 

    pub fn set_is_password(&mut self, setting: bool) {
        // this will cause text to be inverted. Untrusted entities can try to set this,
        // but the GAM should defeat this for dialog boxes outside of the trusted boot
        // set because they can't achieve a high enough trust level.
        self.is_password = setting;
    }
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
impl ActionApi for Slider {
    fn height(&self, glyph_height: i16, margin: i16, _modal: &Modal) -> i16 {
        /*
        margin
            min            max    <- glyph height
             -----O----------     <- glyph height
                 legend
        margin
        */
        if self.show_legend {
            glyph_height * 3 + margin * 2
        } else {
            glyph_height * 2 + margin * 2
        }
    }
    fn set_action_opcode(&mut self, op: u32) {self.action_opcode = op}

    fn redraw(&self, at_height: i16, modal: &Modal) {
        let color = if self.is_password {
            PixelColor::Light
        } else {
            PixelColor::Dark
        };
        let fill_color = if self.is_password {
            PixelColor::Dark
        } else {
            PixelColor::Light
        };

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

        let maxwidth = (modal.canvas_width - modal.margin * 2) as u16;
        if self.show_legend {
            /* // min/max doesn't look good, leave it out for now
            // render min
            tv.bounds_computed = None;
            tv.bounds_hint = TextBounds::GrowableFromTl(
                Point::new(modal.margin, at_height + modal.margin),
                maxwidth
            );
            tv.text.clear();
            write!(tv, "{}{}", self.min, self.units.to_str()).unwrap();
            modal.gam.post_textview(&mut tv).expect("couldn't post tv");
            // render max
            tv.bounds_computed = None;
            tv.bounds_hint = TextBounds::GrowableFromBr(
                Point::new(modal.canvas_width - modal.margin, at_height + modal.margin + modal.line_height),
                maxwidth
            );
            tv.text.clear();
            write!(tv, "{}{}", self.max, self.units.to_str()).unwrap();
            modal.gam.post_textview(&mut tv).expect("couldn't post tv");
            */
            // estimate width of current setting
            tv.bounds_computed = None;
            tv.bounds_hint = TextBounds::GrowableFromTl(
                Point::new(0, 0),
                maxwidth
            );
            write!(tv, "{}{}", self.action_payload, self.units.to_str()).unwrap();
            modal.gam.bounds_compute_textview(&mut tv).expect("couldn't simulate text size");
            let textwidth = if let Some(bounds) = tv.bounds_computed {
                bounds.br.x - bounds.tl.x
            } else {
                maxwidth as i16
            };
            let offset = (modal.canvas_width - textwidth) / 2;
            // render current setting
            tv.bounds_computed = None;
            tv.bounds_hint = TextBounds::GrowableFromTl(
                Point::new(offset, at_height + modal.margin + modal.line_height*2 + modal.margin),
                maxwidth
            );
            modal.gam.post_textview(&mut tv).expect("couldn't post tv");
        }

        // the actual slider
        let mut draw_list = GamObjectList::new(modal.canvas);
        let outer_rect = Rectangle::new_with_style(
            Point::new(modal.margin * 2, modal.margin + modal.line_height + at_height),
            Point::new(modal.canvas_width - modal.margin * 2, modal.margin + modal.line_height * 2 + at_height),
            DrawStyle::new(fill_color, color, 2)
        );
        draw_list.push(GamObjectType::Rect(outer_rect)).unwrap();
        let total_width = modal.canvas_width - modal.margin * 4;
        let slider_point = (total_width * (self.action_payload - self.min) as i16) / (self.max - self.min) as i16;
        let inner_rect = Rectangle::new_with_style(
            Point::new(modal.margin * 2, modal.margin + modal.line_height + at_height),
            Point::new(modal.margin * 2 + slider_point, modal.margin + modal.line_height * 2 + at_height),
            DrawStyle::new(color, color, 1)
        );
        draw_list.push(GamObjectType::Rect(inner_rect)).unwrap();
        modal.gam.draw_list(draw_list).expect("couldn't execute draw list");
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
                },
                '→' => {
                    if self.action_payload <= self.max - self.step {
                        self.action_payload += self.step;
                    } else if self.action_payload < self.max && self.action_payload > self.max - self.step {
                        self.action_payload = self.max
                    }
                },
                '\u{0}' => {
                    // ignore null messages
                }
                '∴' | '\u{d}' => {
                    // relinquish focus before returning the result
                    let gam = crate::Gam::new(&xous_names::XousNames::new().unwrap()).unwrap();
                    gam.relinquish_focus().unwrap();
                    xous::yield_slice();

                    let ret_payload = SliderPayload(self.action_payload);

                    let buf = Buffer::into_buf(ret_payload).expect("couldn't convert message to payload");
                    buf.send(self.action_conn, self.action_opcode).map(|_| ()).expect("couldn't send action message");

                    return None;
                }
                _ => {
                    // ignore all other messages
                }
            }
            None
        } else {
            if k == '🛑' { // use the "stop" emoji as a signal that we should close the progress bar
                // relinquish focus on stop
                let gam = crate::Gam::new(&xous_names::XousNames::new().unwrap()).unwrap();
                gam.relinquish_focus().unwrap();
                xous::yield_slice();

                None
            } else {
                None
            }
        }
    }
}
