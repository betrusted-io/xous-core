/// Style options for Latin script fonts
#[derive(Copy, Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum GlyphStyle {
    Small = 0,
    Regular = 1,
    Bold = 2,
    Monospace = 3,
    Cjk = 4,
    Large = 5,
    ExtraLarge = 6,
    Tall = 7,
}

/// Convert number to style for use with register-based message passing sytems
// [by bunnie for Xous]
impl From<usize> for GlyphStyle {
    fn from(gs: usize) -> Self {
        match gs {
            0 => GlyphStyle::Small,
            1 => GlyphStyle::Regular,
            2 => GlyphStyle::Bold,
            3 => GlyphStyle::Monospace,
            4 => GlyphStyle::Cjk,
            5 => GlyphStyle::Large,
            6 => GlyphStyle::ExtraLarge,
            7 => GlyphStyle::Tall,
            _ => GlyphStyle::Regular,
        }
    }
}

/// Convert style to number for use with register-based message passing sytems
// [by bunnie for Xous]
impl From<GlyphStyle> for usize {
    fn from(g: GlyphStyle) -> usize {
        match g {
            GlyphStyle::Small => 0,
            GlyphStyle::Regular => 1,
            GlyphStyle::Bold => 2,
            GlyphStyle::Monospace => 3,
            GlyphStyle::Cjk => 4,
            GlyphStyle::Large => 5,
            GlyphStyle::ExtraLarge => 6,
            GlyphStyle::Tall => 7,
        }
    }
}

/// Estimate line-height for Latin script text in the given style
/// These are hard-coded in because we want to keep the rest of the font data
/// structures private to this crate. Moving the font files out of their
/// current location would also require modifying a bunch of codegen infrastruture,
/// so, this is one spot where we have to manually maintain a link.
pub fn glyph_to_height_hint(g: GlyphStyle) -> usize {
    match g {
        GlyphStyle::Small => 12,      // crate::blitstr2::fonts::small::MAX_HEIGHT as usize,
        GlyphStyle::Regular => 15,    // crate::blitstr2::fonts::regular::MAX_HEIGHT as usize,
        GlyphStyle::Bold => 15,       // crate::blitstr2::fonts::regular::MAX_HEIGHT as usize,
        GlyphStyle::Monospace => 15,  // crate::blitstr2::fonts::mono::MAX_HEIGHT as usize,
        GlyphStyle::Cjk => 16,        // crate::blistr2::fonts::emoji::MAX_HEIGHT as usize,
        GlyphStyle::Large => 24,      // 2x of small
        GlyphStyle::ExtraLarge => 30, // 2x of regular
        GlyphStyle::Tall => 19,
    }
}
