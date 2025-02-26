#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
pub mod cursor;
mod fontmap;
pub mod minigfx;
pub mod platform;
#[cfg(all(feature = "std", any(feature = "cramium-soc", feature = "hosted-baosec")))]
pub mod widgets;
#[cfg(feature = "std")]
pub mod wordwrap;
#[cfg(feature = "std")]
pub const SYSTEM_STYLE: blitstr2::GlyphStyle = blitstr2::GlyphStyle::Tall;
pub mod bitmaps;
#[cfg(feature = "std")]
pub mod service;
