#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use xous::{CID, send_message};
use num_traits::ToPrimitive;
