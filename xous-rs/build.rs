// NOTE: Adapted from cortex-m/build.rs
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap();
    let name = env::var("CARGO_PKG_NAME").unwrap();

    println!("cargo:rerun-if-changed=build.rs");
}
