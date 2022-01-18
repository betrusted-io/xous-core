/// Style options for Latin script fonts
#[derive(Copy, Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum GlyphStyle {
    Small = 0,
    Regular = 1,
    Bold = 2,
    Monospace = 3,
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
            _ => GlyphStyle::Regular,
        }
    }
}

/// Convert style to number for use with register-based message passing sytems
// [by bunnie for Xous]
impl Into<usize> for GlyphStyle {
    fn into(self) -> usize {
        match self {
            GlyphStyle::Small => 0,
            GlyphStyle::Regular => 1,
            GlyphStyle::Bold => 2,
            GlyphStyle::Monospace => 3,
        }
    }
}

/// Estimate line-height for Latin script text in the given style
pub fn glyph_to_height_hint(g: GlyphStyle) -> usize {
    match g {
        GlyphStyle::Small => crate::blitstr2::fonts::small::MAX_HEIGHT as usize,
        GlyphStyle::Regular => crate::blitstr2::fonts::regular::MAX_HEIGHT as usize,
        GlyphStyle::Bold => crate::blitstr2::fonts::regular::MAX_HEIGHT as usize,
        GlyphStyle::Monospace => crate::blitstr2::fonts::mono::MAX_HEIGHT as usize,
    }
}