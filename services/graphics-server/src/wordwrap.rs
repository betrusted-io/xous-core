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
/// `xous::LANG`, then rules based on the `base_style: GlyphStyle` field, which allows for all the text within
/// a given string to be eg. small, regular, monospace, bold (mixing of different styles is not yet supported,
/// but could be in the future if we add some sort of markup parsing to the text stream).
///
/// The location of the GlyphSprites do a "Best effort" to fit the words within the `bounds` based on the
/// designated rule without word-wrapping. If a single word overflows one line width, it will be broken
/// into two words at the closest boundary that does not overflow the text box, otherwise, it will be moved
/// to a new line on its own (e.g. no hyphenation rules are applied).
///
/// If the overall string cannot fit within the absolute bounds defined by the `max` area and/or the `bounds`,
/// the rendering is halted, and ellipses are inserted at the end.

use crate::blitstr2::*;
use crate::style_macros::*;

/// A TypesetWord is a Word that has beet turned into sprites and placed at a specific location on the canvas,
/// defined by its `bb` record. The intention is that this abstract representation can be passed directly to
/// a rasterizer for rendering.
pub struct TypesetWord {
    pub gs: Vec::<GlyphSprite>,
    pub origin: Pt,
    pub width: usize,
    pub height: usize,
}
impl TypesetWord {
    pub fn new(origin: Pt) -> Self {
        TypesetWord {
            gs: Vec::<GlyphSprite>::new(),
            origin,
            width: 0,
            height: 0,
        }
    }
    pub fn one_glyph(origin: Pt, gs: GlyphSprite) -> Self {
        TypesetWord {
            gs: vec![gs],
            origin,
            width: gs.wide as usize,
            height: gs.high as usize,
        }
    }
    pub fn push(&mut self, gs: GlyphSprite) {
        self.width += gs.wide as usize;
        self.height = self.height.max(gs.high as usize);
    }
    /// offset has to take an explicit x/y set because Pt is defined as a usize, so we can't do negative offsets
    pub fn offset(&mut self, x: i16, y: i16) {
        let ox = self.origin.x as i16 + x;
        let oy = self.origin.y as i16 + y;
        self.origin.x = if ox > 0 { ox as usize } else { 0 };
        self.origin.y = if oy > 0 { oy as usize } else { 0 };
    }
}

use crate::api::TextBounds;
//use graphics_server::TextBounds;
/// The TypesetWords in this case are defined by a bounding box into which they can
/// be directly rendered.
///
/// All coordinates must be specified in terms of absolute screen coordinates.
/// The resulting `TypesetWord` vector is in absolute screen coordinates.
///
/// `border` defines the maximum space available for rending the growable types. It
/// must always be able to contain any elements specified with the `bounds` requirement
/// specified in TextBounds. If the border is smaller than the bounds requirement,
/// then unpredictable results will occur. There is (currently) no error checking for this condition.
///
/// Upon return, `border` is shrunk to the actual amount of space used by the renderer, if and only
/// if the `bounds` spec is not `BoundingBox`.
///
/// An artifact of this rendering is that all whitespace is stripped and replaced with
/// a single "space" character. Multiple whitespace will have to be a special case
/// down the road, probably the domain of a "mono" glyph style, where no word wrapping
/// is done, and just clipping.
pub fn fit_str_to_clip(
    s: &str,
    border: &mut ClipRect,
    bounds: &TextBounds,
    base_style: &GlyphStyle,
    cursor_state: &mut graphics_server::Cursor,
    overflow: &mut bool
) -> Vec::<TypesetWord> {
    *overflow = false;
    let space = style_glyph(' ', base_style);
    let ellipsis = find_glyph_latin_small('…');

    // compute the renderable extent of the text region. This is the largest region allowed by the TextBounds specifier.
    // we'll shrink this down after we're done rendering
    let renderable = match *bounds {
        TextBounds::BoundingBox(r) => {
            ClipRect::new(r.tr().x as _, r.tr().y as _, r.bl().x as _, r.bl().y as _)
        }
        TextBounds::GrowableFromBr(br, width) => {
            let min_x = if br.x - (width as i16) < 0 { 0usize } else { (br.x - width as i16) as usize };
            let min_y = border.min.y;
            let max_x = br.x as usize;
            let max_y = br.y as usize;
            ClipRect::new(min_x as _, min_y as _, max_x as _, max_y as _)
        }
        TextBounds::GrowableFromTl(tl, width) => {
            let min_x = tl.x as usize;
            let min_y = tl.y;
            let max_x = if (tl.x + width as i16) as usize > border.max.x { border.max.x } else { (tl.x + width as i16) as usize };
            let max_y = border.max.y;
            ClipRect::new(min_x as _, min_y as _, max_x as _, max_y as _)
        }
        TextBounds::GrowableFromBl(bl, width) => {
            let min_x = bl.x as usize;
            let min_y = border.min.y as usize;
            let max_x = if (bl.x + width as i16) as usize > border.max.x { border.max.x } else {(bl.x + width as i16) as usize};
            let max_y = bl.y as usize;
            ClipRect::new(min_x as _, min_y as _, max_x as _, max_y as _)
        }
    };
    // now let's start rending from the "top left" of the renderable space.
    let mut words = Vec::<TypesetWord>::new();
    let mut cursor = Cursor::new(
        renderable.min.x + cursor_state.pt.x as usize,
        renderable.min.y + cursor_state.pt.y as usize,
        0 + cursor_state.line_height as usize
    ); // line_height tracks the tallest char so far
    let lines = s.split('\n');
    for line in lines {
        // for now, all white spaces are stripped and condensed into a single "space" character
        for word in line.split_whitespace() {
            // for each word, first compute the candidate's overall width and height
            // the origin point is 0,0, because we will translate it to the correct spot later
            let mut tsw = TypesetWord::new(Pt::new(0, 0));
            for ch in word.chars() {
                tsw.push(style_glyph(ch, base_style));
            }
            // there are five cases from here:
            // 1. The word fits in the remaining space on the line.
            // 2. The word doesn't fit in the remaining space on the line, but it could fit on a new line
            // 3. The word doesn't fit in the remaining space on the line, and there is no space for a new line (insert ellipses)
            // 4. The word can't fit in the width of a single line and must be broken up into two parts, and there's a new line
            // 5. The word can't fit in the width of a single line and there's no new lines avaliable to break it up (terminate with ellipses)
            if tsw.width >= renderable.max.x // word is longer than our renderable width
            {
                // case 4 and 5, word is longer than a line.
                // start by laying out characters until the line is "full", then, either wrap or ellipses depending upon new line state
                for gs in tsw.gs {
                    if cursor.pt.x + (gs.wide as usize) < renderable.max.x {
                        let before = cursor.pt.clone();
                        cursor.update_glyph(&gs);
                        words.push(TypesetWord::one_glyph(before, gs));
                    } else if tsw.height + cursor.pt.y + cursor.line_height < renderable.max.y { // there's a new line available for more chars
                        // put the cursor on the next line
                        cursor.pt.x = renderable.min.x;
                        cursor.pt.y += cursor.line_height;
                        cursor.line_height = 0;
                        // now push the next char
                        let before = cursor.pt.clone();
                        cursor.update_glyph(&gs);
                        words.push(TypesetWord::one_glyph(before, gs));
                    } else { // no more lines available, push an ellipsis and bail
                        *overflow = true;
                        words.push(TypesetWord::one_glyph(cursor.pt.clone(), ellipsis.clone()));
                        cursor.update_glyph(&ellipsis);
                        break
                    }
                }
                if *overflow { // abort further rendering if we hit an overflow condition
                    break;
                }
            } else if tsw.width + cursor.pt.x < renderable.max.x {
                // case 1, everything fits. Push the word into the rendering set, and add a space or newline to prep for the next word
                tsw.offset(cursor.pt.x as _, cursor.pt.y as _); // offset the rendering location of the word to the current cursor location
                cursor.update_word(&tsw); // move the cursor location based on the absolute width/height of the word
                words.push(tsw); // store the word
                // add a trailing space or new line
                let before = cursor.pt.clone();
                if cursor.space_or_newline(&space, renderable.max.x, renderable.min.x) {
                    words.push(TypesetWord::one_glyph(before, space.clone()));
                }
            } else if (tsw.width + cursor.pt.x >= renderable.max.x) // it's longer than our current line
            && (tsw.height + cursor.pt.y + cursor.line_height < renderable.max.y) // there's a new line available to hold the whole word
            && (tsw.width < renderable.max.x) // the whole word could fit on a new line
            { // case 2, put the word on a new line
                // put the cursor on a new line
                cursor.pt.x = renderable.min.x;
                cursor.pt.y += cursor.line_height;
                cursor.line_height = 0;
                // now push the word
                tsw.offset(cursor.pt.x as _, cursor.pt.y as _);
                cursor.update_word(&tsw);
                words.push(tsw);
                // add a trailing space or new line
                let before = cursor.pt.clone();
                if cursor.space_or_newline(&space, renderable.max.x, renderable.min.x) {
                    words.push(TypesetWord::one_glyph(before, space.clone()));
                }
            } else if (tsw.width + cursor.pt.x >= renderable.max.x) // it's longer than our current line
            && (tsw.height + cursor.pt.y + cursor.line_height >= renderable.max.y) // there's no lines available
            { // case 3, no more space for words
                *overflow = true;
                words.push(TypesetWord::one_glyph(cursor.pt.clone(), ellipsis.clone()));
                cursor.update_glyph(&ellipsis);
                break
            } else {
                // unhandled case, log the parameters so we can handle it
                log::error!("Unhandled case in word wrapper: new word width {} height {}, cursor {:?}, renderable {:?}", tsw.width, tsw.height, cursor, renderable);
            }
        }
        // don't render any more lines if we hit an overflow condition
        if *overflow {
            break;
        }
    }
    // now update the computed bounds, based on the final value of the cursor
    let actual_width = cursor.pt.x - renderable.min.x;
    let actual_height = (cursor.pt.y + cursor.line_height) - renderable.min.y;
    match &bounds {
        TextBounds::BoundingBox(_) => {
            // do nothing, this is a fixed box and we just fill it in from the top left.
        }
        TextBounds::GrowableFromBr(br, width) => {
            border.min.x = br.x as usize - actual_width;
            border.min.y = br.y as usize - actual_height;
            border.max.x = br.x as usize;
            border.max.y = br.y as usize;
        }
        TextBounds::GrowableFromTl(tl, width) => {
            border.min.x = tl.x as usize;
            border.min.y = tl.y as usize;
            border.max.x = tl.x as usize + actual_width;
            border.max.y = tl.y as usize + actual_height;
        }
        TextBounds::GrowableFromBl(bl, width) => {
            border.min.x = bl.x as usize;
            border.min.y = bl.y as usize - actual_height;
            border.max.x = bl.x as usize + actual_width;
            border.max.y = bl.y as usize;
        }
    }
    // update the cursor based on the final state
    cursor_state.pt.x = (cursor.pt.x - renderable.min.x) as i32;
    cursor_state.pt.y = (cursor.pt.y - renderable.min.y) as i32;

    words
}


/// Find glyph for char using latin regular, emoji, ja, zh, and kr font data
pub fn style_glyph(ch: char, base_style: &GlyphStyle) -> GlyphSprite {
    match xous::LANG {
        "zh" => {
            style_wrapper!(zh_rules, base_style, ch)
        }
        "jp" => {
            style_wrapper!(jp_rules, base_style, ch)
        }
        "kr" => {
            style_wrapper!(kr_rules, base_style, ch)
        }
        "en-audio" => {
            style_wrapper!(en_audio_rules, base_style, ch)
        }
        // default to English rules
        _ => {
            style_wrapper!(english_rules, base_style, ch)
        }
    }
}
