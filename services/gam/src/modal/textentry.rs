use core::cell::Cell;
use core::fmt::Write;
use std::cell::RefCell;

use blitstr2::*;
use num_traits::*;
use ux_api::minigfx::*;
use xous_ipc::Buffer;

use crate::*;

// TODO: figure out this, do we really have to limit ourselves to 10?
const MAX_FIELDS: isize = 10;

pub type ValidatorErr = String;

pub type Payloads = [TextEntryPayload; MAX_FIELDS as usize];

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Eq, PartialEq, Default)]
pub struct TextEntryPayloads(Payloads, usize);

impl TextEntryPayloads {
    pub fn first(&self) -> TextEntryPayload { self.0[0].clone() }

    pub fn content(&self) -> Vec<TextEntryPayload> { self.0[..self.1].to_vec() }
}

#[derive(Debug, Copy, Clone, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum TextEntryVisibility {
    /// text is fully visible
    Visible = 0,
    /// only last chars are shown of text entry, the rest obscured with *
    LastChars = 1,
    /// all chars hidden as *
    Hidden = 2,
}

#[derive(Clone)]
pub struct TextEntry {
    pub is_password: bool,
    pub visibility: TextEntryVisibility,
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    // validator borrows the text entry payload, and returns an error message if something didn't go well.
    // validator takes as ragument the current action_payload, and the current action_opcode
    pub validator: Option<fn(TextEntryPayload, u32) -> Option<ValidatorErr>>,
    pub action_payloads: Vec<TextEntryPayload>,

    max_field_amount: u32,
    selected_field: isize,
    field_height: Cell<isize>,
    /// track if keys were hit since initialized: this allows us to clear the default text,
    /// instead of having it re-appear every time the text area is cleared
    keys_hit: [bool; MAX_FIELDS as usize],
    // gam: crate::Gam, // no GAM field because this needs to be a clone-capable structure. We create a GAM
    // handle when we need it.
    /// Stores the allowed height of a given text line, based on the contents and the space available
    /// in the box. The height of a given line may be limited to make sure there is enough space for
    /// later lines to be rendered.
    action_payloads_allowed_heights: RefCell<Vec<isize>>,
}

impl Default for TextEntry {
    fn default() -> Self {
        Self {
            is_password: Default::default(),
            visibility: TextEntryVisibility::Visible,
            action_conn: Default::default(),
            action_opcode: Default::default(),
            validator: Default::default(),
            selected_field: Default::default(),
            action_payloads: Default::default(),
            max_field_amount: 0,
            field_height: Cell::new(0),
            keys_hit: [false; MAX_FIELDS as usize],
            action_payloads_allowed_heights: RefCell::new(Vec::new()),
        }
    }
}

impl TextEntry {
    pub fn new(
        is_password: bool,
        visibility: TextEntryVisibility,
        action_conn: xous::CID,
        action_opcode: u32,
        action_payloads: Vec<TextEntryPayload>,
        validator: Option<fn(TextEntryPayload, u32) -> Option<ValidatorErr>>,
    ) -> Self {
        if action_payloads.len() as isize > MAX_FIELDS {
            panic!("can't have more than {} fields, found {}", MAX_FIELDS, action_payloads.len());
        }

        Self {
            is_password,
            visibility,
            action_conn,
            action_opcode,
            action_payloads,
            validator,
            ..Default::default()
        }
    }

    pub fn reset_action_payloads(&mut self, fields: u32, placeholders: Option<[Option<(String, bool)>; 10]>) {
        let mut payload = vec![TextEntryPayload::default(); fields as usize];

        if let Some(placeholders) = placeholders {
            for (index, element) in payload.iter_mut().enumerate() {
                if let Some((p, persist)) = &placeholders[index] {
                    element.placeholder = Some(p.to_string());
                    element.placeholder_persist = *persist;
                } else {
                    element.placeholder = None;
                    element.placeholder_persist = false;
                }
                element.insertion_point = None;
            }
        }

        self.action_payloads = payload;
        self.max_field_amount = fields;
        self.keys_hit = [false; MAX_FIELDS as usize];
    }

    fn get_bullet_margin(&self) -> isize {
        if self.action_payloads.len() > 1 {
            17 // this is the margin for drawing the selection bullet
        } else {
            0 // no selection bullet
        }
    }
}

impl ActionApi for TextEntry {
    fn set_action_opcode(&mut self, op: u32) { self.action_opcode = op }

    fn is_password(&self) -> bool { self.is_password }

    /// The total canvas height is computed with this API call
    /// The canvas height is not dynamically adjustable for modals.
    fn height(&self, glyph_height: isize, margin: isize, modal: &Modal) -> isize {
        /*
            -------------------
            | ****            |    <-- glyph_height + 2*margin
            -------------------
                ← 👁️ 🕶️ * →        <-- glyph_height

            + 2 * margin top/bottom

            auto-closes on enter
        */
        // the glyph_height is an opaque value because the final value depends upon a lookup table
        // stored in the graphics_server crate. To obtain this would require a handle to that server
        // which is private to the GAM. Thus we receive a copy of this from our caller and stash it
        // here for future reference. `glyph_height` can change depending upon the locale; in particular,
        // CJK languages have taller glyphs.
        self.field_height.set(glyph_height + 2 * margin); // stash a copy for later

        // compute the overall_height of the entry fields
        let mut overall_height = if modal.growable {
            let mut editing = false;
            for &hit in self.keys_hit.iter() {
                if hit {
                    editing = true;
                }
            }
            if editing {
                // don't recompute heights if text fields are being edited
                let mut sum = 0;
                for &prev_heights in self.action_payloads_allowed_heights.borrow().iter() {
                    sum += prev_heights;
                }
                sum
            } else {
                self.action_payloads_allowed_heights.borrow_mut().clear();
                let growable_limit = modal.maximal_height;
                // minimum size required to display exactly one line for every field.
                // decrement this by a field_height.get() for every line of content computed.
                let mut minimum_size_remaining =
                    self.field_height.get() * self.action_payloads.len() as isize;
                if growable_limit < minimum_size_remaining + self.field_height.get() {
                    // if the user is dumb and specified a growable limit that allows no space for growth,
                    // "snap" the user spec to something big enough to actually show one line of each bit of
                    // text, and skip the computation because we know there is nothing to
                    // gain from doing it.
                    for _ in 0..self.action_payloads.len() {
                        self.action_payloads_allowed_heights.borrow_mut().push(self.field_height.get());
                    }
                    minimum_size_remaining
                } else {
                    let bullet_margin = self.get_bullet_margin();
                    let left_text_margin = modal.margin + bullet_margin; // space for the bullet point on the left, if it's there
                    let mut current_height = 0;

                    for payload in self.action_payloads.iter() {
                        let mut tv = TextView::new(
                            modal.canvas,
                            TextBounds::GrowableFromBl(
                                Point::new(left_text_margin, current_height),
                                (modal.canvas_width - (modal.margin + bullet_margin) - left_text_margin)
                                    as u16,
                            ),
                        );
                        tv.ellipsis = false;
                        tv.invert = self.is_password;
                        tv.style = if self.is_password { GlyphStyle::Monospace } else { modal.style };
                        tv.margin = Point::new(0, 0);
                        tv.draw_border = false;
                        tv.text.clear();
                        let content = {
                            if payload.placeholder.is_some() && payload.content.len().is_zero() {
                                let placeholder_content = payload.placeholder.as_deref().unwrap();
                                placeholder_content.to_string()
                            } else {
                                payload.content.to_string()
                            }
                        };
                        write!(tv.text, "{}", &content).unwrap();
                        // select to just compute bounds, not render the text
                        modal.gam.bounds_compute_textview(&mut tv).expect("couldn't flow textview");
                        if let Some(computed_bounds) = tv.bounds_computed {
                            log::debug!("computed height: {} for {}", computed_bounds.height(), content);
                            // see if we can "afford" to grow the text to accommodate the total height
                            let required_extra_height =
                                (computed_bounds.height() as isize // height of text as flowed, with no margins
                                + 2*margin  // margin between boxes
                                + minimum_size_remaining)
                                    .saturating_sub(self.field_height.get()); // subtract one line from the size remaining to account for the fact that we allocated one line for this field already
                            let provisioned_height =
                                if required_extra_height + current_height > growable_limit {
                                    // it doesn't fit, reduce height so it does fit
                                    // this algorithm will greedily allocate height to the first fields, but
                                    // will always leave at least one line
                                    // for the remaining yet-to-be-flowed text fields
                                    growable_limit - current_height
                                    // we could, I suppose, at this point just stop rendering the future
                                    // fields and populate the allowable
                                    // heights based on one line each for the remainder...
                                } else {
                                    // it can fit, move on to the next field
                                    computed_bounds.height() as isize + 2 * margin
                                };
                            self.action_payloads_allowed_heights.borrow_mut().push(provisioned_height);
                            current_height += provisioned_height; // reset the current height to after the computed height
                            minimum_size_remaining -= self.field_height.get(); // decrement one line from the minimum remaining
                        } else {
                            log::warn!("{} did not have a computed height!", content);
                        }
                    }
                    current_height
                }
            }
        } else {
            self.field_height.get() * self.action_payloads.len() as isize
        };

        // if we're a password, we add an extra glyph_height to the bottom for the text visibility items
        if self.is_password {
            overall_height += glyph_height;
        }

        overall_height
    }

    fn redraw(&self, at_height: isize, modal: &Modal) {
        const MAX_CHARS: usize = 33;
        let color = if self.is_password { PixelColor::Light } else { PixelColor::Dark };

        let mut current_height = at_height;
        let payloads = self.action_payloads.clone();

        let bullet_margin = self.get_bullet_margin();

        for (index, payload) in payloads.iter().enumerate() {
            log::debug!("{}: {}", index, current_height);
            if index as isize == self.selected_field && payloads.len() > 1 {
                // draw the dot
                let mut tv = TextView::new(
                    modal.canvas,
                    TextBounds::BoundingBox(Rectangle::new(
                        Point::new(modal.margin, current_height),
                        Point::new(modal.canvas_width - modal.margin, current_height + modal.line_height),
                    )),
                );

                tv.text.clear();
                tv.bounds_computed = None;
                tv.draw_border = false;
                write!(tv, "•").unwrap(); // emoji glyph will be summoned in this case
                modal.gam.post_textview(&mut tv).expect("couldn't post tv");
            }

            let left_text_margin = modal.margin + bullet_margin; // space for the bullet point on the left, if it's there

            // draw the currently entered text
            let mut tv = if modal.growable {
                assert!(self.action_payloads_allowed_heights.borrow().len() == payloads.len());
                // growable limit was set, we know what the height of every field should be already, in
                // theory!
                TextView::new(
                    modal.canvas,
                    TextBounds::GrowableFromBl(
                        Point::new(
                            left_text_margin,
                            current_height + self.action_payloads_allowed_heights.borrow()[index]
                                - modal.margin
                                - 4,
                        ),
                        (modal.canvas_width - (modal.margin + bullet_margin)) as u16,
                    ),
                )
            } else {
                TextView::new(
                    modal.canvas,
                    TextBounds::BoundingBox(Rectangle::new(
                        Point::new(left_text_margin, current_height),
                        Point::new(
                            modal.canvas_width - (modal.margin + bullet_margin),
                            current_height + modal.line_height,
                        ),
                    )),
                )
            };
            tv.ellipsis = true;
            tv.invert = self.is_password;
            tv.style = if self.is_password {
                GlyphStyle::Monospace
            } else {
                if payload.placeholder.is_some() && payload.content.len().is_zero() && self.keys_hit[index] {
                    // note: this is just a "recommendation" - if there is an emoji or chinese character in
                    // this string, the height revers to modal.style's height
                    GlyphStyle::Small
                } else {
                    modal.style
                }
            };
            tv.margin = Point::new(0, 0);
            tv.draw_border = false;
            tv.insertion = if let Some(index) = payload.insertion_point {
                Some(index as i32)
            } else {
                Some(payload.content.len() as i32)
            };
            tv.text.clear(); // make sure this is blank
            let payload_chars = payload.content.as_str().chars().count();
            // TODO: condense the "above MAX_CHARS" chars length path a bit -- written out "the dumb way" just
            // to reason out the logic a bit
            match self.visibility {
                TextEntryVisibility::Visible => {
                    let content = {
                        if payload.placeholder.is_some()
                            && payload.content.len().is_zero()
                            && !self.keys_hit[index]
                        {
                            let placeholder_content = payload.placeholder.as_ref().unwrap();
                            placeholder_content.to_string()
                        } else {
                            payload.content.to_string()
                        }
                    };

                    log::trace!("action payload: {}", content);
                    if modal.growable {
                        tv.bounds_hint = TextBounds::GrowableFromBl(
                            Point::new(
                                left_text_margin,
                                current_height + self.action_payloads_allowed_heights.borrow()[index]
                                    - modal.margin
                                    - 4,
                            ),
                            (modal.canvas_width - (modal.margin + bullet_margin) - left_text_margin) as u16,
                        );
                        write!(tv.text, "{}", content).unwrap();
                    } else {
                        if payload_chars < MAX_CHARS {
                            write!(tv.text, "{}", content).unwrap();
                        } else {
                            write!(tv.text, "...{}", &content[content.chars().count() - (MAX_CHARS - 3)..])
                                .unwrap();
                        }
                    }
                    modal.gam.post_textview(&mut tv).expect("couldn't post textview");
                }
                TextEntryVisibility::Hidden => {
                    if payload_chars < MAX_CHARS {
                        for _char in payload.content.as_str().chars() {
                            tv.text.push('*');
                        }
                    } else {
                        // just render a pure dummy string
                        tv.text.push('.');
                        tv.text.push('.');
                        tv.text.push('.');
                        for _ in 0..(MAX_CHARS - 3) {
                            tv.text.push('*');
                        }
                    }
                    modal.gam.post_textview(&mut tv).expect("couldn't post textview");
                }
                TextEntryVisibility::LastChars => {
                    if payload_chars < MAX_CHARS {
                        let hide_to = if payload.content.as_str().chars().count() >= 2 {
                            payload.content.as_str().chars().count() - 2
                        } else {
                            0
                        };
                        for (index, ch) in payload.content.as_str().chars().enumerate() {
                            if index < hide_to {
                                tv.text.push('*');
                            } else {
                                tv.text.push(ch);
                            }
                        }
                    } else {
                        tv.text.push('.');
                        tv.text.push('.');
                        tv.text.push('.');
                        let hide_to = if payload.content.as_str().chars().count() >= 2 {
                            payload.content.as_str().chars().count() - 2
                        } else {
                            0
                        };
                        for (index, ch) in
                            payload.content.as_str()[payload_chars - (MAX_CHARS - 3)..].chars().enumerate()
                        {
                            if index + payload_chars - (MAX_CHARS - 3) < hide_to {
                                tv.text.push('*');
                            } else {
                                tv.text.push(ch);
                            }
                        }
                    }
                    modal.gam.post_textview(&mut tv).expect("couldn't post textview");
                }
            }
            if self.is_password {
                let aesthetic_margin = match crate::SYSTEM_STYLE {
                    GlyphStyle::Tall => 4,
                    _ => 0,
                };
                let select_index = match self.visibility {
                    TextEntryVisibility::Visible => 0,
                    TextEntryVisibility::LastChars => 1,
                    TextEntryVisibility::Hidden => 2,
                };
                let prompt_width = glyph_to_height_hint(GlyphStyle::Monospace) as isize * 4;
                let lr_margin = (modal.canvas_width - prompt_width * 3) / 2;
                let left_edge = lr_margin;

                let mut tv = TextView::new(
                    modal.canvas,
                    TextBounds::GrowableFromTl(
                        Point::new(
                            modal.margin,
                            at_height
                                + glyph_to_height_hint(GlyphStyle::Monospace) as isize
                                + modal.margin
                                + aesthetic_margin,
                        ),
                        lr_margin as u16,
                    ),
                );
                tv.style = GlyphStyle::Large;
                tv.margin = Point::new(0, 0);
                tv.invert = self.is_password;
                tv.draw_border = false;
                tv.text.clear();
                write!(tv.text, "\u{2b05}").unwrap();
                modal.gam.post_textview(&mut tv).expect("couldn't post textview");

                for i in 0..3 {
                    let mut tv = TextView::new(
                        modal.canvas,
                        TextBounds::GrowableFromTl(
                            Point::new(
                                left_edge + i * prompt_width,
                                at_height
                                    + glyph_to_height_hint(GlyphStyle::Monospace) as isize
                                    + modal.margin
                                    + aesthetic_margin,
                            ),
                            prompt_width as u16,
                        ),
                    );
                    tv.style = GlyphStyle::Monospace;
                    tv.margin = Point::new(8, 8);
                    if i == select_index {
                        tv.invert = !self.is_password;
                        tv.draw_border = true;
                        tv.rounded_border = Some(6);
                    } else {
                        tv.invert = self.is_password;
                        tv.draw_border = false;
                        tv.rounded_border = None;
                    }
                    tv.text.clear();
                    match i {
                        0 => write!(tv.text, "abcd").unwrap(),
                        1 => write!(tv.text, "ab**").unwrap(),
                        _ => write!(tv.text, "****").unwrap(),
                    }
                    modal.gam.post_textview(&mut tv).expect("couldn't post textview");
                }

                let mut tv = TextView::new(
                    modal.canvas,
                    TextBounds::GrowableFromTr(
                        Point::new(
                            modal.canvas_width - modal.margin,
                            at_height
                                + glyph_to_height_hint(GlyphStyle::Monospace) as isize
                                + modal.margin
                                + aesthetic_margin,
                        ),
                        lr_margin as u16,
                    ),
                );
                tv.style = GlyphStyle::Large;
                tv.margin = Point::new(0, 0);
                tv.invert = self.is_password;
                tv.draw_border = false;
                tv.text.clear();
                // minor bug - needs a trailing space on the right to make this emoji render. it's an issue in
                // the word wrapper, but it's too late at night for me to figure this out right now.
                write!(tv.text, "\u{27a1} ").unwrap();
                modal.gam.post_textview(&mut tv).expect("couldn't post textview");
            }

            // draw a line for where text gets entered (don't use a box, fitting could be awkward)
            let line_height = if modal.growable {
                self.action_payloads_allowed_heights.borrow()[index] - modal.margin
            } else {
                modal.line_height
            };
            modal
                .gam
                .draw_line(
                    modal.canvas,
                    Line::new_with_style(
                        Point::new(left_text_margin, current_height + line_height + 3),
                        Point::new(
                            modal.canvas_width - (modal.margin + bullet_margin),
                            current_height + line_height + 3,
                        ),
                        DrawStyle::new(color, color, 1),
                    ),
                )
                .expect("couldn't draw entry line");

            if modal.growable {
                current_height += self.action_payloads_allowed_heights.borrow()[index];
            } else {
                current_height += self.field_height.get();
            }
        }
    }

    fn key_action(&mut self, k: char) -> Option<ValidatorErr> {
        // needs to be a reference, otherwise we're operating on a copy of the payload!
        let payload = &mut self.action_payloads[self.selected_field as usize];

        let can_move_downwards = !(self.selected_field + 1 == self.max_field_amount as isize);
        let can_move_upwards = !(self.selected_field - 1 < 0);

        log::trace!("key_action: {}", k);
        match k {
            '←' => {
                if (self.visibility as u32 > 0) && self.is_password {
                    match FromPrimitive::from_u32(self.visibility as u32 - 1) {
                        Some(new_visibility) => {
                            log::trace!("new visibility: {:?}", new_visibility);
                            self.visibility = new_visibility;
                        }
                        _ => {
                            panic!("internal error: an TextEntryVisibility did not resolve correctly");
                        }
                    }
                } else if !self.is_password {
                    if payload.content.len() == 0 {
                        if let Some(placeholder) = payload.placeholder.as_ref() {
                            payload.content.push_str(placeholder);
                        }
                    } else {
                        if let Some(index) = payload.insertion_point {
                            payload.insertion_point = Some(index.saturating_sub(1));
                        } else {
                            payload.insertion_point = Some(payload.content.len().saturating_sub(1));
                        }
                    }
                }
            }
            '→' => {
                if ((self.visibility as u32) < (TextEntryVisibility::Hidden as u32)) && self.is_password {
                    match FromPrimitive::from_u32(self.visibility as u32 + 1) {
                        Some(new_visibility) => {
                            log::trace!("new visibility: {:?}", new_visibility);
                            self.visibility = new_visibility
                        }
                        _ => {
                            panic!("internal error: an TextEntryVisibility did not resolve correctly");
                        }
                    }
                } else if !self.is_password {
                    if payload.content.len() == 0 {
                        if let Some(placeholder) = payload.placeholder.as_ref() {
                            payload.content.push_str(placeholder);
                        }
                    } else {
                        if let Some(index) = payload.insertion_point.take() {
                            if index + 1 < payload.content.len() {
                                payload.insertion_point = Some(index + 1);
                            }
                        } else {
                            // going right on a field without an insertion point does nothing
                        }
                    }
                }
            }
            '∴' | '\u{d}' => {
                if payload.content.len() == 0 && !self.keys_hit[self.selected_field as usize] {
                    if let Some(placeholder) = payload.placeholder.as_ref() {
                        payload.content.push_str(placeholder);
                    }
                }
                if let Some(validator) = self.validator {
                    if let Some(err_msg) = validator(payload.clone(), self.action_opcode) {
                        payload.content.clear(); // reset the input field
                        return Some(err_msg);
                    }
                }

                // now check all the other potential fields for default re-population
                for (index, raw_payload) in
                    self.action_payloads[..self.max_field_amount as usize].iter_mut().enumerate()
                {
                    if raw_payload.content.len() == 0 && !self.keys_hit[index] {
                        if let Some(placeholder) = raw_payload.placeholder.as_ref() {
                            raw_payload.content.push_str(placeholder);
                        }
                    }
                }
                // relinquish focus before returning the result
                let gam = crate::Gam::new(&xous_names::XousNames::new().unwrap()).unwrap();
                gam.relinquish_focus().unwrap();
                xous::yield_slice();

                let mut payloads: TextEntryPayloads = Default::default();
                payloads.1 = self.max_field_amount as usize;
                for (dst, src) in payloads.0[..self.max_field_amount as usize]
                    .iter_mut()
                    .zip(self.action_payloads[..self.max_field_amount as usize].iter())
                {
                    *dst = src.clone();
                }
                let buf = Buffer::into_buf(payloads).expect("couldn't convert message to payload");
                buf.send(self.action_conn, self.action_opcode)
                    .map(|_| ())
                    .expect("couldn't send action message");

                for payload in self.action_payloads.iter_mut() {
                    payload.volatile_clear();
                }
                self.keys_hit[self.selected_field as usize] = false;

                return None;
            }
            '↑' => {
                if can_move_upwards {
                    self.selected_field -= 1
                }
            }
            '↓' => {
                if can_move_downwards {
                    self.selected_field += 1
                }
            }
            '\u{0}' => {
                // ignore null messages
            }
            '\u{8}' => {
                // backspace
                self.keys_hit[self.selected_field as usize] = true;
                #[cfg(feature = "tts")]
                {
                    let xns = xous_names::XousNames::new().unwrap();
                    let tts = tts_frontend::TtsFrontend::new(&xns).unwrap();
                    tts.tts_blocking(locales::t!("input.delete-tts", locales::LANG)).unwrap();
                }
                if payload.placeholder_persist && payload.placeholder.is_some() && payload.content.len() == 0
                {
                    // copy the placeholder into the content string before processing the backspace
                    payload.content.push_str(payload.placeholder.as_ref().unwrap());
                }
                if let Some(insertion_point) = payload.insertion_point {
                    if insertion_point > 0 {
                        assert!(
                            insertion_point < payload.content.len(),
                            "insertion point beyond content length!"
                        );
                        let new_len = payload.content.as_str().chars().count() - 1;

                        // have to use a temporary string because index operators are not implemented on Xous
                        // strings
                        let mut temp_str = String::new();
                        let mut original = payload.content.as_str().chars().enumerate().peekable();
                        // copy the data over, skipping the character that was deleted
                        loop {
                            if let Some((index, c)) = original.next() {
                                if index < insertion_point.saturating_sub(1) {
                                    temp_str.push(c);
                                } else {
                                    if let Some((_, next_char)) = original.peek() {
                                        temp_str.push(*next_char);
                                    } else {
                                        break;
                                    }
                                }
                            } else {
                                break;
                            }
                        }
                        // now copy the data back into the original string
                        let mut c_iter = temp_str.as_str().chars();
                        payload.content.clear();
                        for _ in 0..new_len {
                            payload.content.push(c_iter.next().unwrap());
                        }
                        // clear the temp string
                        // safety: we are going to turn the underlying pointer into NULL bytes, which is valid
                        // UTF-8
                        let temp_bytes = unsafe { temp_str.as_bytes_mut() };
                        let len = temp_bytes.len();
                        let ptr = temp_bytes.as_mut_ptr();
                        for i in 0..len {
                            // safety: the bounds were derived from a valid len; the pointer is aligned
                            // because it's derived from a slice. We use the
                            // "unsafe" version of this to force a zeroize
                            // of contents and avoid optimizations that could otherwise cause this operation
                            // to be skipped (e.g. dropping and de-allocating
                            // without scrubbing)
                            unsafe { ptr.add(i).write_volatile(0) };
                        }
                        // force all the writes to finish if they were re-ordered
                        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
                        // bring the insertion point back one index
                        payload.insertion_point = Some(insertion_point.saturating_sub(1));
                    }
                } else {
                    // coded in a conservative manner to avoid temporary allocations that can leave the
                    // plaintext on the stack
                    if payload.content.len() > 0 {
                        // don't backspace if we have no string.
                        let mut temp_str = String::from(payload.content.as_str());
                        let cur_len = temp_str.as_str().chars().count();
                        let mut c_iter = temp_str.as_str().chars();
                        payload.content.clear();
                        for _ in 0..cur_len - 1 {
                            payload.content.push(c_iter.next().unwrap());
                        }
                        let temp_bytes = unsafe { temp_str.as_bytes_mut() };
                        let len = temp_bytes.len();
                        let ptr = temp_bytes.as_mut_ptr();
                        for i in 0..len {
                            // safety: the bounds were derived from a valid len; the pointer is aligned
                            // because it's derived from a slice. We use the
                            // "unsafe" version of this to force a zeroize
                            // of contents and avoid optimizations that could otherwise cause this operation
                            // to be skipped (e.g. dropping and de-allocating
                            // without scrubbing)
                            unsafe { ptr.add(i).write_volatile(0) };
                        }
                        // force all the writes to finish if they were re-ordered
                        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
                    }
                }
            }
            _ => {
                // text entry
                if !self.keys_hit[self.selected_field as usize]
                    && payload.placeholder_persist
                    && payload.placeholder.is_some()
                    && payload.content.len() == 0
                {
                    // copy the placeholder into the content string before processing the backspace
                    payload.content.push_str(payload.placeholder.as_ref().unwrap());
                }
                self.keys_hit[self.selected_field as usize] = true;
                #[cfg(feature = "tts")]
                {
                    let xns = xous_names::XousNames::new().unwrap();
                    let tts = tts_frontend::TtsFrontend::new(&xns).unwrap();
                    tts.tts_blocking(&k.to_string()).unwrap();
                }
                match k {
                    '\u{f701}' | '\u{f700}' => (),
                    _ => {
                        if let Some(insertion_point) = payload.insertion_point {
                            if insertion_point >= payload.content.len() {
                                payload.content.push(k);
                                payload.insertion_point = None;
                            } else {
                                // have to use a temporary string because index operators are not implemented
                                // on Xous strings
                                let mut temp_str = String::from(payload.content.as_str());
                                let cur_len = temp_str.as_str().chars().count();
                                let mut c_iter = temp_str.as_str().chars();
                                payload.content.clear();
                                for i in 0..cur_len {
                                    if i == insertion_point {
                                        payload.content.push(k); // don't panic if we type too much, just silently drop the character
                                    }
                                    payload.content.push(c_iter.next().unwrap());
                                }
                                payload.insertion_point = Some(insertion_point + 1);
                                let temp_bytes = unsafe { temp_str.as_bytes_mut() };
                                let len = temp_bytes.len();
                                let ptr = temp_bytes.as_mut_ptr();
                                for i in 0..len {
                                    // safety: the bounds were derived from a valid len; the pointer is
                                    // aligned because it's derived from a
                                    // slice. We use the "unsafe" version
                                    // of this to force a zeroize
                                    // of contents and avoid optimizations that could otherwise cause this
                                    // operation to be skipped (e.g.
                                    // dropping and de-allocating
                                    // without scrubbing)
                                    unsafe { ptr.add(i).write_volatile(0) };
                                }
                                // force all the writes to finish if they were re-ordered
                                core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
                            }
                        } else {
                            payload.content.push(k); // don't panic if we type too much, just silently drop the character
                        }
                        log::trace!("****update payload: {}", payload.content);
                        payload.dirty = true;
                    }
                }
            }
        }
        None
    }
}
