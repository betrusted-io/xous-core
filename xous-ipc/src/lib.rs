#![cfg_attr(target_os = "none", no_std)]

mod buffer;
pub use buffer::*;
pub use buffer::XousDeserializer;

mod string;
pub use string::*;
