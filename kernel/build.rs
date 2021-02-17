// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

// NOTE: Adapted from cortex-m/build.rs
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap();
    let name = env::var("CARGO_PKG_NAME").unwrap();

    let target_os = target.split('-').nth(2).unwrap_or("none");

    // If we're not running on a desktop-class operating system, emit the "baremetal"
    // config setting. This will enable software to do tasks such as
    // managing memory.
    if target_os == "none" {
        println!("Target {} is bare metal", target);
        println!("cargo:rustc-cfg=baremetal");
    } else {
        println!("Target {} is NOT bare metal", target);
    }

    // For RISC-V, link in the startup library.
    if target.starts_with("riscv") {
        fs::copy(
            format!("bin/{}.a", target),
            out_dir.join(format!("lib{}.a", name)),
        )
        .unwrap();

        println!("cargo:rustc-link-lib=static={}", name);
        println!("cargo:rustc-link-search={}", out_dir.display());
        println!("cargo:rerun-if-changed=bin/{}.a", target);

        // Put the linker script somewhere the linker can find it
        fs::File::create(out_dir.join("link.x"))
            .unwrap()
            .write_all(include_bytes!("link.x"))
            .unwrap();
        println!("cargo:rustc-link-search={}", out_dir.display());
        println!("cargo:rerun-if-changed=link.x");
    }

    println!("cargo:rerun-if-changed=build.rs");
}
