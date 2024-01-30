#![allow(dead_code)] // because hosted mode doesn't use mappings
#![allow(unused_imports)]
#[rustfmt::skip] // this file is a lookup table. Allow wide columns.
mod qwerty;
pub(crate) use qwerty::*;
#[rustfmt::skip] // this file is a lookup table. Allow wide columns.
mod qwertz;
pub(crate) use qwertz::*;
#[rustfmt::skip] // this file is a lookup table. Allow wide columns.
mod azerty;
pub(crate) use azerty::*;
#[rustfmt::skip] // this file is a lookup table. Allow wide columns.
mod dvorak;
pub(crate) use dvorak::*;
