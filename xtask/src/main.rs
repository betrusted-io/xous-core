mod app_manifest;
use app_manifest::*;
mod versioning;
use versioning::*;
mod utils;
use utils::*;

use std::fs::OpenOptions;
use std::{
    env,
    io::{Read, Write},
    path::{Path, PathBuf, MAIN_SEPARATOR},
    process::Command,
};

// This is the default SVD file to be used to generate Xous. This must be manually
// updated every time the SoC version is bumped.
const SOC_SVD_VERSION: &str = "precursor-c809403";

// This is the minimum Xous version required to read a PDDB backup generated
// by the current kernel revision.
const MIN_XOUS_VERSION: &str = "v0.9.8-791";

type DynError = Box<dyn std::error::Error>;

const PROGRAM_TARGET: &str = "riscv32imac-unknown-xous-elf";
const KERNEL_TARGET: &str = "riscv32imac-unknown-xous-elf";

enum MemorySpec {
    SvdFile(String),
}

#[derive(Debug)]
enum BuildError {
    PathConversionError,
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            BuildError::PathConversionError => write!(f, "could not convert path to UTF-8"),
        }
    }
}

impl std::error::Error for BuildError {}

fn cargo() -> String {
    env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

fn project_root() -> PathBuf {
    Path::new(&env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)
        .unwrap()
        .to_path_buf()
}

// Filter out all arguments, which start with "-"
fn get_packages() -> Vec<String> {
    let mut args = env::args();
    args.nth(1);
    // skip everything past --
    let mut pkgs = Vec::<String>::new();
    for arg in args {
        if arg == "--" {
            break;
        }
        pkgs.push(arg);
    }
    pkgs.into_iter().filter(|x| !x.starts_with("-")).collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    generate_version(env::args().filter(|x| x == "--no-timestamp").count() == 0); // really brute force way to try and get a version into the build system.

    let hw_pkgs = [
        // core OS services
        "gam",
        "status",
        "shellchat",
        "ime-frontend",
        "ime-plugin-shell",
        "graphics-server",
        "ticktimer-server",
        "log-server",
        "com",
        "xous-names",
        "keyboard",
        "trng",
        "llio",
        "susres",
        "codec",
        "sha2",
        "engine-25519",
        "spinor",
        "root-keys",
        "jtag",
        "net",
        "dns",
        "pddb",
        "modals",
        "usb-device-xous",
    ];
    let minimal_pkgs = [
        "ticktimer-server",
        "log-server",
        "xous-names",
        "trng",
        "llio",
        "susres",
        "com",
    ];
    // A base set of packages. This is all you need for a normal
    // operating system that can run libstd
    let base_pkgs = [
        "ticktimer-server",
        "log-server",
        "susres",
        "xous-names",
        "trng",
    ];
    let pddb_dev_pkgs = [
        // just for checking compilation
        "ticktimer-server",
        "log-server",
        "susres",
        "xous-names",
        "trng",
        "pddb",
        "sha2",
        /*
        "llio",
        "root-keys",
        "jtag",
        "rtc",
        "com",
        "gam",
        "graphics-server",
        "keyboard",
        "ime-frontend",
        "ime-plugin-shell",
        "status",
        */
    ];
    let gfx_dev_pkgs = [
        "ticktimer-server",
        "log-server",
        "xous-names",
        "trng",
        "llio",
        "susres",
        "com",
        "graphics-server",
        "keyboard",
        "spinor",
    ];
    let aestest_pkgs = ["ticktimer-server", "log-server", "aes-test"];

    let mut args = env::args();
    let task = args.nth(1);
    // extract lkey/kkey args only after a "--" separator
    let mut next_is_lkey = false;
    let mut next_is_kkey = false;
    let mut lkey: Option<String> = None;
    let mut kkey: Option<String> = None;
    for arg in args {
        if next_is_kkey {
            kkey = Some(arg);
            next_is_kkey = false;
            continue;
        }
        if next_is_lkey {
            lkey = Some(arg);
            next_is_lkey = false;
            next_is_kkey = true;
            continue;
        }
        if arg == "--" {
            next_is_lkey = true;
            continue;
        }
    }
    match task.as_deref() {
        Some("install-toolkit") | Some("install-toolchain") => {
            let arg = env::args().nth(2);
            ensure_compiler(
                &Some(PROGRAM_TARGET),
                true,
                arg.map(|x| x == "--force").unwrap_or(false),
            )?
        }
        Some("renode-image") => {
            let mut args = env::args();
            args.nth(1);
            let mut pkgs = hw_pkgs.to_vec();
            let mut apps: Vec<String> = get_packages();
            if apps.len() == 0 {
                // add the standard demo apps if none are specified
                println!("No apps specified, adding default apps...");
                apps.push("ball".to_string());
                apps.push("repl".to_string());
            }
            for app in &apps {
                pkgs.push(app);
            }
            generate_app_menus(&apps);
            renode_image(false, &pkgs, &[], None, None)?
        }
        Some("renode-test") => {
            let mut args = env::args();
            args.nth(1);
            let mut pkgs = hw_pkgs.to_vec();
            let mut apps: Vec<String> = get_packages();
            if apps.len() == 0 {
                // add the standard demo apps if none are specified
                println!("No apps specified, adding default apps...");
                apps.push("ball".to_string());
                apps.push("repl".to_string());
            }
            for app in &apps {
                pkgs.push(app);
            }
            generate_app_menus(&apps);
            renode_image(false, &minimal_pkgs, &[], None, None)?
        }
        Some("tts") => {
            let tmp_dir = tempfile::Builder::new().prefix("bins").tempdir()?;
            let tts_exec_string = if true {
                println!("Fetching tts executable from build server...");
                let tts_exec_name = tmp_dir.path().join("espeak-embedded");
                let tts_exec_string = tts_exec_name
                    .clone()
                    .into_os_string()
                    .into_string()
                    .unwrap();
                let mut tts_exec_file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(tts_exec_name)
                    .expect("Can't open our version file for writing");
                let mut freader = ureq::get("https://ci.betrusted.io/job/espeak-embedded/lastSuccessfulBuild/artifact/target/riscv32imac-unknown-xous-elf/release/espeak-embedded")
                .call()?
                .into_reader();
                std::io::copy(&mut freader, &mut tts_exec_file)?;
                println!(
                    "TTS exec is {} bytes",
                    tts_exec_file.metadata().unwrap().len()
                );
                tts_exec_string
            } else {
                println!("****** WARNING: using local dev image. Do not check this configuration in! ******");
                "../espeak-embedded/target/riscv32imac-unknown-xous-elf/release/espeak-embedded"
                    .to_string()
            };

            let mut locale_override = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open("xous-rs/src/locale.rs")
                .expect("Can't open locale for modification");
            write!(
                locale_override,
                "{}",
                "pub const LANG: &str = \"en-tts\";\n"
            )
            .unwrap();

            let mut args = env::args();
            args.nth(1);
            let mut pkgs = hw_pkgs.to_vec();
            //let mut pkgs = gfx_dev_pkgs.to_vec();
            let apps: Vec<String> = get_packages();
            for app in &apps {
                pkgs.push(app);
            }
            pkgs.push("tts-frontend");
            pkgs.push("ime-plugin-tts");
            pkgs.retain(|&pkg| pkg != "ime-plugin-shell");
            generate_app_menus(&apps);
            build_hw_image(
                false,
                None,
                &pkgs,
                None,
                None,
                Some(&["--features", "tts", "--features", "braille"]),
                &[&tts_exec_string],
                None, None,
            )?;
            let mut locale_revert = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open("xous-rs/src/locale.rs")
                .expect("Can't open locale for modification");
            write!(locale_revert, "{}", "pub const LANG: &str = \"en\";\n").unwrap();
        }
        Some("usbdev") => {
            let mut args = env::args();
            args.nth(1);
            let mut pkgs = base_pkgs.to_vec();
            pkgs.push("usb-test");
            let args: Vec<String> = args.collect();
            let mut extra_packages = vec![];
            for program in &args {
                extra_packages.push(program.as_str());
            }
            if false {
                renode_image(
                    false,
                    &pkgs,
                    extra_packages.as_slice(),
                    None,
                    Some(&["--no-default-features", "--features", "renode-bypass", "--features", "debug-print"]),
                )?;
            } else {
                build_hw_image(false, None, &pkgs,
                None,
                None,
                None,
                extra_packages.as_slice(), None, None)?;
            }
        }
        Some("libstd-test") => {
            let mut args = env::args();
            args.nth(1);
            let pkgs = base_pkgs.to_vec();
            let args: Vec<String> = get_packages();
            let mut extra_packages = vec![];
            for program in &args {
                extra_packages.push(program.as_str());
            }
            renode_image(
                false,
                &pkgs,
                extra_packages.as_slice(),
                None,
                Some(&["--features", "renode-bypass"]),
            )?;
        }
        Some("ffi-test") => {
            let mut args = env::args();
            args.nth(1);
            //let mut pkgs = hw_pkgs.to_vec();
            let mut pkgs = gfx_dev_pkgs.to_vec();
            pkgs.push("ffi-test");
            let args: Vec<String> = get_packages();
            let mut extra_packages = vec![];
            for program in &args {
                extra_packages.push(program.as_str());
            }
            build_hw_image(
                false,
                None,
                &pkgs,
                None,
                None,
                None,
                &[],
                Some(&["--features", "renode-bypass"]),
                None,
            )?
            //renode_image(false, &pkgs, extra_packages.as_slice(),
            //None, Some(&["--features", "renode-bypass"]))?;
        }
        Some("libstd-net") => {
            let mut args = env::args();
            args.nth(1);
            let mut pkgs = base_pkgs.to_vec();
            pkgs.push("net");
            pkgs.push("com");
            pkgs.push("llio");
            pkgs.push("dns");
            let args: Vec<String> = get_packages();
            let mut extra_packages = vec![];
            for program in &args {
                extra_packages.push(program.as_str());
            }
            renode_image(
                false,
                &pkgs,
                extra_packages.as_slice(),
                Some(&["--features", "renode-minimal"]),
                Some(&["--features", "renode-bypass"]),
            )?;
        }
        Some("renode-aes-test") => {
            generate_app_menus(&Vec::<String>::new());
            renode_image(false, &aestest_pkgs, &[], None, None)?
        }
        Some("renode-image-debug") => {
            generate_app_menus(&vec!["ball".to_string()]);
            renode_image(true, &hw_pkgs, &[], None, None)?
        }
        Some("pddb-ci") => {
            generate_app_menus(&Vec::<String>::new());
            run(
                false,
                &hw_pkgs,
                Some(&["--features", "pddb/ci", "--features", "pddb/deterministic"]),
                false,
            )?
        }
        Some("pddb-btest") => {
            generate_app_menus(&Vec::<String>::new());
            // for hosted runs, compile in the pddb test routines by default...for now.
            run(false, &hw_pkgs,
                Some(&[
                    "--features", "pddbtest",
                    "--features", "autobasis", // this will make secret basis tracking synthetic and automated for stress testing
                    "--features", "pddb/deterministic",
                    "--features", "autobasis-ci",
                ]), false)?
        }
        Some("run") => {
            let mut args = env::args();
            args.nth(1);
            let mut pkgs = hw_pkgs.to_vec();
            let mut apps: Vec<String> = get_packages();
            if apps.len() == 0 {
                // add the standard demo apps if none are specified
                println!("No apps specified, adding default apps...");
                apps.push("ball".to_string());
                apps.push("repl".to_string());
            }
            for app in &apps {
                pkgs.push(app);
            }
            generate_app_menus(&apps);
            // for hosted runs, compile in the pddb test routines by default...for now.
            run(false, &pkgs,
                Some(&[
                    "--features", "pddbtest",
                    "--features", "ditherpunk",
                    "--features", "tracking-alloc",
                    "--features", "tls",
                    // "--features", "test-rekey",
                ]), false)?
        }
        Some("hosted-ci") => {
            let mut pkgs = hw_pkgs.to_vec();
            let mut apps: Vec<String> = get_packages();
            apps.push("ball".to_string());
            apps.push("repl".to_string());
            for app in &apps {
                pkgs.push(app);
            }
            generate_app_menus(&apps);
            run(false, &pkgs, None, true)?
        }
        Some("debug") => {
            let mut args = env::args();
            args.nth(1);
            let mut pkgs = hw_pkgs.to_vec();
            let mut apps: Vec<String> = get_packages();
            if apps.len() == 0 {
                // add the standard demo apps if none are specified
                println!("No apps specified, adding default apps...");
                apps.push("ball".to_string());
                apps.push("repl".to_string());
            }
            for app in &apps {
                pkgs.push(app);
            }
            generate_app_menus(&apps);
            run(true, &pkgs, None, false)?
        }
        Some("app-image") => {
            let mut args = env::args();
            args.nth(1);
            let mut pkgs = hw_pkgs.to_vec();
            let apps = get_packages();
            for app in &apps {
                pkgs.push(app);
            }
            generate_app_menus(&apps);
            build_hw_image(
                false,
                None,
                &pkgs,
                lkey,
                kkey,
                None,
                // Some(&["--features", "ditherpunk"]), // swap for extra_args if you want ditherpunk in the app-image
                &[],
                None, None,
            )?
        }
        Some("perf-image") => {
            // note: to use this image, you need to load a version of the SOC that has the performance counters built in.
            // this can be generated using the command `python3 .\betrusted_soc.py -e .\dummy.nky --perfcounter` in the betrusted-soc repo.
            //
            // to read out performance monitoring data, use the `usb_update.py` script as follows:
            // ` python3 .\..\usb_update.py --dump v2p.txt --dump-file .\ring_aes_8.bin`
            // where the `v2p.txt` file contains a virtual to physical mapping that is generated by the `perflib` framework and
            // formatted in a fashion that can be automatically extracted by the usb_update script.
            let mut args = env::args();
            args.nth(1);
            let mut pkgs = hw_pkgs.to_vec();
            let apps = get_packages();
            for app in &apps {
                pkgs.push(app);
            }
            generate_app_menus(&apps);
            build_hw_image(
                false,
                Some("precursor-c809403-perflib".to_string()), // this is the name of the *feature* flag to utralib
                &pkgs,
                lkey,
                kkey,
                Some(&[
                    "--features", "perfcounter",
                ]),
                &[],
                None,
                Some(&[
                    "--features", "v2p",
                ]),
            )?
        }
        Some("gfx-dev") => run(
            true,
            &gfx_dev_pkgs,
            Some(&["--features", "graphics-server/testing"]),
            false,
        )?,
        Some("pddb-dev") => build_hw_image(
            false,
            env::args().nth(2),
            &pddb_dev_pkgs,
            lkey,
            kkey,
            None,
            &[],
            None, None,
        )?,
        Some("pddb-hosted") => run(false, &pddb_dev_pkgs, None, false)?,
        Some("minimal") => build_hw_image(
            false,
            env::args().nth(2),
            &minimal_pkgs,
            lkey,
            kkey,
            None,
            &[],
            None, None,
        )?,
        Some("trng-test") => {
            generate_app_menus(&Vec::<String>::new());
            build_hw_image(
                false,
                env::args().nth(2),
                &hw_pkgs,
                lkey,
                kkey,
                Some(&["--features", "urandomtest"]),
                &[],
                None, None,
            )?
        }
        Some("ro-test") => {
            generate_app_menus(&Vec::<String>::new());
            build_hw_image(
                false,
                env::args().nth(2),
                &hw_pkgs,
                lkey,
                kkey,
                Some(&["--features", "ringosctest"]),
                &[],
                None, None,
            )?
        }
        Some("av-test") => {
            generate_app_menus(&Vec::<String>::new());
            build_hw_image(
                false,
                env::args().nth(2),
                &hw_pkgs,
                lkey,
                kkey,
                Some(&["--features", "avalanchetest"]),
                &[],
                None, None,
            )?
        }
        Some("generate-locales") => generate_locales()?,
        Some("wycheproof-import") => whycheproof_import()?,
        _ => print_help(),
    }
    Ok(())
}

fn print_help() {
    eprintln!(
        "Tasks:
Hardware images:
 app-image [app1] [..]   builds an image for real hardware of baseline kernel + specified apps
 perf-image [app1] [..]  builds an image for real hardware assuming a performance counter variant of the SOC.

Hosted emulation:
 run [app1] [..]         runs a release build using a hosted environment plus specified apps

Renode emulation:
 renode-image            builds a functional image for renode
 renode-test             builds a test image for renode
 renode-image-debug      builds a test image for renode in debug mode
 libstd-test [pkg1] [..] builds a test image that includes the minimum packages, plus those
                         specified on the command line (e.g. built externally). Bypasses sig checks, keys locked out.
 libstd-net [pkg1] [..]  builds a test image for testing network functions. Bypasses sig checks, keys locked out.

Locale (re-)generation:
 generate-locales        (re)generate the locales include for the language selected in xous-rs/src/locale.rs

Various debug configurations (high chance of bitrot):
 tts                     builds an image with text to speech support via externally linked C executable
 wycheproof-import       generate binary test vectors for engine-25519 from whycheproof-import/x25519.json
 debug                   runs a debug build using a hosted environment
 minimal                 builds a minimal image for API testing
 ffi-test                builds an image for testing C-FFI bindings and integration
 gfx-dev                 minimal configuration for graphics primitive testing
 pddb-dev                PDDB testing only for live hardware
 pddb-hosted             PDDB testing in a hosted environment
 pddb-ci                 PDDB config for CI testing (eg: TRNG->deterministic for reproducible errors)
 usbdev                  minimal, insecure build for new USB core bringup
 trng-test               builds an image for TRNG testing - urandom source seeded by TRNG+AV
 ro-test                 builds an image for ring oscillator only TRNG testing
 av-test                 builds an image for avalanche generater only TRNG testing
 install-toolkit         installs Xous toolkit with no prompt, useful in CI. Specify `--force` to remove existing toolchains

Note: By default, the `ticktimer` will get rebuilt every time. You can skip this by appending `--no-timestamp` to the command.
"
    )
}

fn build_hw_image(
    debug: bool,
    svd: Option<String>,
    packages: &[&str],
    lkey: Option<String>,
    kkey: Option<String>,
    extra_args: Option<&[&str]>,
    extra_packages: &[&str],
    loader_features: Option<&[&str]>,
    kernel_features: Option<&[&str]>,
) -> Result<(), DynError> {
    // ------ configure UTRA generation feature flags ------
    // note: once we switch over to hosted/precursor as first-class flags, the ["--features", "precursor"] should be added here
    // for now that throws an error because we don't use that flag anywhere.
    let mut svd_feat = if let Some(spec) = &svd {
        if spec.contains("renode") {
            vec!["--features"]
        } else {
            vec!["--features", "utralib/precursor", "--features"]
        }
    } else {
        vec!["--features", "utralib/precursor", "--features"]
    };
    let mut svd_path = String::from("utralib/");
    let svd_filename: String;
    match svd {
        Some(s) => {
            svd_path.push_str(&s);
            svd_filename = s.to_string();
            svd_feat.push(&svd_path);
        },
        None => {
            svd_path.push_str(SOC_SVD_VERSION);
            svd_filename = SOC_SVD_VERSION.to_string();
            svd_feat.push(&svd_path);
        },
    };

    // LAST_CONFIG tracks the last SVD configuration. It's used by utralib to track if it
    // should rebuild itself based on a change in SVD configs. Note that for some reason
    // it takes two consecutive builds with the same SVD config before the build system
    // figures out that it doesn't need to rebuild everything. After then, it behaves as expected.
    let stream = if debug { "debug" } else { "release" };
    let last_config = format!("target/{}/{}/build/LAST_CONFIG", PROGRAM_TARGET, stream);
    std::fs::create_dir_all(format!("target/{}/{}/build/", PROGRAM_TARGET, stream)).unwrap();
    let changed = match OpenOptions::new()
        .read(true)
        .open(&last_config) {
        Ok(mut file) => {
            let mut contents = String::new();
            file.read_to_string(&mut contents).unwrap();
            if contents != svd_filename {
                true
            } else {
                false
            }
        }
        _ => true
    };
    if changed {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&last_config).unwrap();
        write!(file, "{}", svd_filename).unwrap();
    }

    // concatenate the passed in feature flags with the computed utra feature flags
    let mut kernel_features_finalized = Vec::<&str>::new();
    let mut loader_features_finalized = Vec::<&str>::new();
    for &s in svd_feat.iter() {
        kernel_features_finalized.push(s);
        loader_features_finalized.push(s);
    }
    if let Some(lf) = loader_features {
        for &s in lf {
            loader_features_finalized.push(s);
        }
    }
    if let Some(kf) = kernel_features {
        for &s in kf {
            kernel_features_finalized.push(s);
        }
    }

    // std::env::set_var("RUST_LOG", "debug"); // set this to debug the image creation process

    // extract key file names; replace with defaults if not specified
    let loaderkey_file = lkey.unwrap_or_else(|| "devkey/dev.key".into());
    let kernelkey_file = kkey.unwrap_or_else(|| "devkey/dev.key".into());

    // ------ build the kernel ------
    let kernel = build_kernel(debug, Some(&kernel_features_finalized))?;

    // ------ build the services ------
    svd_feat.push("--features");
    svd_feat.push("utralib/std");
    let mut init = vec![];
    let base_path = build(
        packages,
        debug,
        Some(PROGRAM_TARGET),
        None,
        extra_args,
        Some(&svd_feat),
    )?;
    for pkg in packages {
        let mut pkg_path = base_path.clone();
        // some packages may have a colon-delimited version after it to clarify crates.io patches.
        // Strip off the version number before passing to the image builder.
        let pkg_maybe_version: Vec<&str> = pkg.split(':').collect();
        let pkg_root = if pkg_maybe_version.len() > 1 {
            pkg_maybe_version[pkg_maybe_version.len() - 2]
        } else {
            pkg
        };
        pkg_path.push(pkg_root);
        init.push(pkg_path);
    }
    for pkg in extra_packages {
        let mut pkg_path = project_root();
        pkg_path.push(pkg);
        init.push(pkg_path);
    }

    // ------ build the loader ------
    // stash any LTO settings applied to the kernel; proper layout of the loader
    // block depends on the loader being compact and highly optimized.
    let existing_lto = std::env::var("CARGO_PROFILE_RELEASE_LTO")
        .map(|v| Some(v))
        .unwrap_or(None);
    let existing_codegen_units = std::env::var("CARGO_PROFILE_RELEASE_CODEGEN_UNITS")
        .map(|v| Some(v))
        .unwrap_or(None);
    // these settings will generate the most compact code (but also the hardest to debug)
    std::env::set_var("CARGO_PROFILE_RELEASE_LTO", "true");
    std::env::set_var("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "1");
    let mut loader = build(
        &["loader"],
        debug,
        Some(KERNEL_TARGET),
        None,
        None,
        Some(&loader_features_finalized),
    )?;
    // restore the LTO settings
    if let Some(existing) = existing_lto {
        std::env::set_var("CARGO_PROFILE_RELEASE_LTO", existing);
    }
    if let Some(existing) = existing_codegen_units {
        std::env::set_var("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", existing);
    }
    loader.push(PathBuf::from("loader"));

    // ---------- extract SVD file path, as computed by utralib ----------
    let svd_spec_path = format!("target/{}/{}/build/SVD_PATH", PROGRAM_TARGET, stream);
    let mut svd_spec_file = OpenOptions::new()
        .read(true)
        .open(&svd_spec_path)?;
    let mut svd_path = String::new();
    svd_spec_file.read_to_string(&mut svd_path)?;

    // --------- package up and sign a binary image ----------
    let output_bundle = create_image(&kernel, &init, debug,
        MemorySpec::SvdFile(svd_path)
    )?;
    println!();
    println!(
        "Kernel+Init bundle is available at {}",
        output_bundle.display()
    );

    let mut loader_bin = output_bundle.parent().unwrap().to_owned();
    loader_bin.push("loader.bin");
    let mut loader_presign = output_bundle.parent().unwrap().to_owned();
    loader_presign.push("loader_presign.bin");
    let status = Command::new(cargo())
        .current_dir(project_root())
        .args(&[
            "run",
            "--package",
            "tools",
            "--bin",
            "copy-object",
            "--",
            loader.as_os_str().to_str().unwrap(),
            loader_presign.as_os_str().to_str().unwrap(),
        ])
        .status()?;
    if !status.success() {
        return Err("cargo build failed".into());
    }

    let status = Command::new(cargo())
        .current_dir(project_root())
        .args(&[
            "run",
            "--package",
            "tools",
            "--bin",
            "sign-image",
            "--",
            "--loader-image",
            loader_presign.to_str().unwrap(),
            "--loader-key",
            loaderkey_file.as_str(),
            "--loader-output",
            loader_bin.to_str().unwrap(),
            "--min-xous-ver",
            MIN_XOUS_VERSION,
        ])
        .status()?;
    if !status.success() {
        return Err("loader image sign failed".into());
    }

    let mut xous_img_path = output_bundle.parent().unwrap().to_owned();
    let mut xous_img_presign_path = xous_img_path.clone();
    xous_img_path.push("xous.img");
    xous_img_presign_path.push("xous_presign.img");
    let mut xous_img =
        std::fs::File::create(&xous_img_presign_path).expect("couldn't create xous.img");
    let mut bundle_file = std::fs::File::open(output_bundle).expect("couldn't open output bundle");
    let mut buf = vec![];
    bundle_file
        .read_to_end(&mut buf)
        .expect("couldn't read output bundle file");
    xous_img
        .write_all(&buf)
        .expect("couldn't write bundle file to xous.img");
    println!("Bundled image file created at {}", xous_img_path.display());

    let status = Command::new(cargo())
        .current_dir(project_root())
        .args(&[
            "run",
            "--package",
            "tools",
            "--bin",
            "sign-image",
            "--",
            "--kernel-image",
            xous_img_presign_path.to_str().unwrap(),
            "--kernel-key",
            kernelkey_file.as_str(),
            "--kernel-output",
            xous_img_path.to_str().unwrap(),
            "--min-xous-ver",
            MIN_XOUS_VERSION,
            // "--defile",
        ])
        .status()?;
    if !status.success() {
        return Err("kernel image sign failed".into());
    }

    println!();
    println!("Signed loader at {}", loader_bin.display());
    println!("Signed kernel at {}", xous_img_path.display());

    Ok(())
}

fn renode_image(
    debug: bool,
    packages: &[&str],
    extra_packages: &[&str],
    xous_features: Option<&[&str]>,
    loader_features: Option<&[&str]>,
) -> Result<(), DynError> {
    // Regenerate the Platform file
    let status = Command::new(cargo())
        .current_dir(project_root())
        .args(&[
            "run",
            "-p",
            "svd2repl",
            "--",
            "-i",
            "utralib/renode/renode.svd",
            "-o",
            "emulation/soc/betrusted-soc.repl",
        ])
        .status()?;
    if !status.success() {
        return Err("Unable to regenerate Renode platform file".into());
    }
    build_hw_image(
        debug,
        Some("renode".to_string()),
        packages,
        None,
        None,
        xous_features,
        extra_packages,
        loader_features,
        None,
    )
}

fn run(
    debug: bool,
    init: &[&str],
    features: Option<&[&str]>,
    dry_run: bool,
) -> Result<(), DynError> {
    let stream = if debug { "debug" } else { "release" };

    build(init, debug, None, None, None, features)?;

    // Build and run the kernel
    let mut args = vec!["run"];
    if !debug {
        args.push("--release");
    }

    args.push("--");

    let mut paths = vec![];
    for i in init {
        let tmp: PathBuf = if cfg!(windows) {
            Path::new(&format!(
                "..{}target{}{}{}{}.exe",
                MAIN_SEPARATOR, MAIN_SEPARATOR, stream, MAIN_SEPARATOR, i
            ))
            .to_owned()
        } else {
            Path::new(&format!(
                "..{}target{}{}{}{}",
                MAIN_SEPARATOR, MAIN_SEPARATOR, stream, MAIN_SEPARATOR, i
            ))
            .to_owned()
        };
        paths.push(tmp);
    }
    for t in &paths {
        args.push(t.to_str().ok_or(BuildError::PathConversionError)?);
    }

    let mut dir = project_root();
    dir.push("kernel");

    if !dry_run {
        println!("Building and running kernel...");
        print!("    Command: cargo");
        for arg in &args {
            print!(" {}", arg);
        }
        println!();
        let status = Command::new(cargo())
            .current_dir(dir)
            .args(&args)
            .status()?;
        if !status.success() {
            return Err("cargo build failed".into());
        }
    }

    Ok(())
}

fn build_kernel(debug: bool, features: Option<&[&str]>) -> Result<PathBuf, DynError> {
    let mut path = build(&["xous-kernel"], debug, Some(KERNEL_TARGET), None, None, features)?;
    path.push("xous-kernel");
    Ok(path)

    /*
    // cargo install --target riscv32imac-unknown-xous-elf --target-dir target xous-kernel --version 0.9.0 --features utralib/precursor-c809403 --root .\target\riscv32imac-unknown-xous-elf\release\
    // cargo install --list --root .\target\riscv32imac-unknown-xous-elf\release\
    // drop the --target spec and you'll get hosted mode binaries.
    let stream = if debug { "debug" } else { "release" };
    let mut dir = project_root();
    let mut args = vec!["install"];
    let target_path = format!("target/{}/{}/", KERNEL_TARGET, stream);
    args.push("--root");
    args.push(&target_path);
    args.push("--target");
    args.push(KERNEL_TARGET);
    args.push("xous-kernel");
    args.push("--version");
    args.push("0.9.0");
    args.push("--features");
    args.push("utralib/precursor-c809403");

    let status = Command::new(cargo())
        .current_dir(dir)
        .args(&args)
        .status()?;

    if !status.success() {
        return Err("cargo build failed".into());
    }
    Ok(project_root().join(&target_path).join("bin/xous-kernel"))
    //Ok(PathBuf::from_str("C:\\Users\\bunnie\\.cargo\\bin\\xous-kernel").unwrap())
    */
}


fn build(
    packages: &[&str],
    debug: bool,
    target: Option<&str>,
    directory: Option<PathBuf>,
    extra_args: Option<&[&str]>,
    features: Option<&[&str]>,
) -> Result<PathBuf, DynError> {
    ensure_compiler(&target, false, false)?;
    let stream = if debug { "debug" } else { "release" };
    let mut args = vec!["build"];
    print!("Building");

    if let Some(feature) = features {
        for &a in feature {
            args.push(a);
        }
    }
    for package in packages {
        print!(" {}", package);
        args.push("--package");
        args.push(package);
    }
    println!();
    let mut target_path = "".to_owned();
    if let Some(t) = target {
        args.push("--target");
        args.push(t);
        target_path = format!("{}/", t);
    }

    if !debug {
        args.push("--release");
    }

    if let Some(extra) = extra_args {
        for &a in extra {
            args.push(a);
        }
    }

    let mut dir = project_root();
    if let Some(subdir) = &directory {
        dir.push(subdir);
    }

    print!("    Command: cargo");
    for arg in &args {
        print!(" {}", arg);
    }
    println!();
    let status = Command::new(cargo())
        .current_dir(dir)
        .args(&args)
        .status()?;

    if !status.success() {
        return Err("cargo build failed".into());
    }

    if let Some(base_dir) = &directory {
        Ok(project_root().join(&format!(
            "{}/target/{}{}/",
            base_dir.to_str().ok_or(BuildError::PathConversionError)?,
            target_path,
            stream,
        )))
    } else {
        Ok(project_root().join(&format!("target/{}{}/", target_path, stream)))
    }
}

fn create_image(
    kernel: &Path,
    init: &[PathBuf],
    debug: bool,
    memory_spec: MemorySpec,
) -> Result<PathBuf, DynError> {
    let stream = if debug { "debug" } else { "release" };
    let mut args = vec!["run", "--package", "tools", "--bin", "create-image", "--"];

    let output_file = format!("target/{}/{}/args.bin", PROGRAM_TARGET, stream);
    args.push(&output_file);

    args.push("--kernel");
    args.push(kernel.to_str().ok_or(BuildError::PathConversionError)?);

    for i in init {
        args.push("--init");
        args.push(i.to_str().ok_or(BuildError::PathConversionError)?);
    }

    match memory_spec {
        MemorySpec::SvdFile(ref s) => {
            args.push("--svd");
            args.push(s);
        }
    }

    let status = Command::new(cargo())
        .current_dir(project_root())
        .args(&args)
        .status()?;

    if !status.success() {
        return Err("cargo build failed".into());
    }
    Ok(project_root().join(&format!("target/{}/{}/args.bin", PROGRAM_TARGET, stream)))
}
