use std::{
    env,
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
    let hw_pkgs = [
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
        "rtc",
        "susres",
        "codec",
        "sha2",
        "engine-25519",
        "spinor",
        "root-keys",
        "jtag",
        "oqc-test",
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
        //        "rkyv-test-client",
        //        "rkyv-test-server",
        "test-stub",
        "susres",
        "com",
    ];
    // A base set of packages. This is all you need for a normal
    // operating system that can run libstd
    let base_pkgs = ["ticktimer-server", "log-server", "susres", "xous-names", "trng"];
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
    let aestest_pkgs = ["ticktimer-server", "log-server", "aes-test"];
    let mut args = env::args();
    let task = args.nth(1);
    let lkey = args.nth(3);
    let kkey = args.nth(4);
    match task.as_deref() {
        Some("renode-image") => renode_image(false, &hw_pkgs, &[])?,
        Some("renode-test") => renode_image(false, &cbtest_pkgs, &[])?,
        Some("libstd-test") => {
            let mut args = env::args();
            args.nth(1);
            let pkgs = base_pkgs.to_vec();
            let args: Vec<String> = args.collect();
            let mut extra_packages = vec![];
            for program in &args {
                extra_packages.push(program.as_str());
            }
            renode_image(false, &pkgs, extra_packages.as_slice())?;
        }
        Some("renode-aes-test") => renode_image(false, &aestest_pkgs, &[])?,
        Some("renode-image-debug") => renode_image(true, &hw_pkgs, &[])?,
        Some("run") => run(false, &hw_pkgs)?,
        Some("hw-image") => {
            build_hw_image(false, env::args().nth(2), &hw_pkgs, lkey, kkey, None, &[])?
        }
        Some("benchmark") => build_hw_image(
            false,
            env::args().nth(2),
            &benchmark_pkgs,
            lkey,
            kkey,
            None,
            &[],
        )?,
        Some("minimal") => build_hw_image(
            false,
            env::args().nth(2),
            &minimal_pkgs,
            lkey,
            kkey,
            None,
            &[],
        )?,
        Some("cbtest") => build_hw_image(
            false,
            env::args().nth(2),
            &cbtest_pkgs,
            lkey,
            kkey,
            None,
            &[],
        )?,
        Some("trng-test") => build_hw_image(
            false,
            env::args().nth(2),
            &hw_pkgs,
            lkey,
            kkey,
            Some(&["--features", "urandomtest"]),
            &[],
        )?,
        Some("ro-test") => build_hw_image(
            false,
            env::args().nth(2),
            &hw_pkgs,
            lkey,
            kkey,
            Some(&["--features", "ringosctest"]),
            &[],
        )?,
        Some("av-test") => build_hw_image(
            false,
            env::args().nth(2),
            &hw_pkgs,
            lkey,
            kkey,
            Some(&["--features", "avalanchetest"]),
            &[],
        )?,
        Some("sr-test") => {
            build_hw_image(false, env::args().nth(2), &sr_pkgs, lkey, kkey, None, &[])?
        }
        Some("debug") => run(true, &hw_pkgs)?,
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
renode-image            builds a functional image for renode
renode-test             builds a test image for renode
renode-image-debug      builds a test image for renode in debug mode
libstd-test [pkg1] [..] builds a test image that includes the minimum packages, plus those
                        specified on the command line (e.g. built externally)
hw-image [soc.svd] [loader.key] [kernel.key]   builds an image for real hardware
run                     runs a release build using a hosted environment
debug                   runs a debug build using a hosted environment
benchmark [soc.svd]     builds a benchmarking image for real hardware
minimal [soc.svd]       builds a minimal image for API testing
cbtest                  builds an image for callback testing
trng-test [soc.svd]     builds an image for TRNG testing - urandom source seeded by TRNG+AV
ro-test [soc.svd]       builds an image for ring oscillator only TRNG testing
av-test [soc.svd]       builds an image for avalanche generater only TRNG testing
sr-test [soc.svd]       builds the suspend/resume testing image
burn-kernel             invoke the `usb_update.py` utility to burn the kernel
burn-loader             invoke the `usb_update.py` utility to burn the loader
burn-soc                invoke the `usb_update.py` utility to stage the SoC gateware, which must then be provisioned with secret material using the Precursor device.
nuke-soc                'Factory reset' - invoke the `usb_update.py` utility to burn the SoC gateware, erasing most secrets. For developers.
generate-locales        only generate the locales include for the language selected in xous-rs/src/locale.rs
wycheproof-import       generate binary test vectors for engine-25519 from whycheproof-import/x25519.json

Please refer to tools/README_UPDATE.md for instructions on how to set up `usb_update.py`
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
        let stdout = Command::new("python3")
            .arg("tools/usb_update.py")
            .arg("-k")
            .arg("target/riscv32imac-unknown-none-elf/release/xous.img")
            .stdout(Stdio::piped())
            .spawn()?
            .stdout
            .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture output"))?;

        let reader = BufReader::new(stdout);
        reader
            .lines()
            .for_each(|line| println!("{}", line.unwrap()));
    }
    if do_loader {
        println!("Burning loader. After this is done, you must select 'Sign xous update' to self-sign the image.");
        let stdout = Command::new("python3")
            .arg("tools/usb_update.py")
            .arg("-l")
            .arg("target/riscv32imac-unknown-none-elf/release/loader.bin")
            .stdout(Stdio::piped())
            .spawn()?
            .stdout
            .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture output"))?;

        let reader = BufReader::new(stdout);
        reader
            .lines()
            .for_each(|line| println!("{}", line.unwrap()));
    }
    if stage_soc {
        println!("Staging SoC gateware. After this is done, you must select 'Install Gateware Update' from the root menu of your Precursor device.");
        let stdout = Command::new("python3")
            .arg("tools/usb_update.py")
            .arg("-s")
            .arg("precursors/soc_csr.bin")
            .stdout(Stdio::piped())
            .spawn()?
            .stdout
            .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture output"))?;

        let reader = BufReader::new(stdout);
        reader
            .lines()
            .for_each(|line| println!("{}", line.unwrap()));
    }
    if nuke_soc {
        println!("Installing factory-reset SoC gateware (secrets will be lost)!");
        let stdout = Command::new("python3")
            .arg("tools/usb_update.py")
            .arg("--soc")
            .arg("precursors/soc_csr.bin")
            .stdout(Stdio::piped())
            .spawn()?
            .stdout
            .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture output"))?;

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

    // extract key file names; replace with defaults if not specified
    let loaderkey_file = lkey.unwrap_or_else(|| "devkey/dev.key".into());
    let kernelkey_file = kkey.unwrap_or_else(|| "devkey/dev.key".into());

    let kernel = build_kernel(debug)?;
    let mut init = vec![];
    let base_path = build(packages, debug, Some(PROGRAM_TARGET), None, extra_args)?;
    for pkg in packages {
        let mut pkg_path = base_path.clone();
        pkg_path.push(pkg);
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

fn renode_image(debug: bool, packages: &[&str], extra_packages: &[&str]) -> Result<(), DynError> {
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
        None,
        extra_packages,
    )
}

fn run(debug: bool, init: &[&str]) -> Result<(), DynError> {
    let stream = if debug { "debug" } else { "release" };

    build(init, debug, None, None, None)?;

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

    println!("Building and running kernel...");
    let status = Command::new(cargo())
        .current_dir(dir)
        .args(&args)
        .status()?;
    if !status.success() {
        return Err("cargo build failed".into());
    }

    Ok(())
}

fn build_kernel(debug: bool) -> Result<PathBuf, DynError> {
    let mut path = build(
        &["kernel"],
        debug,
        Some(KERNEL_TARGET),
        Some("kernel".into()),
        None,
    )?;
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
fn ensure_compiler(target: &Option<&str>) -> Result<(), String> {
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

        if have_toolchain {
            Ok(Some(toolchain_path))
        } else {
            Ok(None)
        }
    }

    // If the sysroot exists, then we're good.
    let target = target.unwrap_or(PROGRAM_TARGET);
    if get_sysroot(Some(target))?.is_some() {
        DONE_COMPILER_CHECK.store(true, std::sync::atomic::Ordering::SeqCst);
        return Ok(());
    }

    // Since no sysroot exists, we must download a new one.
    let toolchain_path =
        get_sysroot(None)?.ok_or_else(|| "default toolchain not installed".to_owned())?;
    // If the terminal is a tty, offer to download the latest toolchain.
    if !atty::is(atty::Stream::Stdin) {
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
) -> Result<PathBuf, DynError> {
    ensure_compiler(&target)?;
    let stream = if debug { "debug" } else { "release" };
    let mut args = vec!["build"];
    print!("Building");
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

/// Force the locales to be regenerated. This simply `touches`
/// the `build.rs` for locales, causing a rebuild next time.
fn generate_locales() -> Result<(), std::io::Error> {
    let ts = filetime::FileTime::from_system_time(std::time::SystemTime::now());
    filetime::set_file_mtime("locales/src/lib.rs", ts)
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
            output_file
        ])
        .status()?;
    if !status.success() {
        return Err("wycheproof-import failed. If any, the output will not be usable.".into());
    }

    println!();
    println!("Wrote wycheproof x25519 testvectors to '{}'.", output_file);

    return Ok(())
}

// fn dist_dir() -> PathBuf {
//     project_root().join("target/dist")
// }
