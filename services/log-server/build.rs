// NOTE: Adapted from cortex-m/build.rs
use std::env;

extern crate vergen;
use vergen::{ConstantsFlags, generate_cargo_keys};

fn main() {
    let target = env::var("TARGET").unwrap();

    generate_cargo_keys(ConstantsFlags::SHA | ConstantsFlags::BUILD_TIMESTAMP).unwrap();

    let target_os = target.split('-').nth(2).unwrap_or("none");

    // If we're not running on a desktop-class operating system, emit the "baremetal"
    // config setting. This will enable software to do tasks such as
    // managing memory.
    if target_os == "none" {
        println!("cargo:rustc-cfg=baremetal");
    }

    println!("cargo:rerun-if-changed=build.rs");
}
