use std::fmt::Write;

use xous_ipc::Buffer;

use super::*;
use crate::blitstr2::glyph_height_hint;
use crate::minigfx::*;

pub struct Modal<'a> {
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
    pub name: String,

    // optimize draw time
    top_dirty: bool,
    top_memoized_height: Option<isize>,
    bot_dirty: bool,
    bot_memoized_height: Option<isize>,
}

impl<'a> Modal<'a> {
    pub fn new(
        name: &str,
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
        let mut modal = Modal {
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
            name: String::from(name),
            top_dirty: true,
            bot_dirty: true,
            top_memoized_height: None,
            bot_memoized_height: None,
            growable: false,
        };
        modal
    }

    pub fn activate(&self) {
        todo!();
    }

    pub fn redraw(&mut self) {
        todo!();
    }

    pub fn key_event(&mut self, keys: [char; 4]) {
        todo!();
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
    }
}
