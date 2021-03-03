#![cfg_attr(target_os = "none", no_std)]

pub use ime_plugin_api::*;

pub struct PredictionApiImpl {
}

impl PredictionApi for PredictionApiImpl {
    // inherit all the default methods
}