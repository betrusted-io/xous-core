#![cfg_attr(not(feature = "std"), no_std)]

mod blitstr2;
pub mod minigfx;
pub mod widgets;
mod wordwrap;
#[macro_use]
mod style_macros;
mod fontmap;
pub mod platform;

pub const SYSTEM_STYLE: crate::minigfx::GlyphStyle = crate::minigfx::GlyphStyle::Tall;
