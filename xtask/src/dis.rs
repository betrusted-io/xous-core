// SPDX-license-identifier: BSD-2-Clause
//
// Optional post-build disassembly for baremetal targets.
//
// This module is deliberately best-effort: if no RISC-V objdump (or a close
// equivalent from one of the common toolchain prefixes) is found on PATH, we
// print a friendly hint and skip. Disassembly is a convenience for developers,
// so a missing C toolchain must never fail a build.
//
// The pipeline mirrors:
//   riscv-none-elf-objdump -d <elf> | rustfilt > <elf>.dis
// implemented with std::process so it works identically on Windows and Linux.
//
// This stub written with the help of qwen3.8-flash-next-unsloth-ud-iq4_xs + omp harness

use std::env;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The disassembly hint printed when no usable objdump is found.
const NO_TOOLCHAIN_MSG: &str = concat!(
    "No disassembly created; install the xpack toolchain ",
    "(https://github.com/xpack-dev-tools/riscv-none-elf-gcc-xpack/releases) ",
    "and add it to PATH for automatic disassembly.",
);

/// Common objdump prefixes across the RISC-V toolchains people actually use:
/// xpack (current and legacy names), SiFive, Andes, nuclei, Fedora/RISC-V,
/// Debian's multilib binutils, and plain riscv64-linux-gnu from distro repos.
const OBJDUMP_PREFIXES: &[&str] = &[
    "riscv-none-elf",      // xpack (current), upstream binutils default
    "riscv64-unknown-elf", // xpack (legacy), SiFive, old upstream
    "riscv32-unknown-elf", // 32-bit-only builds
    "riscv64-elf",         // Homebrew / Arch style
    "riscv32-elf",
    "riscv-elf",
    "riscv64-linux-gnu", // distro binutils; can read riscv32 ELFs fine
];

/// Probes a command by running `--version`. Returns true only if the process
/// launches and exits successfully. A missing binary simply errors out of
/// `status()`, which we treat as "not installed".
fn command_works(cmd: &str, args: &[&str]) -> bool {
    match Command::new(cmd).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status() {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

/// Finds a usable RISC-V objdump. Honors the `RISCV_OBJDUMP` environment
/// variable as an explicit override (full path or bare program name), then
/// walks the common toolchain prefixes on PATH. Returns the resolved program
/// name/path, or None if nothing works.
fn find_riscv_objdump() -> Option<String> {
    if let Ok(explicit) = env::var("RISCV_OBJDUMP") {
        if !explicit.is_empty() && command_works(&explicit, &["--version"]) {
            return Some(explicit);
        }
    }
    for prefix in OBJDUMP_PREFIXES {
        // the generic spelling is also present on some platforms
        for name in [format!("{}-objdump", prefix), format!("{}-objdump.exe", prefix)] {
            if command_works(&name, &["--version"]) {
                return Some(name);
            }
        }
    }
    None
}

/// Locates the cargo home directory, so we can find `rustfilt` even when
/// `~/.cargo/bin` is not currently on PATH (common on fresh CI runners and
/// on Windows where the installer modifies the registry, not this process).
fn cargo_home() -> Option<PathBuf> {
    if let Ok(dir) = env::var("CARGO_HOME") {
        return Some(PathBuf::from(dir));
    }
    #[cfg(windows)]
    let home = env::var("USERPROFILE").ok();
    #[cfg(not(windows))]
    let home = env::var("HOME").ok();
    home.map(|h| PathBuf::from(h).join(".cargo"))
}

/// Finds a rustfilt executable: first on PATH, then in the cargo bin directory.
fn find_rustfilt() -> Option<PathBuf> {
    if command_works("rustfilt", &["--version"]) {
        return Some(PathBuf::from("rustfilt"));
    }
    if let Some(home) = cargo_home() {
        let mut cand = home.join("bin").join("rustfilt");
        if cfg!(windows) {
            cand.set_extension("exe");
        }
        if cand.exists() {
            return Some(cand);
        }
    }
    None
}

/// Installs rustfilt via `cargo install`. Userspace-only, so it works without
/// admin rights on both Windows and Linux. Failures (typically offline dev
/// boxes) are reported but non-fatal.
fn install_rustfilt() -> Option<PathBuf> {
    println!(
        "rustfilt not found; installing it with `cargo install rustfilt` (this can take a few minutes)..."
    );
    let status = Command::new("cargo").args(["install", "rustfilt"]).status();
    match status {
        Ok(s) if s.success() => find_rustfilt(),
        _ => {
            println!("Could not install rustfilt; skipping disassembly (this does not affect the build).");
            None
        }
    }
}

/// Attempts `objdump -d <elf> | rustfilt > <elf>.dis`.
///
/// This never fails the build: every error path prints an informational
/// message and returns Ok(()). Call it right after a successful build of a
/// baremetal image; if the ELF doesn't exist, it is skipped silently-ish.
pub fn try_disassemble(elf_path: &Path) -> Result<(), ()> {
    if !elf_path.exists() {
        println!("No disassembly created: {} not found.", elf_path.display());
        return Ok(());
    }

    let objdump = match find_riscv_objdump() {
        Some(o) => o,
        None => {
            println!("{}", NO_TOOLCHAIN_MSG);
            return Ok(());
        }
    };

    let rustfilt = match find_rustfilt().or_else(install_rustfilt) {
        Some(r) => r,
        None => return Ok(()),
    };

    let mut dis_path = elf_path.as_os_str().to_os_string();
    dis_path.push(".dis");
    let dis_path = PathBuf::from(dis_path);
    let dis_file = match File::create(&dis_path) {
        Ok(f) => f,
        Err(e) => {
            println!("Could not create {} ({}); skipping disassembly.", dis_path.display(), e);
            return Ok(());
        }
    };

    // objdump -d <elf> | rustfilt > <elf>.dis
    let objdump_child = Command::new(&objdump)
        .args(["-d"])
        .arg(elf_path)
        .stdout(Stdio::piped())
        // objdump chatters on stderr about things we can't act on (e.g.
        // unparsable padding); keep it visible but don't pollute the .dis.
        .spawn();
    let mut objdump_child = match objdump_child {
        Ok(c) => c,
        Err(e) => {
            println!("Could not run `{}` ({}); skipping disassembly.", objdump, e);
            return Ok(());
        }
    };
    let objdump_stdout = objdump_child.stdout.take().expect("stdout was piped");
    let rustfilt_status =
        Command::new(&rustfilt).stdin(Stdio::from(objdump_stdout)).stdout(Stdio::from(dis_file)).status();
    let objdump_status = objdump_child.wait();

    let ok = matches!(&objdump_status, Ok(s) if s.success());
    match &rustfilt_status {
        Ok(s) if s.success() && ok => {
            println!("Disassembly written to {}", dis_path.display());
        }
        _ => {
            // leave a clean message and remove any partial garbage file
            let _ = std::fs::remove_file(&dis_path);
            println!(
                "Disassembly failed ({}: {:?}, rustfilt: {:?}); the build itself succeeded.",
                objdump, objdump_status, rustfilt_status
            );
        }
    }
    Ok(())
}
