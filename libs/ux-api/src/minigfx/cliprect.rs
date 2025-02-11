// Copyright (c) 2022 Sam Blenny
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
use super::*;
use crate::platform::{LINES, WIDTH};

/// ClipRect specifies a region of pixels. X and y pixel ranges are inclusive of
/// min and exclusive of max (i.e. it's min.x..max.x rather than min.x..=max.x)
/// Coordinate System Notes:
/// - (0,0) is top left
/// - Increasing Y moves downward on the screen, increasing X moves right
#[derive(Copy, Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct ClipRect {
    pub min: Pt,
    pub max: Pt,
}
#[allow(dead_code)]
impl ClipRect {
    /// Initialize a rectangle using automatic min/max fixup for corner points
    pub fn new(min_x: isize, min_y: isize, max_x: isize, max_y: isize) -> ClipRect {
        // Make sure min_x <= max_x && min_y <= max_y
        let mut min = Pt { x: min_x, y: min_y };
        let mut max = Pt { x: max_x, y: max_y };
        if min_x > max_x {
            min.x = max_x;
            max.x = min_x;
        }
        if min_y > max_y {
            min.y = max_y;
            max.y = min_y;
        }
        ClipRect { min, max }
    }

    pub fn to_rect(&self) -> Rectangle {
        Rectangle::new_coords(
            self.min.x as isize,
            self.min.y as isize,
            self.max.x as isize,
            self.max.y as isize,
        )
    }

    /// Make a rectangle of the full screen size (0,0)..(WIDTH,LINES)
    pub fn full_screen() -> ClipRect { ClipRect::new(0, 0, WIDTH, LINES) }

    /// Make a rectangle of the screen size minus padding (6,6)..(WIDTH-6,LINES-6)
    pub fn padded_screen() -> ClipRect {
        let pad = 6;
        ClipRect::new(pad, pad, WIDTH - pad, LINES - pad)
    }

    pub fn intersects(&self, other: ClipRect) -> bool {
        !(self.max.x < other.min.x
            || self.min.y > other.max.y
            || self.max.y < other.min.y
            || self.min.x > other.max.x)
    }

    pub fn intersects_point(&self, point: Pt) -> bool {
        ((point.x >= self.min.x) && (point.x <= self.max.x))
            && ((point.y >= self.min.y) && (point.y <= self.max.y))
    }

    /// takes the current Rectangle, and clips it with a clipping Rectangle; returns a new rectangle as the
    /// result
    pub fn clip_with(&self, clip: ClipRect) -> Option<ClipRect> {
        // check to see if we even overlap; if not, don't do any computation
        if !self.intersects(clip) {
            return None;
        }
        let tl: Pt = Pt::new(
            if self.min.x < clip.min.x { clip.min.x } else { self.min.x },
            if self.min.y < clip.min.y { clip.min.y } else { self.min.y },
        );
        let br: Pt = Pt::new(
            if self.max.x > clip.max.x { clip.max.x } else { self.max.x },
            if self.max.y > clip.max.y { clip.max.y } else { self.max.y },
        );
        Some(ClipRect::new(tl.x, tl.y, br.x, br.y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cliprect_equivalence() {
        let cr1 = ClipRect { min: Pt { x: 1, y: 2 }, max: Pt { x: 8, y: 9 } };
        // Called properly:
        let cr2 = ClipRect::new(1, 2, 8, 9);
        // Called with mixed up corners that should get auto-corrected
        let cr3 = ClipRect::new(8, 2, 1, 9);
        let cr4 = ClipRect::new(1, 9, 8, 2);
        assert_eq!(cr1, cr2);
        assert_eq!(cr2, cr3);
        assert_eq!(cr3, cr4);
    }

    #[test]
    fn test_cliprect_full_screen() {
        let clip = ClipRect::full_screen();
        assert_eq!(clip.min, Pt::new(0, 0));
        assert_eq!(clip.max, Pt::new(WIDTH, LINES));
    }

    #[test]
    fn test_cliprect_padded_screen() {
        let c1 = ClipRect::full_screen();
        let c2 = ClipRect::padded_screen();
        assert!(c2.min > c1.min);
        assert!(c2.max < c1.max);
    }
}
