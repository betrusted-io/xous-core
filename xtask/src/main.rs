use std::{
    env,
    io::{Read, Write},
    path::{Path, PathBuf, MAIN_SEPARATOR},
    process::Command,
};

mod generate_locales;
use generate_locales::*;

type DynError = Box<dyn std::error::Error>;

const TARGET: &str = "riscv32imac-unknown-none-elf";

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

fn main() {
    if let Err(e) = try_main() {
        eprintln!("{}", e);
        std::process::exit(-1);
    }
}

fn try_main() -> Result<(), DynError> {
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
        "engine-sha512",
        "engine-25519",
        "spinor",
        "root-keys",
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
    let base_pkgs = ["ticktimer-server", "log-server"];
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
        Some("renode-image") => renode_image(false, &hw_pkgs)?,
        Some("renode-test") => renode_image(false, &cbtest_pkgs)?,
        Some("libstd-test") => {
            let mut pkgs = base_pkgs.to_vec();
            let args: Vec<String> = args.collect();
            for program in &args {
                pkgs.push(&program);
            }
            renode_image(false, &pkgs)?;
        }
        Some("renode-aes-test") => renode_image(false, &aestest_pkgs)?,
        Some("renode-image-debug") => renode_image(true, &hw_pkgs)?,
        Some("run") => run(false, &hw_pkgs)?,
        Some("hw-image") => build_hw_image(false, env::args().nth(2), &hw_pkgs, lkey, kkey, None)?,
        Some("benchmark") => {
            build_hw_image(false, env::args().nth(2), &benchmark_pkgs, lkey, kkey, None)?
        }
        Some("minimal") => {
            build_hw_image(false, env::args().nth(2), &minimal_pkgs, lkey, kkey, None)?
        }
        Some("cbtest") => {
            build_hw_image(false, env::args().nth(2), &cbtest_pkgs, lkey, kkey, None)?
        }
        Some("trng-test") => build_hw_image(
            false,
            env::args().nth(2),
            &hw_pkgs,
            lkey,
            kkey,
            Some(&["--features", "urandomtest"]),
        )?,
        Some("ro-test") => build_hw_image(
            false,
            env::args().nth(2),
            &hw_pkgs,
            lkey,
            kkey,
            Some(&["--features", "ringosctest"]),
        )?,
        Some("av-test") => build_hw_image(
            false,
            env::args().nth(2),
            &hw_pkgs,
            lkey,
            kkey,
            Some(&["--features", "avalanchetest"]),
        )?,
        Some("sr-test") => build_hw_image(false, env::args().nth(2), &sr_pkgs, lkey, kkey, None)?,
        Some("debug") => run(true, &hw_pkgs)?,
        Some("burn-kernel") => update_usb(true, false, false)?,
        Some("burn-loader") => update_usb(false, true, false)?,
        Some("burn-soc") => update_usb(false, false, true)?,
        Some("generate-locales") => generate_locales(),
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
burn-soc                invoke the `usb_update.py` utility to burn the SoC gateware
generate-locales        only generate the locales include for the language selected in xous-rs/src/locale.rs

Please refer to tools/README_UPDATE.md for instructions on how to set up `usb_update.py`
"
    )
}

fn update_usb(do_kernel: bool, do_loader: bool, do_soc: bool) -> Result<(), DynError> {
    use std::io::{BufRead, BufReader, Error, ErrorKind};
    use std::process::Stdio;

    if do_kernel {
        println!("Burning kernel");
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
        println!("Burning loader");
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
    if do_soc {
        println!("Burning SoC gateware");
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

    Ok(())
}

fn build_hw_image(
    debug: bool,
    svd: Option<String>,
    packages: &[&str],
    lkey: Option<String>,
    kkey: Option<String>,
    extra_args: Option<&[&str]>,
) -> Result<(), DynError> {
    generate_locales();

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

    // extract key file names; replace with defaults if not specified
    let loaderkey_file = lkey.unwrap_or("devkey/dev.key".into());
    let kernelkey_file = kkey.unwrap_or("devkey/dev.key".into());

    let kernel = build_kernel(debug)?;
    let mut init = vec![];
    let base_path = build(packages, debug, Some(TARGET), None, extra_args)?;
    for pkg in packages {
        let mut pkg_path = base_path.clone();
        pkg_path.push(pkg);
        init.push(pkg_path);
    }
    let mut loader = build(
        &["loader"],
        debug,
        Some(TARGET),
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
    println!("Signed loader at {}", loader_bin.to_str().unwrap());
    println!("Signed kernel at {}", xous_img_path.to_str().unwrap());

    Ok(())
}

/*
fn sign_loader(in_path: Pathbuf, out_path: Pathbuf) -> Result<(), DynError> {
    let mut in_file = File::open(in_path)?;
    let mut out_file = File::open(out_path)?;

    let mut loader = Vec::<u8>::new();
    in_file.read_to_end(&mut loader);


}*/

fn renode_image(debug: bool, packages: &[&str]) -> Result<(), DynError> {
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
        Err("Unable to regenerate Renode platform file")?;
    }
    build_hw_image(
        debug,
        Some("emulation/soc/renode.svd".to_owned()),
        packages,
        None,
        None,
        None,
    )
}

fn run(debug: bool, init: &[&str]) -> Result<(), DynError> {
    generate_locales();

    let stream = if debug { "debug" } else { "release" };

    build(&init, debug, None, None, None)?;

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
        Some(TARGET),
        Some("kernel".into()),
        None,
    )?;
    path.push("kernel");
    Ok(path)
}

fn build(
    packages: &[&str],
    debug: bool,
    target: Option<&str>,
    directory: Option<PathBuf>,
    extra_args: Option<&[&str]>,
) -> Result<PathBuf, DynError> {
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

    let output_file = format!("target/{}/{}/args.bin", TARGET, stream);
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
    Ok(project_root().join(&format!("target/{}/{}/args.bin", TARGET, stream)))
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

// fn dist_dir() -> PathBuf {
//     project_root().join("target/dist")
// }
