#![cfg_attr(target_os = "none", no_std)]

pub const SERVER_NAME_IME_PLUGIN_TTS: &str = "_IME TTS plugin_";

// just inherit all the default from the ime_plugin_api
pub use ime_plugin_api::*;
