#![cfg_attr(not(feature = "std"), no_std)]

pub mod minigfx;
pub mod widgets;
mod wordwrap;

pub mod cursor;
mod fontmap;
pub mod platform;

pub const SYSTEM_STYLE: blitstr2::GlyphStyle = blitstr2::GlyphStyle::Tall;
