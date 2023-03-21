// NOTE: Adapted from cortex-m/build.rs
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap();
    let name = env::var("CARGO_PKG_NAME").unwrap();

    if target.starts_with("riscv") || target.starts_with("arm") {
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
