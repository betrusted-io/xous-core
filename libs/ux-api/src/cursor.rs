// Copyright (c) 2022 Sam Blenny
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
use blitstr2::{ClipRect, GlyphSprite};

use crate::{minigfx::Point, wordwrap::TypesetWord};

/// Cursor specifies a drawing position along a line of text. Lines of text can
/// be different heights. Line_height is for keeping track of the tallest
/// character that has been drawn so far on the current line.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Cursor {
    pub pt: Point,
    pub line_height: usize,
}
#[allow(dead_code)]
impl Cursor {
    // Make a new Cursor. When in doubt, set line_height = 0.
    pub fn new(x: isize, y: isize, line_height: usize) -> Cursor {
        Cursor { pt: Point { x, y }, line_height }
    }

    // Make a Cursor aligned at the top left corner of a ClipRect
    pub fn from_top_left_of(r: ClipRect) -> Cursor {
        Cursor { pt: Point::new(r.min.x, r.min.y), line_height: 0 }
    }

    pub fn update_glyph(&mut self, glyph: &GlyphSprite) {
        self.pt.x += glyph.wide as isize;
        self.line_height = self.line_height.max(glyph.high as usize);
    }

    pub(crate) fn update_word(&mut self, word: &TypesetWord) {
        self.pt.x += word.width;
        self.line_height = self.line_height.max(word.height as usize);
    }

    pub fn add(&self, other: Cursor) -> Cursor {
        Cursor {
            pt: Point::new(self.pt.x + other.pt.x, self.pt.y + other.pt.y),
            line_height: self.line_height.max(other.line_height),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_equivalence() {
        let c1 = Cursor { pt: Pt { x: 1, y: 2 }, line_height: 0 };
        let c2 = Cursor::new(1, 2, 0);
        assert_eq!(c1, c2);
        let clip = ClipRect::new(1, 2, 3, 4);
        let c3 = Cursor::from_top_left_of(clip);
        assert_eq!(c1, c3);
    }

    #[test]
    fn test_cursor_from_clip_rect() {
        let cr = ClipRect::new(1, 2, 8, 9);
        let c = Cursor::from_top_left_of(cr);
        assert_eq!(c.pt, cr.min);
    }
}
