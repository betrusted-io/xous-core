use chrono::Local;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as StdWrite;
use std::fs::OpenOptions;
use std::{
    env,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf, MAIN_SEPARATOR},
    process::Command,
};

type DynError = Box<dyn std::error::Error>;

const PROGRAM_TARGET: &str = "riscv32imac-unknown-xous-elf";
const KERNEL_TARGET: &str = "riscv32imac-unknown-xous-elf";
const TOOLCHAIN_URL_PREFIX: &str =
    "https://github.com/betrusted-io/rust/releases/latest/download/riscv32imac-unknown-xous_";
const TOOLCHAIN_URL_SUFFIX: &str = ".zip";

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    generate_version(); // really brute force way to try and get a version into the build system.

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
        "sha2:0.9.8",
        "engine-25519",
        "spinor",
        "root-keys",
        "jtag",
        "net",
        "dns",
        "pddb",
        "modals",
    ];
    let app_pkgs = [
        // "standard" demo apps
        "ball", "repl",
    ];
    let benchmark_pkgs = [
        "benchmark",
        "benchmark-target",
        "graphics-server",
        "ticktimer-server",
        "log-server",
        "xous-names",
        "keyboard",
        "trng",
        "susres",
        "llio",
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
    let cbtest_pkgs = [
        "ticktimer-server",
        "log-server",
        "xous-names",
        "trng",
        "llio",
        "cb-test-srv",
        "cb-test-c1",
        "cb-test-c2",
        "susres",
    ];
    let sr_pkgs = [
        "ticktimer-server",
        "log-server",
        "xous-names",
        "trng",
        "llio",
        "rkyv-test-client",
        "rkyv-test-server",
        "com",
        "susres",
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
    let lkey = args.nth(3);
    let kkey = args.nth(4);
    match task.as_deref() {
        Some("install-toolkit") => {
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
            let mut apps: Vec<String> = args.collect();
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
            renode_image(
                false,
                &hw_pkgs,
                &[],
                None,
                Some(&["--features", "renode-bypass"]),
            )?
        }
        Some("renode-test") => {
            let mut args = env::args();
            args.nth(1);
            let mut pkgs = hw_pkgs.to_vec();
            let mut apps: Vec<String> = args.collect();
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
            renode_image(false, &cbtest_pkgs, &[], None, None)?
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
            let apps: Vec<String> = args.collect();
            for app in &apps {
                pkgs.push(app);
            }
            pkgs.push("tts-frontend");
            pkgs.push("ime-plugin-tts");
            pkgs.retain(|&pkg| pkg != "ime-plugin-shell");
            generate_app_menus(&apps);
            build_hw_image(
                false,
                Some("./precursors/soc.svd".to_string()),
                &pkgs,
                None,
                None,
                Some(&["--features", "tts", "--features", "braille"]),
                &[&tts_exec_string],
                None,
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
        Some("libstd-test") => {
            let mut args = env::args();
            args.nth(1);
            let pkgs = base_pkgs.to_vec();
            let args: Vec<String> = args.collect();
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
            let args: Vec<String> = args.collect();
            let mut extra_packages = vec![];
            for program in &args {
                extra_packages.push(program.as_str());
            }
            build_hw_image(
                false,
                Some("./precursors/soc.svd".to_string()),
                &pkgs,
                None,
                None,
                None,
                &[],
                Some(&["--features", "renode-bypass"]),
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
            let args: Vec<String> = args.collect();
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
        Some("run") => {
            let mut args = env::args();
            args.nth(1);
            let mut pkgs = hw_pkgs.to_vec();
            let mut apps: Vec<String> = args.collect();
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
            run(false, &pkgs, None, false)?
        }
        Some("hosted-ci") => {
            let mut pkgs = hw_pkgs.to_vec();
            let mut apps: Vec<String> = args.collect();
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
            let mut apps: Vec<String> = args.collect();
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
            let apps: Vec<String> = args.collect();
            for app in &apps {
                pkgs.push(app);
            }
            generate_app_menus(&apps);
            build_hw_image(
                false,
                Some("./precursors/soc.svd".to_string()),
                &pkgs,
                lkey,
                kkey,
                None,
                &[],
                None,
            )?
        }
        Some("hw-image") => {
            let mut pkgs = vec![];
            for pkg in hw_pkgs {
                pkgs.push(pkg);
            }
            for app in app_pkgs {
                pkgs.push(app);
            }
            let mut app_strs = Vec::<String>::new();
            for app in app_pkgs {
                app_strs.push(app.to_string());
            }
            generate_app_menus(&app_strs);
            build_hw_image(
                false,
                env::args().nth(2),
                &pkgs,
                lkey,
                kkey,
                None,
                &[],
                None,
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
            None,
        )?,
        Some("pddb-hosted") => run(false, &pddb_dev_pkgs, None, false)?,
        Some("benchmark") => build_hw_image(
            false,
            env::args().nth(2),
            &benchmark_pkgs,
            lkey,
            kkey,
            None,
            &[],
            None,
        )?,
        Some("minimal") => build_hw_image(
            false,
            env::args().nth(2),
            &minimal_pkgs,
            lkey,
            kkey,
            None,
            &[],
            None,
        )?,
        Some("cbtest") => build_hw_image(
            false,
            env::args().nth(2),
            &cbtest_pkgs,
            lkey,
            kkey,
            None,
            &[],
            None,
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
                None,
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
                None,
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
                None,
            )?
        }
        Some("sr-test") => build_hw_image(
            false,
            env::args().nth(2),
            &sr_pkgs,
            lkey,
            kkey,
            None,
            &[],
            None,
        )?,
        Some("burn-kernel") => update_usb(true, false, false, false)?,
        Some("burn-loader") => update_usb(false, true, false, false)?,
        Some("nuke-soc") => update_usb(false, false, true, false)?,
        Some("burn-soc") => update_usb(false, false, false, true)?,
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
 hw-image [soc.svd]      builds an image for real hardware with baseline demo apps
          [loader.key]   plus signing key options
          [kernel.key]
 app-image [app1] [..]   builds an image for real hardware of baseline kernel + specified apps

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

Direct USB updates:
 ** Please refer to tools/README_UPDATE.md for instructions on how to set up `usb_update.py` **
 burn-kernel             invoke the `usb_update.py` utility to burn the kernel
 burn-loader             invoke the `usb_update.py` utility to burn the loader
 burn-soc                invoke the `usb_update.py` utility to stage the SoC gateware, which must then be provisioned with secret material using the Precursor device.
 nuke-soc                'Factory reset' - invoke the `usb_update.py` utility to burn the SoC gateware, erasing most secrets. For developers.

Various debug configurations:
 debug                   runs a debug build using a hosted environment
 benchmark [soc.svd]     builds a benchmarking image for real hardware
 minimal [soc.svd]       builds a minimal image for API testing
 cbtest                  builds an image for callback testing
 trng-test [soc.svd]     builds an image for TRNG testing - urandom source seeded by TRNG+AV
 ro-test [soc.svd]       builds an image for ring oscillator only TRNG testing
 av-test [soc.svd]       builds an image for avalanche generater only TRNG testing
 sr-test [soc.svd]       builds the suspend/resume testing image
 wycheproof-import       generate binary test vectors for engine-25519 from whycheproof-import/x25519.json
 pddb-dev                PDDB testing only for live hardware
 pddb-hosted             PDDB testing in a hosted environment
 pddb-ci                 PDDB config for CI testing (eg: TRNG->deterministic for reproducible errors)
 ffi-test                builds an image for testing C-FFI bindings and integration
 tts                     builds an image with text to speech support via externally linked C executable
 install-toolkit         installs Xous toolkit with no prompt, useful in CI. Specify `--force` to remove existing toolchains
"
    )
}

fn update_usb(
    do_kernel: bool,
    do_loader: bool,
    nuke_soc: bool,
    stage_soc: bool,
) -> Result<(), DynError> {
    use std::io::{BufRead, BufReader, Error, ErrorKind};
    use std::process::Stdio;

    if do_kernel {
        println!("Burning kernel. After this is done, you must select 'Sign xous update' to self-sign the image.");
        let stdout = if cfg!(target_os = "windows") {
            Command::new("cmd")
                .args([
                    "/C",
                    "python",
                    "tools/usb_update.py",
                    "-k",
                    "target/riscv32imac-unknown-xous-elf/release/xous.img",
                ])
                .stdout(Stdio::piped())
                .spawn()?
                .stdout
                .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture output"))?
        } else {
            Command::new("python3")
                .arg("tools/usb_update.py")
                .arg("-k")
                .arg("target/riscv32imac-unknown-xous-elf/release/xous.img")
                .stdout(Stdio::piped())
                .spawn()?
                .stdout
                .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture output"))?
        };

        let reader = BufReader::new(stdout);
        reader
            .lines()
            .for_each(|line| println!("{}", line.unwrap()));
    }
    if do_loader {
        println!("Burning loader. After this is done, you must select 'Sign xous update' to self-sign the image.");
        let stdout = if cfg!(target_os = "windows") {
            Command::new("cmd")
                .args([
                    "/C",
                    "python",
                    "tools/usb_update.py",
                    "-l",
                    "target/riscv32imac-unknown-xous-elf/release/loader.bin",
                ])
                .stdout(Stdio::piped())
                .spawn()?
                .stdout
                .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture output"))?
        } else {
            Command::new("python3")
                .arg("tools/usb_update.py")
                .arg("-l")
                .arg("target/riscv32imac-unknown-xous-elf/release/loader.bin")
                .stdout(Stdio::piped())
                .spawn()?
                .stdout
                .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture output"))?
        };

        let reader = BufReader::new(stdout);
        reader
            .lines()
            .for_each(|line| println!("{}", line.unwrap()));
    }
    if stage_soc {
        println!("Staging SoC gateware. After this is done, you must select 'Install Gateware Update' from the root menu of your Precursor device.");
        let stdout = if cfg!(target_os = "windows") {
            Command::new("cmd")
                .args([
                    "/C",
                    "python",
                    "tools/usb_update.py",
                    "-s",
                    "precursors/soc_csr.bin",
                ])
                .stdout(Stdio::piped())
                .spawn()?
                .stdout
                .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture output"))?
        } else {
            Command::new("python3")
                .arg("tools/usb_update.py")
                .arg("-s")
                .arg("precursors/soc_csr.bin")
                .stdout(Stdio::piped())
                .spawn()?
                .stdout
                .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture output"))?
        };

        let reader = BufReader::new(stdout);
        reader
            .lines()
            .for_each(|line| println!("{}", line.unwrap()));
    }
    if nuke_soc {
        println!("Installing factory-reset SoC gateware (secrets will be lost)!");
        let stdout = if cfg!(traget_os = "windows") {
            Command::new("cmd")
                .args([
                    "/C",
                    "python",
                    "tools/usb_update.py",
                    "--soc",
                    "precursors/soc_csr.bin",
                ])
                .stdout(Stdio::piped())
                .spawn()?
                .stdout
                .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture output"))?
        } else {
            Command::new("python3")
                .arg("tools/usb_update.py")
                .arg("--soc")
                .arg("precursors/soc_csr.bin")
                .stdout(Stdio::piped())
                .spawn()?
                .stdout
                .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture output"))?
        };
        let reader = BufReader::new(stdout);
        reader
            .lines()
            .for_each(|line| println!("{}", line.unwrap()));
    }

    Ok(())
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
) -> Result<(), DynError> {
    let svd_file = match svd {
        Some(s) => s,
        None => return Err("svd file not specified".into()),
    };

    let path = std::path::Path::new(&svd_file);
    if !path.exists() {
        return Err("svd file does not exist".into());
    }

    // Tools use this environment variable to know when to rebuild the UTRA crate.
    std::env::set_var("XOUS_SVD_FILE", path.canonicalize().unwrap());
    println!("XOUS_SVD_FILE: {}", path.canonicalize().unwrap().display());
    // std::env::set_var("RUST_LOG", "debug"); // set this to debug the image creation process

    // extract key file names; replace with defaults if not specified
    let loaderkey_file = lkey.unwrap_or_else(|| "devkey/dev.key".into());
    let kernelkey_file = kkey.unwrap_or_else(|| "devkey/dev.key".into());

    let kernel = build_kernel(debug)?;
    let mut init = vec![];
    let base_path = build(
        packages,
        debug,
        Some(PROGRAM_TARGET),
        None,
        extra_args,
        None,
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
    let mut loader = build(
        &["loader"],
        debug,
        Some(KERNEL_TARGET),
        Some("loader".into()),
        None,
        loader_features,
    )?;
    loader.push(PathBuf::from("loader"));

    let output_bundle = create_image(&kernel, &init, debug, MemorySpec::SvdFile(svd_file))?;
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

/*
fn sign_loader(in_path: Pathbuf, out_path: Pathbuf) -> Result<(), DynError> {
    let mut in_file = File::open(in_path)?;
    let mut out_file = File::open(out_path)?;

    let mut loader = Vec::<u8>::new();
    in_file.read_to_end(&mut loader);


}*/

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
            "emulation/soc/renode.svd",
            "-o",
            "emulation/soc/betrusted-soc.repl",
        ])
        .status()?;
    if !status.success() {
        return Err("Unable to regenerate Renode platform file".into());
    }
    build_hw_image(
        debug,
        Some("emulation/soc/renode.svd".to_owned()),
        packages,
        None,
        None,
        xous_features,
        extra_packages,
        loader_features,
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
        let tmp: PathBuf = Path::new(&format!(
            "..{}target{}{}{}{}",
            MAIN_SEPARATOR, MAIN_SEPARATOR, stream, MAIN_SEPARATOR, i
        ))
        .to_owned();
        // .canonicalize()
        // .or(Err(BuildError::PathConversionError))?;
        paths.push(tmp);
    }
    for t in &paths {
        args.push(t.to_str().ok_or(BuildError::PathConversionError)?);
    }

    let mut dir = project_root();
    dir.push("kernel");

    if !dry_run {
        println!("Building and running kernel...");
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

fn build_kernel(debug: bool) -> Result<PathBuf, DynError> {
    let mut path = build(&["kernel"], debug, Some(KERNEL_TARGET), None, None, None)?;
    path.push("kernel");
    Ok(path)
}

/// Since we use the same TARGET for all calls to `build()`,
/// cache it inside an atomic boolean. If this is `true` then
/// it means we can assume the check passed already.
static DONE_COMPILER_CHECK: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Ensure we have a compatible compiler toolchain. We use a new Target,
/// and we want to give the user a friendly way of installing the latest
/// Rust toolchain.
fn ensure_compiler(
    target: &Option<&str>,
    force_install: bool,
    remove_existing: bool,
) -> Result<(), String> {
    use std::process::Stdio;
    if DONE_COMPILER_CHECK.load(std::sync::atomic::Ordering::SeqCst) {
        return Ok(());
    }

    /// Return the sysroot for the given target. If the target does not exist,
    /// return None.
    fn get_sysroot(target: Option<&str>) -> Result<Option<String>, String> {
        let mut args = vec!["--print", "sysroot"];
        if let Some(target) = target {
            args.push("--target");
            args.push(target);
        }

        let sysroot_cmd = Command::new("rustc")
            .stderr(Stdio::null())
            .stdout(Stdio::piped())
            .args(&args)
            .spawn()
            .expect("could not run rustc");
        let sysroot_output = sysroot_cmd.wait_with_output().unwrap();
        let have_toolchain = sysroot_output.status.success();

        let toolchain_path = String::from_utf8(sysroot_output.stdout)
            .map_err(|_| "Unable to find Rust sysroot".to_owned())?
            .trim()
            .to_owned();

        // Look for the "RUST_VERSION" file to ensure it's compatible with this version.
        if let Some(target) = target {
            let mut version_path = PathBuf::from(&toolchain_path);
            version_path.push("lib");
            version_path.push("rustlib");
            version_path.push(target);
            version_path.push("RUST_VERSION");
            if let Ok(mut vp) = File::open(&version_path) {
                let mut version_str = String::new();
                if let Err(_) = vp.read_to_string(&mut version_str) {
                    return Err("Unable to get version string".to_owned());
                }

                let rustc_version_str = format!("{}", rustc_version::version().unwrap());
                if version_str.trim() != rustc_version_str.trim() {
                    println!("Version upgrade. Compiler is version {}, the installed toolchain is for {}", version_str.trim(), rustc_version_str.trim());
                    // return Err(format!("Version upgrade. Compiler is version {}, the installed toolchain is for {}", version_str, rustc_version_str));
                    return Ok(None);
                }
            } else {
                println!("Outdated toolchain installed.");
                // return Err("Outdated toolchain installed".to_owned());
                return Ok(None);
            }
        }

        if have_toolchain {
            Ok(Some(toolchain_path))
        } else {
            Ok(None)
        }
    }

    // If the sysroot exists, then we're good.
    let target = target.unwrap_or(PROGRAM_TARGET);
    if let Some(path) = get_sysroot(Some(target))? {
        let mut version_path = PathBuf::from(&path);
        version_path.push("lib");
        version_path.push("rustlib");
        version_path.push(PROGRAM_TARGET);
        if remove_existing {
            println!("Target path exists, removing it");
            std::fs::remove_dir_all(version_path)
                .or_else(|e| Err(format!("unable to remove existing toolchain: {}", e)))?;
            println!("Also removing target directories for existing toolchain");
            let mut target_main = project_root();
            target_main.push("target");
            target_main.push(PROGRAM_TARGET);
            std::fs::remove_dir_all(target_main).ok();

            let mut target_loader = project_root();
            target_loader.push("loader");
            target_loader.push("target");
            target_loader.push(PROGRAM_TARGET);
            std::fs::remove_dir_all(target_loader).ok();

        } else {
            DONE_COMPILER_CHECK.store(true, std::sync::atomic::Ordering::SeqCst);
            return Ok(());
        }
    }

    // Since no sysroot exists, we must download a new one.
    let toolchain_path =
        get_sysroot(None)?.ok_or_else(|| "default toolchain not installed".to_owned())?;
    // If the terminal is a tty, or if toolchain installation is forced,
    // download the latest toolchain.
    if !atty::is(atty::Stream::Stdin) && !force_install {
        return Err(format!("Toolchain for {} not found", target));
    }

    // Version 1.54 was the last major version that was released.
    let ver = rustc_version::version().unwrap();
    if ver.major == 1 && ver.minor < 54 {
        return Err("Rust 1.54 or higher is required".into());
    }

    // Ask the user if they want to install the toolchain.
    let mut buffer = String::new();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    println!();
    println!(
        "Error: Toolchain for {} was not found on this system!",
        target
    );
    loop {
        if force_install {
            break;
        }

        print!("Would you like this program to attempt to download and install it?   [Y/n] ");
        stdout.flush().unwrap();
        buffer.clear();
        stdin.read_line(&mut buffer).unwrap();

        let trimmed = buffer.trim();

        if trimmed == "n" || trimmed == "N" {
            return Err(format!("Please install the {} toolchain", target));
        }

        if trimmed == "y" || trimmed == "Y" || trimmed.is_empty() {
            break;
        }
        println!();
    }

    let toolchain_url = format!(
        "{}{}.{}.{}{}",
        TOOLCHAIN_URL_PREFIX, ver.major, ver.minor, ver.patch, TOOLCHAIN_URL_SUFFIX
    );

    println!(
        "Attempting to install toolchain for {} into {}",
        target, toolchain_path
    );
    println!("Downloading from {}...", toolchain_url);

    print!("Download rogress: 0%");
    stdout.flush().unwrap();
    let mut zip_data = vec![];
    {
        let mut easy = curl::easy::Easy::new();
        easy.url(&toolchain_url).unwrap();
        easy.follow_location(true).unwrap();
        easy.progress(true).unwrap();
        let mut transfer = easy.transfer();
        transfer
            .progress_function(
                |total_bytes, bytes_so_far, _total_uploaded, _uploaded_so_far| {
                    // If either number is infinite, don't print anything and just continue.
                    if total_bytes.is_infinite() || bytes_so_far.is_infinite() {
                        return true;
                    }

                    // Display progress.
                    print!(
                        "\rDownload progress: {:3.02}% ",
                        bytes_so_far / total_bytes * 100.0
                    );
                    stdout.flush().unwrap();

                    // Return `true` to continue the transfer.
                    true
                },
            )
            .unwrap();
        transfer
            .write_function(|data| {
                zip_data.extend_from_slice(data);
                Ok(data.len())
            })
            .unwrap();
        transfer
            .perform()
            .map_err(|e| format!("Unable to download toolchain: {}", e))?;
        println!();
    }
    println!(
        "Download successful. Total data size is {} bytes",
        zip_data.len()
    );

    /// Extract the zipfile to the target directory, ensuring that all files
    /// contained within are created.
    fn extract_zip<P: std::io::Read + std::io::Seek, P2: AsRef<Path>>(
        archive_data: P,
        extract_to: P2,
    ) -> Result<(), String> {
        let mut archive = zip::ZipArchive::new(archive_data)
            .map_err(|e| format!("unable to extract zip: {}", e))?;
        for i in 0..archive.len() {
            let mut entry_in_archive = archive
                .by_index(i)
                .map_err(|e| format!("unable to locate file index {}: {}", i, e))?;
            // println!(
            //     "Trying to extract file {}",
            //     entry_in_archive.mangled_name().display()
            // );

            let output_path = extract_to.as_ref().join(entry_in_archive.mangled_name());
            if entry_in_archive.is_dir() {
                std::fs::create_dir_all(&output_path).map_err(|e| {
                    format!(
                        "unable to create directory {}: {}",
                        output_path.display(),
                        e
                    )
                })?;
            } else {
                // Create the parent directory if necessary
                if let Some(parent) = output_path.parent() {
                    if !parent.exists() {
                        std::fs::create_dir_all(&parent).map_err(|e| {
                            format!(
                                "unable to create directory {}: {}",
                                output_path.display(),
                                e
                            )
                        })?;
                    }
                }
                let mut outfile = std::fs::File::create(&output_path).map_err(|e| {
                    format!("unable to create file {}: {}", output_path.display(), e)
                })?;
                std::io::copy(&mut entry_in_archive, &mut outfile).map_err(|e| {
                    format!(
                        "unable to write extracted file {}: {}",
                        output_path.display(),
                        e
                    )
                })?;
            }
        }
        Ok(())
    }
    println!("Extracting toolchain to {}...", toolchain_path);
    extract_zip(std::io::Cursor::new(zip_data), &toolchain_path)?;

    println!("Toolchain successfully installed");

    DONE_COMPILER_CHECK.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
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

/// Regenerate the locales files. This is only done when the command is explicitly run.
fn generate_locales() -> Result<(), std::io::Error> {
    let ts = filetime::FileTime::from_system_time(std::time::SystemTime::now());
    filetime::set_file_mtime("locales/src/lib.rs", ts)?;
    let mut path = project_root();
    path.push("locales");
    let status = Command::new(cargo())
        .current_dir(path)
        .args(&["build", "--package", "locales"])
        .status()?;
    if !status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Couldn't generate the locales",
        ));
    }
    return Ok(());
}

fn whycheproof_import() -> Result<(), DynError> {
    let input_file = "tools/wycheproof-import/x25519_test.json";
    let output_file = "services/shellchat/src/cmds/x25519_test.bin";
    let status = Command::new(cargo())
        .current_dir(project_root())
        .args(&[
            "run",
            "--package",
            "wycheproof-import",
            "--",
            input_file,
            output_file,
        ])
        .status()?;
    if !status.success() {
        return Err("wycheproof-import failed. If any, the output will not be usable.".into());
    }

    println!();
    println!("Wrote wycheproof x25519 testvectors to '{}'.", output_file);

    return Ok(());
}

////////////////////////// Versioning infrastructure
fn generate_version() {
    let output = if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(["/C", "git describe --tags"])
            .output()
            .expect("failed to execute process")
    } else {
        Command::new("sh")
            .arg("-c")
            .arg("git describe --tags")
            .output()
            .expect("failed to execute process")
    };
    let gitver = output.stdout;
    let semver = String::from_utf8_lossy(&gitver);

    let mut vfile = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open("services/ticktimer-server/src/version.rs")
        .expect("Can't open our version file for writing");
    print_header(&mut vfile);
    #[cfg(not(feature = "no-timestamp"))]
    let now = Local::now();
    #[cfg(not(feature = "no-timestamp"))]
    write!(
        vfile,
        "#[allow(dead_code)]\npub const TIMESTAMP: &'static str = \"{}\";\n",
        now.to_rfc2822()
    )
    .expect("couldn't add our timestamp");
    write!(
        vfile,
        "pub const SEMVER: &'static str = \"{}\";\n",
        semver
            .strip_suffix("\r\n")
            .or(semver.strip_suffix("\n"))
            .unwrap_or(&semver)
    )
    .expect("couldn't add our semver");
}

fn print_header<U: Write>(out: &mut U) {
    let s = r####"// Versioning information is kept in a separate file, attached to a small, well-known server in the Xous System
// This is a trade-off between rebuild times and flexibility.
// This was autogenerated by xtask/src/main.rs:print_header(). Do not edit manually.

pub(crate) fn get_version() -> crate::api::VersionString {
    let mut v = crate::api::VersionString {
        version: xous_ipc::String::new()
    };
    v.version.append(SEMVER).ok();
    #[cfg(not(feature="no-timestamp"))]
    v.version.append("\n").ok();
    #[cfg(not(feature="no-timestamp"))]
    v.version.append(TIMESTAMP).ok();
    v
}
"####;
    out.write_all(s.as_bytes())
        .expect("couldn't write our version template header");
}

////////////////////////// App manifest infrastructure
use serde::{Deserialize, Serialize};
use std::string::String;
#[derive(Deserialize, Serialize, Debug)]
struct AppManifest {
    context_name: String,
    menu_name: HashMap<String, HashMap<String, String>>,
}
#[derive(Deserialize, Serialize, Debug)]
struct Locales {
    locales: HashMap<String, HashMap<String, String>>,
}

fn generate_app_menus(apps: &Vec<String>) {
    let file = File::open("apps/manifest.json").expect("Failed to open the manifest file");
    let mut reader = std::io::BufReader::new(file);
    let mut content = String::new();
    reader
        .read_to_string(&mut content)
        .expect("Failed to read the file");
    let manifest: HashMap<String, AppManifest> =
        serde_json::from_str(&content).expect("Cannot parse manifest file");

    // localization file
    // inject all the localization strings into the i18n file, which in theory reduces the churn on other crates that depend
    // on the global i18n file between build variants
    let mut l = BTreeMap::<String, BTreeMap<String, String>>::new();
    for (_app, manifest) in manifest.iter() {
        for (name, translations) in &manifest.menu_name {
            let mut map = BTreeMap::<String, String>::new();
            for (language, phrase) in translations {
                map.insert(language.to_string(), phrase.to_string());
            }
            l.insert(name.to_string(), map);
        }
    }
    // output a JSON localizations file, if things have changed
    let new_i18n = serde_json::to_string(&l).unwrap();
    overwrite_if_changed(&new_i18n, "apps/i18n.json");

    // output the Rust manifests - tailored just for the apps requested
    let mut working_set = BTreeMap::<String, &AppManifest>::new();
    // derive a working_set that is just the apps we requested
    for app in apps {
        if let Some(manifest) = manifest.get(app) {
            working_set.insert(app.to_string(), &manifest);
        }
    }

    // construct the gam_tokens
    let mut gam_tokens = String::new();
    writeln!(
        gam_tokens,
        "// This file is auto-generated by xtask/main.rs generate_app_menus()"
    )
    .unwrap();
    for (app_name, manifest) in working_set.iter() {
        writeln!(
            gam_tokens,
            "pub const APP_NAME_{}: &'static str = \"{}\";",
            app_name.to_uppercase(),
            manifest.context_name,
        )
        .unwrap();
    }
    writeln!(
        gam_tokens,
        "\npub const EXPECTED_APP_CONTEXTS: &[&'static str] = &["
    )
    .unwrap();
    for (app_name, _manifest) in working_set.iter() {
        writeln!(gam_tokens, "    APP_NAME_{},", app_name.to_uppercase(),).unwrap();
    }
    writeln!(gam_tokens, "];").unwrap();
    overwrite_if_changed(&gam_tokens, "services/gam/src/apps.rs");

    // construct the app menu
    let mut menu = String::new();
    writeln!(
        menu,
        "// This file is auto-generated by xtask/main.rs generate_app_menus()"
    )
    .unwrap();
    if apps.len() == 0 {
        writeln!(menu, "// NO APPS SELECTED: suppressing warning messages!").unwrap();
        writeln!(menu, "#![allow(dead_code)]").unwrap();
        writeln!(menu, "#![allow(unused_imports)]").unwrap();
        writeln!(menu, "#![allow(unused_variables)]").unwrap();
    }
    writeln!(menu, r####"use crate::StatusOpcode;
use gam::{{MenuItem, MenuPayload}};
use locales::t;
use num_traits::*;
use std::{{error::Error, fmt}};

#[derive(Debug)]
pub enum AppDispatchError {{
    IndexNotFound(usize),
}}

impl Error for AppDispatchError {{}}

impl fmt::Display for AppDispatchError {{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {{
        match self {{
            AppDispatchError::IndexNotFound(app_index) => write!(f, "Index {{}} not found", app_index),
        }}
    }}
}}

pub(crate) fn app_dispatch(gam: &gam::Gam, token: [u32; 4], index: usize) -> Result<(), AppDispatchError> {{
    match index {{"####).unwrap();
    for (index, (app_name, _manifest)) in working_set.iter().enumerate() {
        writeln!(
            menu,
            "        {} => {{
            gam.switch_to_app(gam::APP_NAME_{}, token).expect(\"couldn't raise app\");
            Ok(())
        }},",
            index,
            app_name.to_uppercase()
        )
        .unwrap();
    }
    writeln!(
        menu,
        r####"        _ => Err(AppDispatchError::IndexNotFound(index)),
    }}
}}

pub(crate) fn app_index_to_name(index: usize) -> Result<&'static str, AppDispatchError> {{
    match index {{"####
    )
    .unwrap();
    for (index, (_, _manifest)) in working_set.iter().enumerate() {
        for name in _manifest.menu_name.keys() {
            writeln!(
                menu,
                "        {} => Ok(t!(\"{}\", xous::LANG)),",
                index, name,
            )
            .unwrap();
        }
    }
    writeln!(
        menu,
        r####"        _ => Err(AppDispatchError::IndexNotFound(index)),
    }}
}}

pub(crate) fn app_menu_items(menu_items: &mut Vec::<MenuItem>, status_conn: u32) {{
"####
    )
    .unwrap();
    for (index, (_app_name, manifest)) in working_set.iter().enumerate() {
        writeln!(menu, "    menu_items.push(MenuItem {{",).unwrap();
        assert!(
            manifest.menu_name.len() == 1,
            "Improper menu name record entry"
        );
        for name in manifest.menu_name.keys() {
            writeln!(
                menu,
                "        name: xous_ipc::String::from_str(t!(\"{}\", xous::LANG)),",
                name
            )
            .unwrap();
        }
        writeln!(menu, "        action_conn: Some(status_conn),",).unwrap();
        writeln!(
            menu,
            "        action_opcode: StatusOpcode::SwitchToApp.to_u32().unwrap(),",
        )
        .unwrap();
        writeln!(
            menu,
            "        action_payload: MenuPayload::Scalar([{}, 0, 0, 0]),",
            index
        )
        .unwrap();
        writeln!(menu, "        close_on_select: true,",).unwrap();
        writeln!(menu, "    }});\n",).unwrap();
    }
    writeln!(menu, "}}").unwrap();
    overwrite_if_changed(&menu, "services/status/src/app_autogen.rs");
}

fn overwrite_if_changed(new_string: &String, old_file: &str) {
    let original = match OpenOptions::new().read(true).open(old_file) {
        Ok(mut ref_file) => {
            let mut buf = String::new();
            ref_file
                .read_to_string(&mut buf)
                .expect("UTF-8 error in previous localization file");
            buf
        }
        _ => String::new(),
    };
    if &original != new_string {
        let mut new_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(old_file)
            .expect("Can't open our gam manifest for writing");
        write!(new_file, "{}", new_string).unwrap()
    }
}
