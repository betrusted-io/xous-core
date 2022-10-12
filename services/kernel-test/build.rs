// NOTE: Adapted from cortex-m/build.rs
use std::env;

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();

    // If we're not running on a desktop-class operating system, emit the "baremetal"
    // config setting. This will enable software to do tasks such as
    // managing memory.
    if target_os == "none" {
        println!("cargo:rustc-cfg=baremetal");
    }

    println!("cargo:rerun-if-changed=build.rs");
}
