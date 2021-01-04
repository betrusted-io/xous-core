#![cfg_attr(target_os = "none", no_std)]

pub mod api;

use xous::{send_message, Error, CID};
