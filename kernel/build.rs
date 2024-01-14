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
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let name = env::var("CARGO_PKG_NAME").unwrap();

    // If we're not running on a desktop-class operating system, emit the "baremetal"
    // config setting. This will enable software to do tasks such as
    // managing memory.
    if target_os == "none" || target_os == "xous" {
        println!("Target {} is bare metal", target);
        println!("cargo:rustc-cfg=baremetal");
    } else {
        println!("Target {} is NOT bare metal", target);
    }

    // For RISC-V and ARM, link in the startup library.
    if target.starts_with("riscv") || target.starts_with("arm") {
        fs::copy(
            format!("bin/{}.a", target),
            out_dir.join(format!("lib{}.a", name)),
        )
        .unwrap();

        println!("cargo:rustc-link-lib=static={}", name);
        println!("cargo:rustc-link-search={}", out_dir.display());
        println!("cargo:rerun-if-changed=bin/{}.a", target);
        println!("cargo:rustc-link-arg=-Tlink.x");

        let linker_file_path = if target.starts_with("arm") {
            PathBuf::from("src/arch/arm/link.x")
        } else {
            PathBuf::from("link.x")
        };

        // Put the linker script somewhere the linker can find it
        fs::File::create(out_dir.join("link.x"))
            .unwrap()
            .write_all(fs::read_to_string(linker_file_path).expect("linker file read").as_bytes())
            .unwrap();

        println!("cargo:rustc-link-search={}", out_dir.display());
        println!("cargo:rerun-if-changed=link.x");
        println!("cargo:rustc-link-arg=-Map=kernel.map");
    }

    println!("cargo:rerun-if-changed=build.rs");

    // CI sets this variable. This changes how the panic handler works.
    println!("cargo:rerun-if-env-changed=CI");
    if option_env!("CI").is_some() {
        println!("cargo:rustc-cfg=ci");
    }
}
