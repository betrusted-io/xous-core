#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
pub mod cursor;
mod fontmap;
pub mod minigfx;
pub mod platform;
#[cfg(feature = "std")]
pub mod widgets;
#[cfg(feature = "std")]
mod wordwrap;
#[cfg(feature = "std")]
pub const SYSTEM_STYLE: blitstr2::GlyphStyle = blitstr2::GlyphStyle::Tall;
