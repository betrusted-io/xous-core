#![allow(dead_code)] // because hosted mode doesn't use mappings
#![allow(unused_imports)]
mod qwerty;
pub (crate) use qwerty::*;
mod qwertz;
pub (crate) use qwertz::*;
mod azerty;
pub (crate) use azerty::*;
mod dvorak;
pub (crate) use dvorak::*;
