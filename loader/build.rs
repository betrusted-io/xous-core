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
        #[cfg(feature="cramium-soc")]
        let p = PathBuf::from("src/platform/cramium/link-soc.x");
        #[cfg(feature="cramium-fpga")]
        let p = PathBuf::from("src/platform/cramium/link-soc.x");  // note deviation -- this is because we are still simulating the FPGA model and offsets are not yet settled for that
        #[cfg(not(any(feature="cramium-soc", feature="cramium-fpga")))]
        let p = PathBuf::from("src/platform/precursor/link.x");

        println!("cargo:rerun-if-changed={}", p.clone().into_os_string().into_string().unwrap());
        println!("cargo:rustc-link-arg=-Tlink.x");

        p
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

    println!("{}", out_dir.join("link.x").display());
    // Put the linker script somewhere the linker can find it
    std::fs::OpenOptions::new().create(true).write(true).truncate(true).open(out_dir.join("link.x"))
        .unwrap()
        .write_all(fs::read_to_string(linker_file_path).expect("linker file read").as_bytes())
        .unwrap();
    println!("cargo:rustc-link-search={}", out_dir.display());

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=link.x");
}
