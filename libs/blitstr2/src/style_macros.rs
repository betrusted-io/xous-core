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
#[cfg(feature = "zh")]
macro_rules! zh_rules {
    ($base_style:expr, $emoji_style:expr, $ch:ident) => {{
        // Always-present: zh itself, base, emoji
        #[allow(unused_mut)]
        let mut result =
            zh_glyph($ch).ok().or_else(|| $base_style($ch).ok()).or_else(|| $emoji_style($ch).ok());

        #[cfg(feature = "ja")]
        if result.is_none() {
            result = ja_glyph($ch).ok();
        }

        #[cfg(feature = "kr")]
        if result.is_none() {
            result = kr_glyph($ch).ok();
        }

        // Always-present: replacement and null sentinel
        result.or_else(|| $base_style(REPLACEMENT).ok()).unwrap_or(NULL_GLYPH_SPRITE)
    }};
}

#[macro_export]
#[cfg(feature = "ja")]
macro_rules! jp_rules {
    ($base_style:expr, $emoji_style:expr, $ch:ident) => {{
        // Always-present: ja itself, base, emoji
        #[allow(unused_mut)]
        let mut result =
            ja_glyph($ch).ok().or_else(|| $base_style($ch).ok()).or_else(|| $emoji_style($ch).ok());

        // Optional: zh fallback only if zh is in the build
        #[cfg(feature = "zh")]
        if result.is_none() {
            result = zh_glyph($ch).ok();
        }

        // Optional: kr fallback only if kr is in the build
        #[cfg(feature = "kr")]
        if result.is_none() {
            result = kr_glyph($ch).ok();
        }

        // Always-present: replacement and null sentinel
        result.or_else(|| $base_style(REPLACEMENT).ok()).unwrap_or(NULL_GLYPH_SPRITE)
    }};
}

#[macro_export]
#[cfg(feature = "kr")]
macro_rules! kr_rules {
    ($base_style:expr, $emoji_style:expr, $ch:ident) => {{
        // Always-present: kr itself, base, emoji
        #[allow(unused_mut)]
        let mut result =
            kr_glyph($ch).ok().or_else(|| $base_style($ch).ok()).or_else(|| $emoji_style($ch).ok());

        // Optional: ja fallback only if ja is in the build
        #[cfg(feature = "ja")]
        if result.is_none() {
            result = ja_glyph($ch).ok();
        }

        // Optional: zh fallback only if zh is in the build
        #[cfg(feature = "zh")]
        if result.is_none() {
            result = zh_glyph($ch).ok();
        }

        // Always-present: replacement and null sentinel
        result.or_else(|| $base_style(REPLACEMENT).ok()).unwrap_or(NULL_GLYPH_SPRITE)
    }};
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
    ($base_style:expr, $emoji_style:expr, $ch:ident) => {{
        // Always-present: base, emoji
        #[allow(unused_mut)]
        let mut result = $base_style($ch).ok().or_else(|| $emoji_style($ch).ok());

        // Optional: zh fallback only if zh is in the build
        #[cfg(feature = "zh")]
        if result.is_none() {
            result = zh_glyph($ch).ok();
        }

        // Always-present: replacement and null sentinel
        result.or_else(|| $base_style(REPLACEMENT).ok()).unwrap_or(NULL_GLYPH_SPRITE)
    }};
}

pub use en_audio_rules;
pub use english_rules;
#[cfg(feature = "ja")]
pub use jp_rules;
#[cfg(feature = "kr")]
pub use kr_rules;
pub use style_wrapper;
#[cfg(feature = "zh")]
pub use zh_rules;
