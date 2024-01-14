use core::cell::Cell;
use core::fmt::Write;
use std::string::String;

use glyphstyle::GlyphStyle;
use graphics_server::api::*;
use locales::t;
use xous_ipc::Buffer;

use crate::*;

pub struct Bip39Entry {
    pub is_password: bool,
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub accepted_words: Vec<String>,
    pub user_input: String,
    pub payload: Option<Vec<u8>>,
    pub suggested_words: Vec<String>,
    suggestion_index: Cell<i16>,
    line_height: Cell<i16>,
    margin: Cell<i16>,
    gam: crate::Gam,
}

impl Default for Bip39Entry {
    fn default() -> Self {
        // this is slightly horriffic but we keep the bip39 phrase book inside the GAM and not
        // in this client-side library to avoid having 24kiB of phrases being compiled into every
        // process that potentially handles graphics. So, this modal needs a dedicated GAM connection
        // to handle bip39 processing on key entry. :-/
        let xns = xous_names::XousNames::new().unwrap();
        let gam = crate::Gam::new(&xns).unwrap();

        Self {
            is_password: Default::default(),
            action_conn: Default::default(),
            action_opcode: Default::default(),
            accepted_words: Vec::new(),
            user_input: String::new(),
            payload: None,
            suggested_words: Vec::new(),
            suggestion_index: Cell::new(0),
            line_height: Cell::new(0),
            margin: Cell::new(0),
            gam,
        }
    }
}

impl Bip39Entry {
    pub fn new(is_password: bool, action_conn: xous::CID, action_opcode: u32) -> Self {
        Self { is_password, action_conn, action_opcode, ..Default::default() }
    }
}

const NUM_RECCOS: i16 = 5;
const AESTHETIC_GAP: i16 = 5;
const STYLE_OVERRIDE: GlyphStyle = GlyphStyle::Bold;
const ACCEPTED_WORD_LINES: i16 = 5; // all 0's key requires 5 lines (abandon abandon abandon....art)
const STATUS_LINES: i16 = 3;

impl ActionApi for Bip39Entry {
    fn set_action_opcode(&mut self, op: u32) { self.action_opcode = op }

    fn is_password(&self) -> bool { self.is_password }

    /// The total canvas height is computed with this API call
    /// The canvas height is not dynamically adjustable for modals.
    fn height(&self, glyph_height: i16, margin: i16, _modal: &Modal) -> i16 {
        /*

              cat                   <-- active entry 1 line + 2x margin
            -------------------
            * cat                   <-- suggestion list (up to 5 lines, blank space if no suggestions)
              catalog                   "INVALID WORD" if active entry line has no matches
              catch                     "Start typing..." if line is blank and BIP39 invalid
              category                  "Presse enter to accept or keep typing..." if line is blank and BIP39 is valid

            -------------------
              Invalid phrase        <-- whether or not the words are valid (1 line)
              0x1234                <-- binary data preview (if word is valid, blank if not valid) (1 line)
            -------------------
              eager dish museum     <-- words accepted so far (6 lines reserved)
              pole aisle ...

            up/down arrow: selects which suggestion. auto-inserts suggested text into active entry line
            enter/center button: selects the suggestion. can only select valid suggestions
            backspace while text is present: remove one letter
            backspace while no text is present: remove latest word in bip39 list

            24 word example:
            oven color flag rich custom
            crawl century oak decad
            dilemma evolve company
            original arctic cat clever
            truth air chuckle radar
            polar silly soda idle
        */
        // the glyph_height is an opaque value because the final value depends upon a lookup table
        // stored in the graphics_server crate. To obtain this would require a handle to that server
        // which is private to the GAM. Thus we receive a copy of this from our caller and stash it
        // here for future reference. `glyph_height` can change depending upon the locale; in particular,
        // CJK languages have taller glyphs.
        self.line_height.set(glyph_to_height_hint(STYLE_OVERRIDE) as i16 + 2);
        self.margin.set(6);

        // compute the overall_height of the entry fields
        let overall_height =
            glyph_height                 // active entry
            + 2*margin                   // divider line
            + glyph_height * NUM_RECCOS  // recco list
            + 2*margin                   // divider line
            + glyph_height * STATUS_LINES // status line
            + 2*margin                   // divider line
            + glyph_height * ACCEPTED_WORD_LINES // words accepted so far
            + margin * 2                 // top/bottom spacing
        ;

        overall_height
    }

    fn redraw(&self, at_height: i16, modal: &Modal) {
        let color = if self.is_password { PixelColor::Light } else { PixelColor::Dark };

        let mut current_height = at_height;
        let bullet_margin = 17;

        // ------ draw the current text entry -------
        let mut tv = TextView::new(
            modal.canvas,
            TextBounds::BoundingBox(Rectangle::new(
                Point::new(self.margin.get(), current_height),
                Point::new(
                    modal.canvas_width - self.margin.get(),
                    current_height + self.line_height.get() + self.margin.get(),
                ),
            )),
        );
        tv.text.clear();
        tv.bounds_computed = None;
        tv.draw_border = false;
        tv.invert = self.is_password;
        tv.style = STYLE_OVERRIDE;
        write!(tv, "{}", self.user_input).ok();
        modal.gam.post_textview(&mut tv).expect("couldn't post tv");

        current_height += self.line_height.get();
        // ------- draw the line under the text entry -----
        modal.gam.draw_line(modal.canvas, Line::new_with_style(
            Point::new(self.margin.get(), current_height + AESTHETIC_GAP), // we manually place the line to an aesthetic position
            Point::new(modal.canvas_width - self.margin.get(), current_height + AESTHETIC_GAP),
            DrawStyle::new(color, color, 1))
            ).expect("couldn't draw entry line");

        current_height += self.margin.get() * 2;
        // ------- draw the suggestion list --------
        let mut draw_at = current_height; // split current_height into a separate variable for this op, because it's a variable length list
        if self.suggested_words.len() > 0 {
            for (index, recco) in self.suggested_words.iter().enumerate() {
                if index >= NUM_RECCOS as usize {
                    break;
                }
                if index as i16 == self.suggestion_index.get() {
                    // draw the dot
                    let mut tv = TextView::new(
                        modal.canvas,
                        TextBounds::BoundingBox(Rectangle::new(
                            Point::new(self.margin.get() + AESTHETIC_GAP, draw_at - 4),
                            Point::new(
                                modal.canvas_width - self.margin.get(),
                                draw_at + self.line_height.get() - 4,
                            ),
                        )),
                    );
                    tv.text.clear();
                    tv.bounds_computed = None;
                    tv.draw_border = false;
                    tv.invert = self.is_password;
                    write!(tv, "•").unwrap(); // emoji glyph will be summoned in this case
                    modal.gam.post_textview(&mut tv).expect("couldn't post tv");
                }
                let left_text_margin = self.margin.get() + AESTHETIC_GAP + bullet_margin; // space for the bullet point on the left

                // draw the reccommendation text
                let mut tv = TextView::new(
                    modal.canvas,
                    TextBounds::BoundingBox(Rectangle::new(
                        Point::new(left_text_margin, draw_at),
                        Point::new(
                            modal.canvas_width - (self.margin.get() + bullet_margin),
                            draw_at + self.line_height.get(),
                        ),
                    )),
                );
                tv.ellipsis = true;
                tv.invert = self.is_password;
                tv.text.clear();
                tv.draw_border = false;
                tv.margin = Point::new(0, 0);
                write!(tv, "{}", recco).ok();
                modal.gam.post_textview(&mut tv).expect("couldn't post tv");

                draw_at += self.line_height.get();
            }
        } else {
            // no suggested words:
            // put in some error/guidance text instead of a recco list
            let mut guidance = String::new();
            if self.user_input.len() > 0 {
                guidance.push_str(t!("bip39.invalid_word", locales::LANG));
            } else {
                // no input yet, give some options
                if self.payload.is_some() {
                    guidance.push_str(t!("bip39.enter_to_complete", locales::LANG));
                } else {
                    guidance.push_str(t!("bip39.start_typing", locales::LANG));
                }
            }
            let mut tv = TextView::new(
                modal.canvas,
                TextBounds::CenteredTop(Rectangle::new(
                    Point::new(self.margin.get(), draw_at),
                    Point::new(
                        modal.canvas_width - self.margin.get(),
                        draw_at + self.line_height.get() * NUM_RECCOS,
                    ),
                )),
            );
            tv.invert = self.is_password;
            tv.text.clear();
            tv.draw_border = false;
            write!(tv, "{}", guidance).ok();
            modal.gam.post_textview(&mut tv).expect("couldn't post tv");
        }

        current_height += self.line_height.get() * NUM_RECCOS;
        // -------- draw a divider line ----------
        modal
            .gam
            .draw_line(
                modal.canvas,
                Line::new_with_style(
                    Point::new(self.margin.get(), current_height + self.margin.get()),
                    Point::new(modal.canvas_width - self.margin.get(), current_height + self.margin.get()),
                    DrawStyle::new(color, color, 1),
                ),
            )
            .expect("couldn't draw entry line");

        current_height += self.margin.get() * 2;
        // ------- draw word list -------
        let mut words = String::new();
        for word in self.accepted_words.iter() {
            words.push_str(word);
            words.push_str(" ");
        }
        let mut tv = TextView::new(
            modal.canvas,
            TextBounds::CenteredTop(Rectangle::new(
                Point::new(self.margin.get(), current_height),
                Point::new(
                    modal.canvas_width - self.margin.get(),
                    current_height + self.line_height.get() * ACCEPTED_WORD_LINES + self.margin.get(),
                ),
            )),
        );
        tv.invert = self.is_password;
        tv.ellipsis = true;
        tv.text.clear();
        tv.draw_border = false;
        tv.style = STYLE_OVERRIDE;
        write!(tv, "{}", words).ok();
        modal.gam.post_textview(&mut tv).expect("couldn't post tv");

        current_height += self.line_height.get() * ACCEPTED_WORD_LINES;
        // -------- draw a divider line ----------
        modal
            .gam
            .draw_line(
                modal.canvas,
                Line::new_with_style(
                    Point::new(self.margin.get(), current_height + self.margin.get()),
                    Point::new(modal.canvas_width - self.margin.get(), current_height + self.margin.get()),
                    DrawStyle::new(color, color, 1),
                ),
            )
            .expect("couldn't draw entry line");

        current_height += self.margin.get() * 2;
        // ------- status --------
        let mut status = String::new();
        if let Some(p) = &self.payload {
            status.push_str(t!("bip39.valid_phrase", locales::LANG));
            status.push_str(" ");
            status.push_str(&hex::encode(&p));
        } else {
            if self.user_input.len() == 0 && self.accepted_words.len() == 0 {
                status.push_str(t!("bip39.waiting", locales::LANG));
            } else {
                status.push_str(t!("bip39.invalid_phrase", locales::LANG));
                status.push_str("\n");
                status.push_str(t!("bip39.abort_help", locales::LANG));
            }
        }
        let mut tv = TextView::new(
            modal.canvas,
            TextBounds::CenteredTop(Rectangle::new(
                Point::new(self.margin.get(), current_height),
                Point::new(
                    modal.canvas_width - self.margin.get(),
                    current_height + self.line_height.get() * STATUS_LINES + self.margin.get(),
                ),
            )),
        );
        tv.invert = self.is_password;
        tv.text.clear();
        tv.draw_border = false;
        tv.ellipsis = true;
        write!(tv, "{}", status).ok();
        modal.gam.post_textview(&mut tv).expect("couldn't post tv");
    }

    fn key_action(&mut self, k: char) -> Option<ValidatorErr> {
        let can_move_downwards = !(self.suggestion_index.get() + 1 == NUM_RECCOS);
        let can_move_upwards = !(self.suggestion_index.get() - 1 < 0);

        // log::debug!("key_action: {}", k);
        match k {
            '∴' | '\u{d}' => {
                if self.user_input.len() == 0 {
                    if let Some(data) = &self.payload {
                        let mut ret = Bip39EntryPayload::default();
                        ret.data[..data.len()].copy_from_slice(&data);
                        ret.len = data.len() as u32;

                        // relinquish focus before returning the result
                        self.gam.relinquish_focus().unwrap();
                        xous::yield_slice();

                        let buf = Buffer::into_buf(ret).expect("couldn't convert message to payload");
                        buf.send(self.action_conn, self.action_opcode)
                            .map(|_| ())
                            .expect("couldn't send action message");
                    }
                } else {
                    if self.suggested_words.len() > 0 {
                        if (self.suggestion_index.get() as usize) < self.suggested_words.len() {
                            // should "always" be true, but good to check
                            self.accepted_words
                                .push(self.suggested_words[self.suggestion_index.get() as usize].to_string());
                            match self.gam.bip39_to_bytes(&self.accepted_words) {
                                Ok(data) => self.payload = Some(data),
                                _ => self.payload = None,
                            }
                            self.user_input.clear();
                            self.suggested_words.clear();
                            self.suggestion_index.set(0);
                        }
                    }
                }
            }
            '↑' => {
                if can_move_upwards {
                    self.suggestion_index.set(self.suggestion_index.get() - 1);
                }
            }
            '↓' => {
                if can_move_downwards {
                    self.suggestion_index.set(self.suggestion_index.get() + 1);
                }
            }
            '\u{0}' => {
                // ignore null messages
            }
            '\u{14}' => {
                // F4
                // relinquish focus before returning the result
                self.gam.relinquish_focus().unwrap();
                xous::yield_slice();

                let ret = Bip39EntryPayload::default(); // return a 0-length entry
                let buf = Buffer::into_buf(ret).expect("couldn't convert message to payload");
                buf.send(self.action_conn, self.action_opcode)
                    .map(|_| ())
                    .expect("couldn't send action message");
                return None;
            }
            '\u{8}' => {
                // backspace
                #[cfg(feature = "tts")]
                {
                    let xns = xous_names::XousNames::new().unwrap();
                    let tts = tts_frontend::TtsFrontend::new(&xns).unwrap();
                    tts.tts_blocking(locales::t!("input.delete-tts", locales::LANG)).unwrap();
                }
                if self.user_input.len() > 0 {
                    // don't backspace if we have no string.
                    self.user_input.pop();
                    if self.user_input.len() > 0 {
                        self.suggested_words =
                            self.gam.bip39_suggestions(&self.user_input).unwrap_or(Vec::<String>::new());
                    } else {
                        self.suggested_words.clear();
                    }
                    self.suggestion_index.set(0);
                } else {
                    self.accepted_words.pop();
                    self.suggested_words.clear();
                    self.suggestion_index.set(0);
                    match self.gam.bip39_to_bytes(&self.accepted_words) {
                        Ok(data) => self.payload = Some(data),
                        _ => self.payload = None,
                    }
                }
            }
            _ => {
                // text entry
                #[cfg(feature = "tts")]
                {
                    let xns = xous_names::XousNames::new().unwrap();
                    let tts = tts_frontend::TtsFrontend::new(&xns).unwrap();
                    tts.tts_blocking(&k.to_string()).unwrap();
                }
                if k.is_ascii_alphabetic() {
                    // ignore any other input, since it's invalid.
                    let lk = k.to_lowercase();
                    for c in lk {
                        self.user_input.push(c);
                    }
                    // now regenerate all the hints
                    self.suggested_words =
                        self.gam.bip39_suggestions(&self.user_input).unwrap_or(Vec::<String>::new());
                    self.suggestion_index.set(0);
                }
            }
        }
        None
    }
}
