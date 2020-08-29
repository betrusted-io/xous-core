use std::{
    env,
    path::{Path, PathBuf, MAIN_SEPARATOR},
    process::Command,
};

type DynError = Box<dyn std::error::Error>;

const TARGET: &str = "riscv32imac-unknown-none-elf";

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
        Some("renode-image") => image(false)?,
        Some("renode-image-debug") => image(true)?,
        Some("run") => run(false)?,
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
run                     runs a release build using a hosted environment
debug                   runs a debug build using a hosted environment
"
    )
}

fn image(debug: bool) -> Result<(), DynError> {
    let kernel = build_kernel(debug)?;
    let mut init = vec![];
    for pkg in &["shell", "log-server", "graphics-server"] {
        init.push(build(pkg, debug, Some(TARGET), None)?);
    }
    build("loader", debug, Some(TARGET), Some("loader".into()))?;

    create_image(&kernel, &init, debug)?;

    Ok(())
}

fn run(debug: bool) -> Result<(), DynError> {
    let stream = if debug { "debug" } else { "release" };
    let init = ["shell", "log-server", "graphics-server"];

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

fn create_image(kernel: &Path, init: &[PathBuf], debug: bool) -> Result<PathBuf, DynError> {
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

    args.push("--csv");
    args.push("emulation/csr.csv");

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
