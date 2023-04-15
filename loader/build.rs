// NOTE: Adapted from cortex-m/build.rs
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap();

    let linker_file_path = if target.starts_with("riscv") {
        println!("cargo:rustc-link-search={}", out_dir.display());
        println!("cargo:rustc-link-arg=-Tlink.x");

        PathBuf::from("link.x")
    } else if target.starts_with("armv7a") {
        let name = env::var("CARGO_PKG_NAME").unwrap();

        fs::copy(
            format!("bin/{}.a", target),
            out_dir.join(format!("lib{}.a", name)),
        ).expect("copy arm assembly binary");

        println!("cargo:rustc-link-lib=static={}", name);
        println!("cargo:rustc-link-search={}", out_dir.display());
        println!("cargo:rerun-if-changed=bin/{}.a", target);
        println!("cargo:rustc-link-arg=-Tlink.x");

        PathBuf::from("src/platform/atsama5d27/link.x")
    } else {
        unreachable!("unsupported target");
    };

    // Put the linker script somewhere the linker can find it
    fs::File::create(out_dir.join("link.x"))
        .unwrap()
        .write_all(fs::read_to_string(linker_file_path).expect("linker file read").as_bytes())
        .unwrap();
    println!("cargo:rustc-link-search={}", out_dir.display());

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=link.x");
}
