use std::{
    env,
    io::{Read, Write},
    path::{Path, PathBuf, MAIN_SEPARATOR},
    process::Command,
};

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
    ];
    let fcc_pkgs = [
        "fcc-agent",
        "graphics-server",
        "ticktimer-server",
        "log-server",
        "com",
        "xous-names",
        "keyboard",
        "trng",
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
    ];
    let minimal_pkgs = [
        "ticktimer-server",
        "log-server",
        "xous-names",
        "trng",
        "llio",
        "rkyv-test-client",
        "rkyv-test-server",
    ];
    let task = env::args().nth(1);
    match task.as_deref() {
        Some("renode-image") => renode_image(false, &minimal_pkgs)?,
        Some("renode-test") => renode_image(
            false,
            &[
                "ticktimer-server",
                "log-server",
                "xous-names",
                "rkyv-test-client",
                "rkyv-test-server",
            ],
        )?,
        Some("renode-image-debug") => renode_image(true, &hw_pkgs)?,
        Some("run") => run(false, &hw_pkgs)?,
        Some("hw-image") => build_hw_image(false, env::args().nth(2), &hw_pkgs)?,
        Some("benchmark") => build_hw_image(false, env::args().nth(2), &benchmark_pkgs)?,
        Some("fcc-agent") => build_hw_image(false, env::args().nth(2), &fcc_pkgs)?,
        Some("minimal") => build_hw_image(false, env::args().nth(2), &minimal_pkgs)?,
        Some("debug") => run(true, &hw_pkgs)?,
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
hw-image [soc.svd]      builds an image for real hardware
run                     runs a release build using a hosted environment
debug                   runs a debug build using a hosted environment
benchmark [soc.svd]     builds a benchmarking image for real hardware
fcc-agent [soc.svd]     builds a version suitable for FCC testing
minimal [soc.svd]       builds a minimal image for API testing
"
    )
}

fn build_hw_image(debug: bool, svd: Option<String>, packages: &[&str]) -> Result<(), DynError> {
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

    // std::fs::copy(path, std::path::Path::new("emulation/renode.svd"))?;

    let kernel = build_kernel(debug)?;
    let mut init = vec![];
    let base_path = build(packages, debug, Some(TARGET), None)?;
    for pkg in packages {
        let mut pkg_path = base_path.clone();
        pkg_path.push(pkg);
        init.push(pkg_path);
    }
    let mut loader = build(&["loader"], debug, Some(TARGET), Some("loader".into()))?;
    loader.push(PathBuf::from("loader"));

    let output_bundle = create_image(&kernel, &init, debug, MemorySpec::SvdFile(svd_file))?;
    println!();
    println!(
        "Kernel+Init bundle is available at {}",
        output_bundle.display()
    );

    let mut loader_bin = output_bundle.parent().unwrap().to_owned();
    loader_bin.push("loader.bin");
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
            loader_bin.as_os_str().to_str().unwrap(),
        ])
        .status()?;
    if !status.success() {
        return Err("cargo build failed".into());
    }

    let mut xous_img_path = output_bundle.parent().unwrap().to_owned();
    xous_img_path.push("xous.img");
    let mut xous_img = std::fs::File::create(&xous_img_path).expect("couldn't create xous.img");
    let mut bundle_file = std::fs::File::open(output_bundle).expect("couldn't open output bundle");
    let mut buf = vec![];
    bundle_file
        .read_to_end(&mut buf)
        .expect("couldn't read output bundle file");
    xous_img
        .write_all(&buf)
        .expect("couldn't write bundle file to xous.img");

    println!();
    println!("Bundled image file created at {}", xous_img_path.display());

    Ok(())
}

fn renode_image(debug: bool, packages: &[&str]) -> Result<(), DynError> {
    let path = std::path::Path::new("emulation/renode.svd");
    std::env::set_var("XOUS_SVD_FILE", path.canonicalize().unwrap());
    let kernel = build_kernel(debug)?;
    let mut init = vec![];
    let base_path = build(packages, debug, Some(TARGET), None)?;
    for pkg in packages {
        let mut pkg_path = base_path.clone();
        pkg_path.push(pkg);
        init.push(pkg_path);
    }
    build(&["loader"], debug, Some(TARGET), Some("loader".into()))?;

    create_image(
        &kernel,
        &init,
        debug,
        MemorySpec::SvdFile("emulation/renode.svd".into()),
    )?;

    Ok(())
}

fn run(debug: bool, init: &[&str]) -> Result<(), DynError> {
    let stream = if debug { "debug" } else { "release" };

    build(&init, debug, None, None)?;

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
    let mut path = build(&["kernel"], debug, Some(TARGET), Some("kernel".into()))?;
    path.push("kernel");
    Ok(path)
}

fn build(
    packages: &[&str],
    debug: bool,
    target: Option<&str>,
    directory: Option<PathBuf>,
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
