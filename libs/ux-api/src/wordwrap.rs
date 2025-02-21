/// Wordwrap stratgey
///
/// Strings are submitted to the Wordwrapper, and they are split into lines, and then into lexical words.
///
/// The rule for line splitting is simple: '\n' denotes a new line.
/// The rule for word splitting is done according to Rust's built-in "split_whitespace()" function.
///
/// Once split into words, each word is turned into a `TypesetWord` structure, which is a series of
/// GlyphSprites (e.g. references to bitmap font data), wrapped in a bounding box `bb` that denotes
/// exactly where it would be rendered in absolute coordinates on a canvas as defined by a `max` clipping
/// rectangle (which is the do-not-exceed area based on absolute screen coordinates) and a starting point
/// defined by a `bounds` record.
///
/// The exact GlyphSprite chosen is picked based on a hierarchy that starts with a hint based on
/// `locales::LANG`, then rules based on the `base_style: GlyphStyle` field, which allows for all the
/// text within a given string to be eg. small, regular, monospace, bold (mixing of different styles is
/// not yet supported, but could be in the future if we add some sort of markup parsing to the text
/// stream).
///
/// The location of the GlyphSprites do a "Best effort" to fit the words within the `bounds` based on the
/// designated rule without word-wrapping. If a single word overflows one line width, it will be broken
/// into two words at the closest boundary that does not overflow the text box, otherwise, it will be
/// moved to a new line on its own (e.g. no hyphenation rules are applied).
///
/// If the overall string cannot fit within the absolute bounds defined by the `max` area and/or the
/// `bounds`, the rendering is halted, and ellipses are inserted at the end.
use blitstr2::*;

use crate::minigfx::*;

/// A TypesetWord is a Word that has beet turned into sprites and placed at a specific location on the canvas,
/// defined by its `bb` record. The intention is that this abstract representation can be passed directly to
/// a rasterizer for rendering.
#[derive(Debug)]
pub struct TypesetWord {
    /// glyph data to directly render the word
    pub gs: Vec<GlyphSprite>,
    /// top left origin point for rendering of the glyphs
    pub origin: Point,
    /// width of the word
    pub width: isize,
    /// overall height for the word
    pub height: isize,
    /// set if this `word` is not drawable, e.g. a newline placeholder.
    /// *however* the Vec<GlyphSprite> should still be checked for an insertion point, so that
    /// successive newlines properly get their insertion point drawn
    pub non_drawable: bool,
    /// the position in the originating abstract string of the first character in the word
    pub strpos: usize,
}

impl TypesetWord {
    pub fn new(origin: Point, strpos: usize) -> Self {
        TypesetWord {
            gs: Vec::<GlyphSprite>::new(),
            origin,
            width: 0,
            height: 0,
            non_drawable: false,
            strpos,
        }
    }

    pub fn one_glyph(origin: Point, gs: GlyphSprite, strpos: usize) -> Self {
        TypesetWord {
            gs: vec![gs],
            origin,
            width: gs.wide as isize,
            height: gs.high as isize,
            non_drawable: false,
            strpos,
        }
    }

    pub fn push(&mut self, gs: GlyphSprite) {
        self.width += (gs.wide + gs.kern) as isize;
        self.height = self.height.max(gs.high as isize);
        self.gs.push(gs);
    }

    /// if the pop is invalid, we'll return an invalid character. Just...don't do that. k?
    pub fn pop(&mut self) -> GlyphSprite {
        let gs = self.gs.pop().unwrap_or(NULL_GLYPH_SPRITE);
        self.width -= (gs.wide + gs.kern) as isize;
        // we can't undo any height transformations, unfortunately, because we don't know what the previous
        // state was but it's fairly minor if text is set funny on a line because text overflowed and
        // had e.g. emoji buried amongst small font text...
        gs
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum OverflowStrategy {
    /// overflow text is truncated and replaced with an ellipsis
    Ellipsis,
    /// stop rendering at overflow
    Abort,
    /// render exactly one line of text; reset the renderer for a new line at the top left of the max bb area
    /// This will yield only whole words if the next word could fit in a single line; otherwise, it will
    /// split a longer-than-one-line word unceremoniously into multiple lines
    ///
    /// This is intended for implementing e.g. scrollable text, where a text area is split
    /// into multiple lines of typeset text, and then we can selectively render a range of text at a
    /// given offset to simulate scrolling.
    ///
    /// The function hasn't been tested at all, however.
    #[allow(dead_code)]
    // it's true, we haven't tested this mode yet. Remove this once we have tested it.
    OneLineIterator,
}

/// ComposedType is text that has been laid out and wrapped, along with metadata about the bounds
/// of the final composition. ComposedType coordinates are always in screen-space.
pub struct ComposedType {
    words: Vec<TypesetWord>,
    bounding_box: ClipRect,
    cursor: Cursor,
    overflow: bool,
}
impl ComposedType {
    pub fn new(words: Vec<TypesetWord>, bounds: ClipRect, cursor: Cursor, overflow: bool) -> Self {
        ComposedType { words, bounding_box: bounds, cursor, overflow }
    }

    pub fn bb_width(&self) -> isize { self.bounding_box.max.x - self.bounding_box.min.x }

    pub fn bb_height(&self) -> isize { self.bounding_box.max.y - self.bounding_box.min.y }

    /// Note: it is up to the caller to ensure that clip_rect is within the renderable screen area. We do no
    /// additional checks around this.
    pub fn render(&self, frbuf: &mut [u32], offset: Point, invert: bool, clip_rect: Rectangle) {
        const MAX_GLYPH_MARGIN: isize = 16;
        // let mut strpos; // just for debugging insertion points
        for word in self.words.iter() {
            // strpos = word.strpos;
            let mut point = word.origin.clone();
            for glyph in word.gs.iter() {
                // strpos += 1;
                // the offset can actually be negative for good reasons, e.g., we're doing a scrollable
                // buffer, but the blitstr2 was written assuming only positive offsets. Handle
                // this here.
                let maybe_x = offset.x + point.x as isize;
                let maybe_y = offset.y + point.y as isize;
                let mut renderable = true;
                // allow MAX_GLYPH_MARGIN so we can get partial rendering of text that's slightly off screen
                if maybe_x < (clip_rect.tl().x - MAX_GLYPH_MARGIN) || maybe_x > clip_rect.br().x {
                    log::trace!("not renderable maybe_x: {}, {:?}", maybe_x, clip_rect);
                    renderable = false;
                }
                if maybe_y < (clip_rect.tl().y - MAX_GLYPH_MARGIN) || maybe_y > clip_rect.br().y {
                    log::trace!("not renderable maybe_y: {}, {:?}", maybe_y, clip_rect);
                    renderable = false;
                }
                point.x += (glyph.wide + glyph.kern) as isize; // keep scorekeeping on this, because it could eventually become renderable
                if !renderable {
                    // quickly short circuit over any text that is definitely outside of our clipping
                    // rectangle
                    continue;
                } else {
                    let cr =
                        ClipRect::new(clip_rect.tl().x, clip_rect.tl().y, clip_rect.br().x, clip_rect.br().y);
                    if glyph.large {
                        blitstr2::xor_glyph_large(
                            frbuf,
                            (maybe_x, maybe_y),
                            *glyph,
                            glyph.invert ^ invert,
                            cr,
                        );
                    } else if !glyph.double {
                        blitstr2::xor_glyph(frbuf, (maybe_x, maybe_y), *glyph, glyph.invert ^ invert, cr);
                    } else {
                        blitstr2::xor_glyph_2x(frbuf, (maybe_x, maybe_y), *glyph, glyph.invert ^ invert, cr);
                    }
                    if glyph.insert {
                        // log::info!("insert at {},{}", glyph.ch, strpos - 1);
                        // draw the insertion point after the glyph's position
                        crate::minigfx::op::line(
                            frbuf,
                            Line::new(
                                Point::new(maybe_x as isize - 1, maybe_y as _),
                                Point::new(maybe_x as isize - 1, maybe_y as isize + glyph.high as isize),
                            ),
                            Some(clip_rect),
                            invert,
                        );
                    }
                }
            }
        }
    }

    pub fn final_cursor(&self) -> Cursor { self.cursor }

    pub fn final_overflow(&self) -> bool { self.overflow }
}
/// Typesetter takes a string and attempts to lay it out within a region defined by
/// a single point known as the "Extent". This is the maximum extent allowable for
/// the type.
///
/// The string may overflow the provided Extent. The strategy for handling the overflow
/// is specified with the `OverflowStrategy`, specified at the time of the typesetting call.
///
/// The Typesetter can run in essentially two modes: Line-by-line, or Fill-to-overflow. The
/// terminus of Fill-to-overflow can be either ellipses at the end, or a simple truncation.
///
/// When the Typesetter ends, the Typesetter will leave off in a way that Typesetting can
/// resume according to the specified mode. For Line-by-Line, it will resume Typesetting at
/// the top left of the bounding box. For other modes, it will resume at the point of overflow;
/// one must "rewind" the Typesetter, before calling it again.
///
/// The Typesetter returns a `ComposedType` object which is a `Vec` of `TypesetWord` along
/// with a helper field that summarizes the actual bounding box of the text, along with the
/// final cursor position.
///
/// An insertion point cursor will be injected into the TypesetWord stream at the character offset in
/// the input `string` if it is specified as `Some(usize)`.
pub struct Typesetter {
    charpos: usize,
    cursor: Cursor, /* indicates the current insertion point for a candidate. it is not updated as the
                     * candidates are formed. */
    candidate: TypesetWord,
    bb: ClipRect,
    space: GlyphSprite,
    ellipsis: GlyphSprite,
    large_space: GlyphSprite,
    insertion_point: Option<usize>,
    s: String,
    base_style: GlyphStyle,
    overflow: bool,
    max_width: isize,
    last_line_height: usize, // scorecarding for the very last line on the loop exit
}
impl Typesetter {
    pub fn setup(s: &str, extent: &Point, base_style: &GlyphStyle, insertion_point: Option<usize>) -> Self {
        let bb = ClipRect::new(0, 0, extent.x, extent.y);
        let mut space = style_glyph(' ', base_style);
        space.kern = 0;
        let mut ellipsis = style_glyph('…', base_style);
        ellipsis.kern = 0;

        #[cfg(not(feature = "cramium-soc"))]
        let mut large_space = style_glyph(' ', &GlyphStyle::Cjk);
        #[cfg(feature = "cramium-soc")]
        let mut large_space = style_glyph(' ', &GlyphStyle::Tall);

        if cfg!(feature = "cramium-soc") {
            large_space.wide = glyph_to_height_hint(GlyphStyle::Tall) as u8;
        } else {
            large_space.wide = glyph_to_height_hint(GlyphStyle::Cjk) as u8;
        }

        Typesetter {
            charpos: 0,
            cursor: Cursor::new(0, 0, 0),
            candidate: TypesetWord::new(Point::new(0, 0), 0), /* first word candidate starts at the top
                                                               * left
                                                               * corner */
            bb,
            space,
            ellipsis,
            large_space,
            base_style: base_style.clone(),
            s: String::from(s),
            insertion_point,
            overflow: false,
            max_width: 0,
            last_line_height: 0,
        }
    }

    /// Wrap the words in the string until the space overflows, leaving ellipsis at the end.
    /// Any prior result in self.words is overwritten.
    ///
    /// For overflow strategies that are not OneLineIterator,
    /// the caller must reset the cursor to the desired resume position, otherwise it will pick
    /// up again at the overflow position.
    ///
    /// The final Vec::<TypesetWord> is snapped to the top left of the Renderable region. This
    /// needs to be transformed into the final screen coordinate space before blitting.
    pub fn typeset(&mut self, strat: OverflowStrategy) -> ComposedType {
        // a composition only lasts as long as the lifetime of this call, and is passed back to the caller
        // at the end. Thus, we create it here and pass it out in the end. It's a bad idea to make it
        // part of the core `struct` because then you'd have to wrap it in an Option to deal with the object
        // going out of scope at the end of the call.
        let mut composition = Vec::<TypesetWord>::new();

        if self.bb.max.x - self.bb.min.x < glyph_to_height_hint(GlyphStyle::Regular) as isize {
            // we flag this because the typesetter algorithm may never converge if it can't set any characters
            // because the region is just too narrow.
            log::error!("Words cannot be typset because the width of the typset region is too narrow.");
            return ComposedType::new(
                composition,
                ClipRect::new(self.bb.min.x, self.bb.min.y, self.bb.min.x, self.bb.min.y),
                self.cursor,
                true,
            );
        }
        // algorithm:
        // - If not a whitespace or newline:
        //   1. append a character to the current word (candidate)
        //   2. Test if the new character would overflow. If so, move the word to a new line, or exit with
        //      overflow.
        // Thus upon entry to this loop, the `candidate` is always guaranteed to be a fitting word at its
        // current point of rendering.
        // - If a whitespace:
        //   1. put the `candidate` into the `words` field
        //   2. Test to see if we can fit a whitespace at the end of this line. If we can't, insert a newline,
        //      or overflow.
        // - If a newline:
        //   1. put the `candidate` into the `words` field
        //   2. Test to see if we can add a newline. If we can't, overflow.
        let working_string = self.s.to_string(); // allocate a full copy to avoid interior mutability issues in the loop below. :-/ ugh.
        // there's probably a more space-efficient way to deal with this using interior mutability but fuck
        // it, I need to get this code working.
        for ch in working_string.chars().skip(self.charpos) {
            // .skip() allows us to resume typesetting where we last left off
            if ch == '\n' {
                // handle the explicit newline case
                match strat {
                    OverflowStrategy::OneLineIterator => {
                        if self.candidate.gs.len() > 0 {
                            // this test is here in case we have multiple spaces or newlines in a row
                            self.commit_candidate_word(&mut composition);
                        }
                        self.oneline_epilogue();
                        break;
                    }
                    _ => {
                        if self.candidate.gs.len() > 0 {
                            // stash the word we just made; if this is 0, we're encounting multiple \n\n in a
                            // row
                            self.commit_candidate_word(&mut composition);
                        }
                        if self.is_newline_available() {
                            self.move_candidate_to_newline();
                            // hard newlines are marked by a non-drawable space at the beginning of a line
                            // this also allows us to place a cursor to "delete" a stray newline since it has
                            // the size and shape of a space
                            if !self.try_append_space(&mut composition) {
                                log::error!(
                                    "Internal error: cursor was set to a newline, yet no space for new characters??"
                                );
                            }
                        } else {
                            self.overflow_typesetting(&mut composition, strat);
                            break;
                        }
                    }
                }
            } else if ch.is_whitespace() && (ch != '\t') {
                if self.candidate.gs.len() > 0 {
                    // this test is here in case we have multiple spaces or newlines in a row
                    self.commit_candidate_word(&mut composition);
                }
                if !self.try_append_space(&mut composition) {
                    match strat {
                        OverflowStrategy::OneLineIterator => {
                            self.oneline_epilogue();
                            break;
                        }
                        _ => {
                            if self.is_newline_available() {
                                self.move_candidate_to_newline();
                                // the call below automatically handles the case of non-drawable spaces at the
                                // beginning of newlines
                                if !self.try_append_space(&mut composition) {
                                    log::error!(
                                        "Internal error: cursor was set to a newline, yet no space for new characters??"
                                    );
                                }
                            } else {
                                self.overflow_typesetting(&mut composition, strat);
                                break;
                            }
                        }
                    }
                }
            } else {
                // at this point, we get a glyph. This next glyph can cause one of the following situations to
                // occur:
                // 1. The evolving word fits.
                // 2. The evolving word is longer than a single line, and there are more lines available.
                // 3. The evolving word is longer than a single line, and there are no more lines available.
                // 4. The evolving word fits a line but doesn't fit this line, and there is space on a new
                //    line for it.
                // 5. The evolving word fits a line but doesn't fit this line, and there is no more space at
                //    all.
                let mut gs =
                    if ch != '\t' { style_glyph(ch, &self.base_style) } else { self.large_space.clone() };
                if self.is_insert_point() {
                    gs.insert = true;
                }
                self.candidate.push(gs.clone());
                if self.is_word_longer_than_line() {
                    // cases 2 & 3
                    match strat {
                        OverflowStrategy::OneLineIterator => {
                            // case 2 is not an option with the OneLine strategy
                            // this exit is weird. We need to commit the partially typeset word, and reset the
                            // rendering state. remove the character that caused
                            // the overflow.
                            let _gs_pop = self.candidate.pop();
                            self.commit_candidate_word(&mut composition);
                            // we have to rewind the character position, because the commit_candidate_word
                            // side effects that, and doesn't know about the
                            // removed character. This is sort of ugly.
                            self.charpos -= 1;
                            // now reset us for the next call
                            self.oneline_epilogue();
                            // typesetting will resume at the broken-off word, because the charpos was left
                            // there.
                            break;
                        }
                        _ => {
                            //      self.char_pos
                            //       v|v gs_pop
                            //  012345|678
                            //  abcdef|hij
                            if self.is_newline_available() {
                                // case 2
                                // commit the fragment of the word to the current line
                                let gs_pop = self.candidate.pop();
                                self.commit_candidate_word(&mut composition);
                                // set the cursor to the next line
                                self.move_candidate_to_newline();
                                // now set the overflowed character on the new line so our state is synched up
                                self.candidate.push(gs_pop);
                            } else {
                                // case 3
                                // similar to the one-line iterator exit, but with a call to overflow at the
                                // end.
                                let _gs_pop = self.candidate.pop();
                                self.commit_candidate_word(&mut composition);
                                self.charpos -= 1;
                                self.overflow_typesetting(&mut composition, strat);
                                break;
                            }
                        }
                    }
                } else if !self.does_word_fit_on_line() {
                    // this is case 4 & 5 -- candidate is shorter than a line, but doesn't fit on this line
                    match strat {
                        OverflowStrategy::OneLineIterator => {
                            // case 4 is not an option
                            // now exit
                            self.reject_candidate_word();
                            self.oneline_epilogue();
                            break;
                        }
                        _ => {
                            if self.is_newline_available() {
                                // case 4
                                self.move_candidate_to_newline();
                                self.commit_candidate_glyph(&gs);
                            } else {
                                // case 5
                                self.reject_candidate_word();
                                self.overflow_typesetting(&mut composition, strat);
                                break;
                            }
                        }
                    }
                } else {
                    // case 1 -- everything fits
                    self.commit_candidate_glyph(&gs);
                }
            }
        }
        if self.candidate.gs.len() > 0 {
            self.commit_candidate_word(&mut composition);
        }
        let ret = ComposedType::new(
            composition,
            ClipRect::new(
                self.bb.min.x,
                self.bb.min.y,
                self.max_width as isize,
                self.cursor.pt.y + self.last_line_height as isize,
            ),
            self.cursor,
            self.overflow,
        );
        // cleanup any state based on the overflow strategy
        match strat {
            OverflowStrategy::OneLineIterator => {
                self.max_width = 0; // it's a fresh line every time
                assert!(
                    self.cursor.pt.x == self.bb.min.x,
                    "internal logic did not clean up the cursor state for the one line iterator"
                )
            }
            _ => {
                // other states "start where they left off"
            }
        }
        ret
    }

    fn is_newline_available(&self) -> bool {
        // repeated, bare newlines will have a candidate height of 0, as it contains no glyphs. correct for
        // that.
        let corrected_height =
            if self.candidate.height == 0 { self.cursor.line_height as isize } else { self.candidate.height };
        log::trace!(
            "{} < {}",
            corrected_height + self.cursor.pt.y + self.cursor.line_height as isize,
            self.bb.max.y
        );
        corrected_height + self.cursor.pt.y + (self.cursor.line_height as isize) < self.bb.max.y
    }

    fn does_word_fit_on_line(&self) -> bool { self.candidate.width + self.cursor.pt.x < self.bb.max.x }

    fn is_word_longer_than_line(&self) -> bool { self.candidate.width >= (self.bb.max.x - self.bb.min.x) }

    fn is_insert_point(&self) -> bool {
        if let Some(ip) = self.insertion_point {
            if ip == self.charpos { true } else { false }
        } else {
            false
        }
    }

    fn commit_candidate_word(&mut self, composition: &mut Vec<TypesetWord>) {
        if !self.candidate.non_drawable {
            // this is mainly for "non-drawable spaces" at the beginning of a line
            self.max_width = self.max_width.max(self.candidate.width + self.candidate.origin.x);
            self.cursor.pt.x += self.candidate.width;
            self.cursor.line_height = self.cursor.line_height.max(self.candidate.height as usize);
            self.last_line_height = self.cursor.line_height;
        }
        if false {
            tsw_debug(&self.candidate)
        };
        composition.push(std::mem::replace(
            &mut self.candidate,
            // prepare a fresh "candidate" starting at the next character position
            TypesetWord::new(self.cursor.pt, self.charpos),
        ));
    }

    fn commit_candidate_glyph(&mut self, _gs: &GlyphSprite) { self.charpos += 1; }

    fn reject_candidate_word(&mut self) {
        // back the rendering up to the starting point of the current word under consideration
        self.charpos = self.candidate.strpos;
        self.cursor.pt = self.candidate.origin;
        self.cursor.line_height = 0;
        // discard the current word, since we're going to start over again on the next call
        self.candidate = TypesetWord::new(self.cursor.pt, self.charpos);
    }

    fn move_candidate_to_newline(&mut self) {
        // advance the rendering line, without inserting a newline placeholder
        self.last_line_height = self.cursor.line_height;
        self.cursor.pt.y += self.cursor.line_height as isize;
        self.cursor.pt.x = self.bb.min.x;
        self.cursor.line_height = self.candidate.height as usize;
        // now set the current candidate word's origin to the beginning of this new line
        self.candidate.origin = self.cursor.pt;
    }

    fn overflow_typesetting(&mut self, composition: &mut Vec<TypesetWord>, strat: OverflowStrategy) {
        self.overflow = true;
        match strat {
            OverflowStrategy::Ellipsis => {
                let mut ov_word =
                    TypesetWord::one_glyph(self.cursor.pt.clone(), self.ellipsis.clone(), self.charpos);
                if self.is_insert_point() {
                    ov_word.gs[0].insert = true;
                }
                composition.push(ov_word);
                // the ellipsis is meta, so when we resume rendering it won't exist. thus don't update the
                // cursor over it. self.cursor.update_glyph(&self.ellipsis);
            }
            _ => {
                () // do nothing
            }
        }
    }

    /// tries to add a space after the current cursor point. If this would overflow the bb bounds,
    /// it returns false.
    /// The rule is, we must always enter this with a "brand new" TypesetWord entry with the charpos
    /// set to our space point, because the caller will have already stashed the previously formed word
    fn try_append_space(&mut self, composition: &mut Vec<TypesetWord>) -> bool {
        assert!(self.candidate.gs.len() == 0, "self.candidate was not set to a new state prior to this call");
        if (self.cursor.pt.x + self.space.wide as isize) < self.bb.max.x {
            // our candidate word is "just as space"
            let mut candidate_space = self.space.clone();
            if self.is_insert_point() {
                candidate_space.insert = true;
            }
            self.candidate.push(candidate_space);
            self.commit_candidate_glyph(&candidate_space);
            self.cursor.line_height = self.cursor.line_height.max(self.space.high as usize);
            // if we're at the beginning of a line, mark the candidate word (that just contains a space) as
            // non-drawable
            if self.cursor.pt.x == self.bb.min.x {
                self.candidate.non_drawable = true;
            }
            // now commit it
            self.commit_candidate_word(composition);
            true
        } else {
            false
        }
    }

    /// resets the cursor state to the top left of the box for the next line to render.
    fn oneline_epilogue(&mut self) {
        self.cursor.pt.y = 0; // this should be redundant, as we never have more than one line in this mode
        self.cursor.pt.x = self.bb.min.x;
        if self.cursor.line_height == 0 {
            // in case we have successive newlines, just default to the "regular" height
            self.cursor.line_height = glyph_to_height_hint(GlyphStyle::Regular);
        }
    }
}

#[allow(dead_code)]
fn tsw_debug(tsw: &TypesetWord) {
    let mut s = String::new();
    for gs in tsw.gs.iter() {
        s.push(gs.ch);
    }
    log::info!("{} @ {},{}+{}={}", &s, tsw.origin.x, tsw.origin.y, tsw.height, tsw.origin.y + tsw.height);
}

/// Find glyph for char using latin regular, emoji, ja, zh, and kr font data
pub fn style_glyph(ch: char, base_style: &GlyphStyle) -> GlyphSprite {
    match locales::LANG {
        "zh" => {
            style_wrapper!(zh_rules, base_style, ch)
        }
        "jp" => {
            style_wrapper!(jp_rules, base_style, ch)
        }
        "kr" => {
            style_wrapper!(kr_rules, base_style, ch)
        }
        "en-tts" => {
            style_wrapper!(en_audio_rules, base_style, ch)
        }
        // default to English rules
        _ => {
            style_wrapper!(english_rules, base_style, ch)
        }
    }
}
