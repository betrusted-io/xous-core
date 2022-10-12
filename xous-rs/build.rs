use core::panic;
// NOTE: Adapted from cortex-m/build.rs
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap();
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let name = env::var("CARGO_PKG_NAME").unwrap();

    // If we're not running on a desktop-class operating system, emit the "baremetal"
    // config setting. This will enable software to do tasks such as
    // managing memory.
    if target_os == "none" {
        println!("Target {} is bare metal", target);
        println!("cargo:rustc-cfg=baremetal");
    } else {
        println!("Target {} is NOT bare metal", target);
    }

    if target.starts_with("riscv") {
        fs::copy(
            format!("bin/{}.a", target),
            out_dir.join(format!("lib{}.a", name)),
        )
        .expect("couldn't find asm support library for target platform");

        println!("cargo:rustc-link-lib=static={}", name);
        println!("cargo:rustc-link-search={}", out_dir.display());
        println!("cargo:rerun-if-changed=bin/{}.a", target);
    }

    println!("cargo:rerun-if-changed=build.rs");
}
