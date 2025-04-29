use std::fmt::Write;

use blitstr2::{GlyphStyle, glyph_height_hint};
use xous_ipc::Buffer;

use super::*;
use crate::minigfx::op::HEIGHT;
use crate::minigfx::*;
use crate::platform::{LINES, WIDTH};
use crate::service::api::Gid;
use crate::service::gfx::Gfx;

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum ModalOpcode {
    // if changes are made here, also update MenuOpcode
    Redraw = 0x4000_0000, /* set the high bit so that "standard" enums don't conflict with the
                           * Modal-specific opcodes */
    Rawkeys,
    Quit,
}

pub struct Modal<'a> {
    pub gfx: Gfx,
    pub top_text: Option<TextView>,
    pub bot_text: Option<TextView>,
    pub action: ActionType,
    /// This is a slightly unsafe option, in that it only works if you have a simple TextEntry
    /// box. It enables Line-by-line text reflow selection. If false, then each entry gets only one line.
    /// If set to true, operations may slow down proportionally with the size of the text as we have to
    /// recompute text reflow every time the object is touched. The algorithm will greedily consume
    /// space in the canvas until the entire canvas is filled with the TextEntry box. This means that
    /// if you have a compound modal (multiple elements in it), the extra elements will be flowed off
    /// the bottom.
    pub growable: bool,

    //pub index: usize, // currently selected item
    pub margin: isize,
    pub line_height: isize,
    pub canvas_width: isize,
    pub maximal_height: isize,
    pub inverted: bool,
    pub style: GlyphStyle,
    pub helper_data: Option<Buffer<'a>>,

    // optimize draw time
    top_dirty: bool,
    top_memoized_height: Option<isize>,
    bot_dirty: bool,
    bot_memoized_height: Option<isize>,
}

impl<'a> Modal<'a> {
    pub fn new(
        _compat_name: &str,
        action: ActionType,
        top_text: Option<&str>,
        bot_text: Option<&str>,
        style: GlyphStyle,
        margin: isize,
    ) -> Modal<'a> {
        let line_height = if locales::LANG == "zh" {
            // zh has no "small" style
            glyph_height_hint(GlyphStyle::Regular) as isize
        } else {
            glyph_height_hint(style) as isize
        };

        log::trace!("initializing Modal structure");
        let inverted = false;

        // we now have a canvas that is some minimal height, but with the final width as allowed by the GAM.
        // compute the final height based upon the contents within.
        let xns = xous_names::XousNames::new().unwrap();
        let mut modal = Modal {
            gfx: Gfx::new(&xns).unwrap(),
            top_text: None,
            bot_text: None,
            action,
            margin,
            line_height,
            canvas_width: crate::platform::WIDTH as isize, // memoize this, it shouldn't change
            maximal_height: 402,                           /* arbitrary number set for aesthetic reasons;
                                                            * limits
                                                            * growth of
                                                            * modals that request reflowable/growable text
                                                            * boxes */
            inverted,
            style,
            helper_data: None,
            top_dirty: true,
            bot_dirty: true,
            top_memoized_height: None,
            bot_memoized_height: None,
            growable: false,
        };
        layout(&mut modal, top_text, bot_text, style);

        modal
    }

    pub fn activate(&mut self) {
        self.gfx.acquire_modal().expect("Couldn't acquire lock on graphics subsystem");
        self.gfx.clear().ok();
        self.redraw();
        self.gfx.flush().ok();
    }

    pub fn redraw(&mut self) {
        let do_redraw = self.top_dirty || self.bot_dirty || self.inverted;
        if do_redraw {
            self.gfx.clear().unwrap();
        }
        let mut cur_height = self.margin;
        if let Some(mut tv) = self.top_text.as_mut() {
            if do_redraw {
                self.gfx.draw_textview(&mut tv).expect("couldn't draw text");
                if let Some(bounds) = tv.bounds_computed {
                    let y = bounds.br.y - bounds.tl.y;
                    let y_clip = if y > HEIGHT - self.line_height * 3 {
                        log::warn!("oversize text, clipping back {}", HEIGHT - (self.line_height * 2));
                        HEIGHT - (self.line_height * 2)
                    } else {
                        y
                    };
                    cur_height += y_clip;
                    log::trace!("top_tv height: {}", y_clip);
                    self.top_memoized_height = Some(y_clip);
                } else {
                    log::warn!("text bounds didn't compute setting to max");
                    self.top_memoized_height = Some(HEIGHT - (self.line_height * 2));
                }
                self.top_dirty = false;
            } else {
                cur_height +=
                    self.top_memoized_height.expect("internal error: memoization didn't work correctly");
            }
        } else {
            self.top_dirty = false;
        }

        let action_height = self.action.height(self.line_height, self.margin, &self);

        let action_resolver: Box<&dyn ActionApi> = Box::new(&self.action);
        action_resolver.redraw(cur_height, &self);

        cur_height += action_height;

        if let Some(mut tv) = self.bot_text.as_mut() {
            if do_redraw {
                self.gfx.draw_textview(&mut tv).expect("couldn't draw text");
                if let Some(bounds) = tv.bounds_computed {
                    cur_height += bounds.br.y - bounds.tl.y;
                    self.bot_memoized_height = Some(bounds.br.y - bounds.tl.y);
                }
                self.bot_dirty = false;
            } else {
                cur_height +=
                    self.bot_memoized_height.expect("internal error: memoization didn't work correctly");
            }
        } else {
            self.bot_dirty = false;
        }
        log::trace!("total height: {}", cur_height);
        log::trace!("modal redraw##");
        self.gfx.flush().unwrap();
    }

    pub fn key_event(&mut self, keys: [char; 4]) {
        for &k in keys.iter() {
            if k != '\u{0}' {
                log::debug!("got key '{}'", k);
                let action_resolver: Box<&mut dyn ActionApi> = Box::new(&mut self.action);
                let err = action_resolver.key_action(k);
                if let Some(err_msg) = err {
                    self.modify(None, None, false, Some(&err_msg), false, None);
                }
            }
        }
        self.redraw();
    }

    /// This empowers an action within a modal to potentially consume all the available height in a canvas
    /// The current implementation works if you have a "simple" TextEntry box, but it will fail if you have
    /// stuff below it because the algorithm can't "see" the reserved space at the moment for extra items
    /// below.
    pub fn set_growable(&mut self, state: bool) { self.growable = state; }

    /// this function will modify UX elements if any of the arguments are Some()
    /// if None, the element is unchanged.
    /// If a text section is set to remove, but Some() is given for the update, the text is not removed, and
    /// instead replaced with the updated text.
    pub fn modify(
        &mut self,
        update_action: Option<ActionType>,
        update_top_text: Option<&str>,
        remove_top: bool,
        update_bot_text: Option<&str>,
        remove_bot: bool,
        update_style: Option<GlyphStyle>,
    ) {
        if let Some(action) = update_action {
            self.action = action;
        };

        if remove_top {
            self.top_dirty = true;
            self.top_text = None;
        }
        if remove_bot {
            self.bot_dirty = true;
            self.bot_text = None;
        }
        if update_top_text.is_some() {
            self.top_dirty = true;
        }
        if update_bot_text.is_some() {
            self.bot_dirty = true;
        }

        let mut top_tv_temp = String::new(); // size matches that used in TextView
        if let Some(top_text) = update_top_text {
            write!(top_tv_temp, "{}", top_text).unwrap();
        } else {
            if let Some(top_text) = self.top_text.as_ref() {
                write!(top_tv_temp, "{}", top_text).unwrap();
            }
        };
        let top_text = if self.top_text.is_none() && update_top_text.is_none() {
            None
        } else {
            Some(top_tv_temp.as_str())
        };

        let mut bot_tv_temp = String::new(); // size matches that used in TextView
        if let Some(bot_text) = update_bot_text {
            write!(bot_tv_temp, "{}", bot_text).unwrap();
        } else {
            if let Some(bot_text) = self.bot_text.as_ref() {
                write!(bot_tv_temp, "{}", bot_text).unwrap();
            }
        };
        let bot_text = if self.bot_text.is_none() && update_bot_text.is_none() {
            None
        } else {
            Some(bot_tv_temp.as_str())
        };

        let style = if let Some(style) = update_style {
            self.top_dirty = true;
            self.bot_dirty = true;
            style
        } else {
            self.style
        };
        layout(self, top_text, bot_text, style);
    }
}

pub fn screen_bounds() -> Point { Point::new(WIDTH as isize, LINES as isize) }

// comment this out while I figure out how to shim in the HAL layer for the graphics emulation
fn layout(modal: &mut Modal, top_text: Option<&str>, bot_text: Option<&str>, style: GlyphStyle) {
    // we need to set a "max" size to our modal box, so that the text computations don't fail later on
    let current_bounds = screen_bounds();

    // method:
    //   - we assume the GAM gives us an initial modal with a "maximum" height setting
    //   - items are populated within this maximal canvas setting, and then the actual height needed is
    //     computed
    //   - the canvas is resized to this actual height
    // problems:
    //   - there is no sanity check on the size of the text boxes. So if you give the UX element a top_text
    //     box that's huge, it will just overflow the canvas size and nothing else will get drawn.

    let mut total_height = modal.margin;
    log::trace!("step 0 total_height: {}", total_height);
    // compute height of top_text, if any
    if let Some(top_str) = top_text {
        if top_str.len() > 0 {
            let mut top_tv = TextView::new(
                Gid::dummy(),
                TextBounds::GrowableFromTl(
                    Point::new(modal.margin, modal.margin),
                    (modal.canvas_width - modal.margin * 2) as u16,
                ),
            );
            top_tv.draw_border = false;
            top_tv.style = style;
            top_tv.margin = Point::new(0, 0); // all margin already accounted for in the raw bounds of the text drawing
            top_tv.ellipsis = false;
            top_tv.invert = true;
            // specify a clip rect that's the biggest possible allowed. If we don't do this, the current
            // canvas bounds are used, and the operation will fail if the text has to get bigger.
            top_tv.clip_rect = Some(Rectangle::new(
                Point::new(0, 0),
                Point::new(current_bounds.x, LINES - 2 * modal.line_height),
            ));
            write!(top_tv.text, "{}", top_str).unwrap();

            log::trace!("posting top tv: {:?}", top_tv);
            modal.gfx.bounds_compute_textview(&mut top_tv).expect("couldn't simulate top text size");
            if let Some(bounds) = top_tv.bounds_computed {
                log::trace!("top_tv bounds computed {}", bounds.br.y - bounds.tl.y);
                total_height += bounds.br.y - bounds.tl.y;
            } else {
                log::warn!("couldn't compute height for modal top_text: {:?}", top_tv);
                // probably should find a better way to deal with this.
                total_height += LINES - (modal.line_height * 2);
            }
            modal.top_text = Some(top_tv);
        } else {
            modal.top_text = None;
        }
    }
    total_height += modal.margin;

    // compute height of action item
    log::trace!("step 1 total_height: {}", total_height);
    total_height +=
        modal.action.height(modal.line_height.try_into().unwrap(), modal.margin.try_into().unwrap(), &modal)
            as isize;
    total_height += modal.margin;

    // compute height of bot_text, if any
    log::trace!("step 2 total_height: {}", total_height);
    if let Some(bot_str) = bot_text {
        let mut bot_tv = TextView::new(
            Gid::dummy(),
            TextBounds::GrowableFromTl(
                Point::new(modal.margin, total_height),
                (modal.canvas_width - modal.margin * 2) as u16,
            ),
        );
        bot_tv.draw_border = false;
        bot_tv.style = style;
        bot_tv.margin = Point::new(0, 0); // all margin already accounted for in the raw bounds of the text drawing
        bot_tv.ellipsis = false;
        bot_tv.invert = true;
        // specify a clip rect that's the biggest possible allowed. If we don't do this, the current canvas
        // bounds are used, and the operation will fail if the text has to get bigger.
        bot_tv.clip_rect = Some(Rectangle::new(
            Point::new(0, 0),
            Point::new(current_bounds.x, LINES - 2 * modal.line_height),
        ));
        write!(bot_tv.text, "{}", bot_str).unwrap();

        log::trace!("posting bot tv: {:?}", bot_tv);
        modal.gfx.bounds_compute_textview(&mut bot_tv).expect("couldn't simulate bot text size");
        if let Some(bounds) = bot_tv.bounds_computed {
            total_height += bounds.br.y - bounds.tl.y;
        } else {
            log::error!("couldn't compute height for modal bot_text: {:?}", bot_tv);
            panic!("couldn't compute height for modal bot_text");
        }
        modal.bot_text = Some(bot_tv);
        total_height += modal.margin;
    }
    log::trace!("step 3 total_height: {}", total_height);
}
