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
    let task = env::args().nth(1);
    match task.as_deref() {
        Some("renode-image") => renode_image(false)?,
        Some("renode-image-debug") => renode_image(true)?,
        Some("run") => run(false)?,
        Some("hw-image") => build_hw_image(false, env::args().nth(2))?,
        Some("debug") => run(true)?,
        _ => print_help(),
    }
    Ok(())
}

fn print_help() {
    eprintln!(
        "Tasks:
renode-image            builds a test image for renode
renode-image-debug      builds a test image for renode in debug mode
hw-image [soc.svd]      builds an image for real hardware
run                     runs a release build using a hosted environment
debug                   runs a debug build using a hosted environment
"
    )
}

fn build_hw_image(debug: bool, svd: Option<String>) -> Result<(), DynError> {
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

    let kernel = build_kernel(debug)?;
    let mut init = vec![];
    for pkg in &[
        "shell",
        "graphics-server",
        "ticktimer-server",
        "log-server",
        "com",
    ] {
        // "xous-names"
        init.push(build(pkg, debug, Some(TARGET), None)?);
    }
    let loader = build("loader", debug, Some(TARGET), Some("loader".into()))?;

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
    let mut loader_bin_file = std::fs::File::open(loader_bin).expect("couldn't open loader.bin");
    let mut buf = vec![];
    loader_bin_file
        .read_to_end(&mut buf)
        .expect("couldn't read loader.bin");
    xous_img
        .write_all(&buf)
        .expect("couldn't write loader.bin to xous.img");
    let leftover_bytes = 65536 - buf.len();
    let mut buf = vec![];
    buf.resize_with(leftover_bytes, Default::default);
    xous_img
        .write_all(&buf)
        .expect("couldn't pad xous.img with zeroes");

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

fn renode_image(debug: bool) -> Result<(), DynError> {
    let path = std::path::Path::new("emulation/renode.svd");
    std::env::set_var("XOUS_SVD_FILE", path.canonicalize().unwrap());
    let kernel = build_kernel(debug)?;
    let mut init = vec![];
    for pkg in &[
        "shell",
        "log-server",
        "graphics-server",
        "ticktimer-server",
        "com",
        "xous-names",
    ] {
        init.push(build(pkg, debug, Some(TARGET), None)?);
    }
    build("loader", debug, Some(TARGET), Some("loader".into()))?;

    create_image(
        &kernel,
        &init,
        debug,
        MemorySpec::SvdFile("emulation/renode.svd".into()),
    )?;

    Ok(())
}

fn run(debug: bool) -> Result<(), DynError> {
    let stream = if debug { "debug" } else { "release" };
    let init = [
        "shell",
        "log-server",
        "graphics-server",
        "ticktimer-server",
        "com",
    ]; // , "xous-names"

    // let mut init_paths = vec![];
    for pkg in &init {
        build(pkg, debug, None, None)?;
    }
    // println!("Built packages: {:?}", init_paths);

    // Build and run the kernel
    let mut args = vec!["run"];
    if !debug {
        args.push("--release");
    }

    args.push("--");

    let mut paths = vec![];
    for i in &init {
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
    build("kernel", debug, Some(TARGET), Some("kernel".into()))
}

fn build(
    project: &str,
    debug: bool,
    target: Option<&str>,
    directory: Option<PathBuf>,
) -> Result<PathBuf, DynError> {
    println!("Building {}...", project);
    let stream = if debug { "debug" } else { "release" };
    let mut args = vec!["build", "--package", project];
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
            "{}/target/{}{}/{}",
            base_dir.to_str().ok_or(BuildError::PathConversionError)?,
            target_path,
            stream,
            project
        )))
    } else {
        Ok(project_root().join(&format!("target/{}{}/{}", target_path, stream, project)))
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
