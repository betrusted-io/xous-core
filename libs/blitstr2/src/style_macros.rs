// A set of macros that define the priority order for resolving fonts across language definitions.
#[macro_export]
macro_rules! style_wrapper {
    ($rule:ident, $base_style:ident, $ch:ident) => {
        match &$base_style {
            GlyphStyle::Small => {
                $rule!(small_glyph, emoji_glyph, $ch)
            }
            GlyphStyle::Bold => {
                $rule!(bold_glyph, emoji_glyph, $ch)
            }
            GlyphStyle::Monospace => {
                $rule!(mono_glyph, emoji_glyph, $ch)
            }
            GlyphStyle::Large => {
                $rule!(large_glyph, emoji_large_glyph, $ch)
            }
            GlyphStyle::ExtraLarge => {
                $rule!(extra_large_glyph, emoji_large_glyph, $ch)
            }
            GlyphStyle::Tall => {
                $rule!(tall_glyph, emoji_glyph, $ch)
            }
            // default to regular
            _ => {
                $rule!(regular_glyph, emoji_glyph, $ch)
            }
        }
    };
}

#[macro_export]
macro_rules! zh_rules {
    ($base_style:expr, $emoji_style:expr, $ch:ident) => {
        match zh_glyph($ch) {
            Ok(g) => g,
            _ => match $base_style($ch) {
                Ok(g) => g,
                _ => match $emoji_style($ch) {
                    Ok(g) => g,
                    _ => match ja_glyph($ch) {
                        Ok(g) => g,
                        _ => match kr_glyph($ch) {
                            Ok(g) => g,
                            _ => match $base_style(REPLACEMENT) {
                                Ok(g) => g,
                                _ => NULL_GLYPH_SPRITE,
                            },
                        },
                    },
                },
            },
        }
    };
}

#[macro_export]
macro_rules! jp_rules {
    ($base_style:expr, $emoji_style:expr, $ch:ident) => {
        match ja_glyph($ch) {
            Ok(g) => g,
            _ => match $base_style($ch) {
                Ok(g) => g,
                _ => match $emoji_style($ch) {
                    Ok(g) => g,
                    _ => match zh_glyph($ch) {
                        Ok(g) => g,
                        _ => match kr_glyph($ch) {
                            Ok(g) => g,
                            _ => match $base_style(REPLACEMENT) {
                                Ok(g) => g,
                                _ => NULL_GLYPH_SPRITE,
                            },
                        },
                    },
                },
            },
        }
    };
}

#[macro_export]
macro_rules! kr_rules {
    ($base_style:expr, $emoji_style:expr, $ch:ident) => {
        match kr_glyph($ch) {
            Ok(g) => g,
            _ => match $base_style($ch) {
                Ok(g) => g,
                _ => match $emoji_style($ch) {
                    Ok(g) => g,
                    _ => match ja_glyph($ch) {
                        Ok(g) => g,
                        _ => match zh_glyph($ch) {
                            Ok(g) => g,
                            _ => match $base_style(REPLACEMENT) {
                                Ok(g) => g,
                                _ => NULL_GLYPH_SPRITE,
                            },
                        },
                    },
                },
            },
        }
    };
}

#[macro_export]
macro_rules! en_audio_rules {
    ($base_style:expr, $emoji_style:expr, $ch:ident) => {
        match $base_style($ch) {
            Ok(g) => g,
            _ => match $emoji_style($ch) {
                Ok(g) => g,
                _ => match $base_style(REPLACEMENT) {
                    Ok(g) => g,
                    _ => NULL_GLYPH_SPRITE,
                },
            },
        }
    };
}

#[macro_export]
#[cfg(not(any(feature = "board-baosec", feature = "hosted-baosec")))]
macro_rules! english_rules {
    ($base_style:expr, $emoji_style:expr, $ch:ident) => {
        match $base_style($ch) {
            Ok(g) => g,
            _ => match $emoji_style($ch) {
                Ok(g) => g,
                _ => match ja_glyph($ch) {
                    Ok(g) => g,
                    _ => match zh_glyph($ch) {
                        Ok(g) => g,
                        _ => match kr_glyph($ch) {
                            Ok(g) => g,
                            _ => match $base_style(REPLACEMENT) {
                                Ok(g) => g,
                                _ => NULL_GLYPH_SPRITE,
                            },
                        },
                    },
                },
            },
        }
    };
}

#[macro_export]
// Reflect reduced font space on small-memory footprint devices
#[cfg(any(feature = "board-baosec", feature = "hosted-baosec"))]
macro_rules! english_rules {
    ($base_style:expr, $emoji_style:expr, $ch:ident) => {
        match $base_style($ch) {
            Ok(g) => g,
            _ => match $emoji_style($ch) {
                Ok(g) => g,
                _ => match $base_style(REPLACEMENT) {
                    Ok(g) => g,
                    _ => NULL_GLYPH_SPRITE,
                },
            },
        }
    };
}

pub use en_audio_rules;
pub use english_rules;
pub use jp_rules;
pub use kr_rules;
pub use style_wrapper;
pub use zh_rules;
