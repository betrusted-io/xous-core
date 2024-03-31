use std::collections::HashMap;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use lazy_static::lazy_static;

use crate::{cargo, project_root};
use crate::{TARGET_TRIPLE_ARM, TARGET_TRIPLE_RISCV32};

const TOOLCHAIN_RELEASE_URL_RISCV32: &str = "https://api.github.com/repos/betrusted-io/rust/releases";
const TOOLCHAIN_RELEASE_URL_ARM: &str = "https://api.github.com/repos/Foundation-Devices/rust/releases";

lazy_static! {
    static ref TOOLCHAIN_RELEASE_URLS: HashMap<String, String> = HashMap::from([
        (TARGET_TRIPLE_RISCV32.to_owned(), TOOLCHAIN_RELEASE_URL_RISCV32.to_owned()),
        (TARGET_TRIPLE_ARM.to_owned(), TOOLCHAIN_RELEASE_URL_ARM.to_owned()),
    ]);
}

/// Since we use the same TARGET for all calls to `build()`,
/// cache it inside an atomic boolean. If this is `true` then
/// it means we can assume the check passed already.
static DONE_COMPILER_CHECK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static DONE_KERNEL_COMPILER_CHECK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn ask_to_install() -> bool {
    let mut buffer = String::new();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    loop {
        stdout.flush().unwrap();
        buffer.clear();
        stdin.read_line(&mut buffer).unwrap();

        let trimmed = buffer.trim();

        if trimmed == "n" || trimmed == "N" {
            return false;
        }

        if trimmed == "y" || trimmed == "Y" || trimmed.is_empty() {
            return true;
        }
        println!();
        println!("Please answer 'y' or 'n'");
    }
}
/// Return the sysroot for the given target. If the target does not exist,
/// return None.
fn get_sysroot(target: Option<&str>, check_version: bool) -> Result<Option<String>, String> {
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
        if check_version {
            version_path.push("RUST_VERSION");
            if let Ok(mut vp) = File::open(&version_path) {
                let mut version_str = String::new();
                if vp.read_to_string(&mut version_str).is_err() {
                    return Err("Unable to get version string".to_owned());
                }

                let rustc_version_str = format!("{}", rustc_version::version().unwrap());
                if version_str.trim() != rustc_version_str.trim() {
                    println!(
                        "Version upgrade. Compiler is version {}, the installed toolchain is for {}",
                        rustc_version_str.trim(),
                        version_str.trim(),
                    );
                    // return Err(format!("Version upgrade. Compiler is version {}, the installed toolchain is
                    // for {}", version_str, rustc_version_str));
                    return Ok(None);
                }
            } else {
                println!("Outdated toolchain installed.");
                // return Err("Outdated toolchain installed".to_owned());
                return Ok(None);
            }
        } else if !Path::new(&version_path).exists() {
            return Ok(None);
        }
    }

    if have_toolchain { Ok(Some(toolchain_path)) } else { Ok(None) }
}

pub(crate) fn ensure_kernel_compiler(target: &Option<&str>, force_install: bool) -> Result<(), String> {
    if DONE_KERNEL_COMPILER_CHECK.load(std::sync::atomic::Ordering::SeqCst) {
        return Ok(());
    }

    // If the sysroot exists, then we're good.
    let target = target.unwrap_or(TARGET_TRIPLE_RISCV32);
    if get_sysroot(Some(target), false)?.is_some() {
        DONE_COMPILER_CHECK.store(true, std::sync::atomic::Ordering::SeqCst);
        return Ok(());
    }

    // If the terminal is a tty, or if toolchain installation is forced,
    // download the latest toolchain.
    if !atty::is(atty::Stream::Stdin) && !force_install {
        return Err(format!("Toolchain for {} not found", target));
    }

    if force_install {
        println!("Downloading toolchain");
    } else {
        println!("Error: {} target was not found on this system!", target);
        print!("Would you like this program to attempt to download and install it using rustup?   [Y/n] ");
        if ask_to_install() {
            println!("Installing toolchain for {} via rustup", target);
        } else {
            return Err(format!("Please install the {} toolchain", target));
        }
    }

    let rustup_command = Command::new("rustup")
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .args(["target", "add", target])
        .spawn()
        .map_err(|e| format!("could not run rustup: {}", e))?;

    let rustup_output = rustup_command.wait_with_output().unwrap();
    if !rustup_output.status.success() {
        return Err(format!(
            "could not add target {}: {}",
            target,
            String::from_utf8_lossy(&rustup_output.stdout)
        ));
    }

    DONE_COMPILER_CHECK.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// Ensure we have a compatible compiler toolchain. We use a new Target,
/// and we want to give the user a friendly way of installing the latest
/// Rust toolchain.
pub(crate) fn ensure_compiler(
    target: &Option<&str>,
    force_install: bool,
    remove_existing: bool,
) -> Result<(), String> {
    if DONE_COMPILER_CHECK.load(std::sync::atomic::Ordering::SeqCst) {
        return Ok(());
    }

    // If the sysroot exists, then we're good.
    let target = target.unwrap_or(TARGET_TRIPLE_RISCV32);
    if let Some(path) = get_sysroot(Some(target), true)? {
        let mut version_path = PathBuf::from(&path);
        version_path.push("lib");
        version_path.push("rustlib");
        version_path.push(target);
        if remove_existing {
            println!("Target path exists, removing it");
            std::fs::remove_dir_all(version_path).map_err(|e| format!("unable to remove existing toolchain: {}", e))?;
            println!("Also removing target directories for existing toolchain");
            let mut target_main = project_root();
            target_main.push("target");
            target_main.push(target);
            std::fs::remove_dir_all(target_main).ok();

            let mut target_loader = project_root();
            target_loader.push("loader");
            target_loader.push("target");
            target_loader.push(target);
            std::fs::remove_dir_all(target_loader).ok();
        } else {
            DONE_COMPILER_CHECK.store(true, std::sync::atomic::Ordering::SeqCst);
            return Ok(());
        }
    }

    // Since no sysroot exists, we must download a new one.
    let toolchain_path =
        get_sysroot(None, true)?.ok_or_else(|| "default toolchain not installed".to_owned())?;
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
    if force_install {
        println!("Downloading toolchain");
    } else {
        println!("Error: Toolchain for {} was not found on this system!", target);
        print!("Would you like this program to attempt to download and install it?   [Y/n] ");
        if ask_to_install() {
            println!("Installing toolchain for {} into {}", target, toolchain_path);
        } else {
            return Err(format!("Please install the {} toolchain", target));
        }
    }

    fn get_toolchain_url(target: &str, major: u64, minor: u64, patch: u64) -> Result<String, String> {
        let url = TOOLCHAIN_RELEASE_URLS
            .get(target)
            .ok_or_else(|| format!("Can't find toolchain URL for target {}", target))?;
        let j: serde_json::Value = ureq::get(url)
            .set("Accept", "application/vnd.github.v3+json")
            .call()
            .map_err(|e| format!("{}", e))?
            .into_json()
            .map_err(|e| format!("{}", e))?;
        // let j: serde_json::Value = serde_json::from_str(CONTENT).expect("Cannot parse manifest file");

        let releases = j.as_array().unwrap();
        let mut tag_urls = std::collections::BTreeMap::new();

        let target_prefix = format!("{}.{}.{}", major, minor, patch);
        for r in releases {
            // println!(">>> Value: {}", r);

            let keys = match r.as_object() {
                None => continue,
                Some(r) => r,
            };
            let release = match keys.get("tag_name") {
                None => continue,
                Some(s) => match s.as_str() {
                    None => continue,
                    Some(s) => s,
                },
            };
            if !release.starts_with(&target_prefix) {
                continue;
            }

            let assets = match keys.get("assets") {
                None => continue,
                Some(s) => match s.as_array() {
                    None => continue,
                    Some(s) => s,
                },
            };

            let first_asset = match assets.first() {
                None => continue,
                Some(s) => s,
            };

            let download_url = match first_asset.get("browser_download_url") {
                None => continue,
                Some(s) => match s.as_str() {
                    None => continue,
                    Some(s) => s,
                },
            };
            // println!("Candidate Release: {}", download_url);
            tag_urls.insert(release.to_owned(), download_url.to_owned());
        }

        if let Some((_k, v)) = tag_urls.into_iter().last() {
            // println!("Found candidate entry: v{} url {}", _k, v);
            return Ok(v);
        }
        Err(format!("No toolchains found for Rust {}", target_prefix))
    }
    let toolchain_url = get_toolchain_url(target, ver.major, ver.minor, ver.patch)?;

    println!("Attempting to install toolchain for {} into {}", target, toolchain_path);
    println!("Downloading toolchain from {}...", toolchain_url);

    print!("Download in progress...");
    std::io::stdout().flush().ok();
    let mut zip_data = vec![];
    {
        let agent = ureq::builder().build();

        let mut freader = agent.get(&toolchain_url).call().map_err(|e| format!("{}", e))?.into_reader();
        freader.read_to_end(&mut zip_data).map_err(|e| format!("{}", e))?;
        println!();
    }
    println!("Download successful. Total data size is {} bytes", zip_data.len());

    /// Extract the zipfile to the target directory, ensuring that all files
    /// contained within are created.
    fn extract_zip<P: Read + std::io::Seek, P2: AsRef<Path>>(
        archive_data: P,
        extract_to: P2,
    ) -> Result<(), String> {
        let mut archive =
            zip::ZipArchive::new(archive_data).map_err(|e| format!("unable to extract zip: {}", e))?;
        for i in 0..archive.len() {
            let mut entry_in_archive =
                archive.by_index(i).map_err(|e| format!("unable to locate file index {}: {}", i, e))?;
            // println!(
            //     "Trying to extract file {}",
            //     entry_in_archive.mangled_name().display()
            // );

            let output_path = extract_to.as_ref().join(entry_in_archive.mangled_name());
            if entry_in_archive.is_dir() {
                std::fs::create_dir_all(&output_path)
                    .map_err(|e| format!("unable to create directory {}: {}", output_path.display(), e))?;
            } else {
                // Create the parent directory if necessary
                if let Some(parent) = output_path.parent() {
                    if !parent.exists() {
                        std::fs::create_dir_all(parent).map_err(|e| {
                            format!("unable to create directory {}: {}", output_path.display(), e)
                        })?;
                    }
                }
                let mut outfile = File::create(&output_path)
                    .map_err(|e| format!("unable to create file {}: {}", output_path.display(), e))?;
                std::io::copy(&mut entry_in_archive, &mut outfile).map_err(|e| {
                    format!("unable to write extracted file {}: {}", output_path.display(), e)
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

/// Regenerate the locales files. This is only done when the command is explicitly run.
pub(crate) fn generate_locales() -> Result<(), std::io::Error> {
    let ts = filetime::FileTime::from_system_time(std::time::SystemTime::now());
    filetime::set_file_mtime("locales/src/lib.rs", ts)?;
    let mut path = project_root();
    path.push("locales");
    let status = Command::new(cargo()).current_dir(path).args(["build", "--package", "locales"]).status()?;
    if !status.success() {
        return Err(std::io::Error::new(std::io::ErrorKind::Other, "Couldn't generate the locales"));
    }
    Ok(())
}

/// Import the Wycheproof test vectors
pub(crate) fn wycheproof_import() -> Result<(), crate::DynError> {
    let input_file = "tools/wycheproof-import/x25519_test.json";
    let output_file = "services/shellchat/src/cmds/x25519_test.bin";
    let status = Command::new(cargo())
        .current_dir(project_root())
        .args(["run", "--package", "wycheproof-import", "--", input_file, output_file])
        .status()?;
    if !status.success() {
        return Err("wycheproof-import failed. If any, the output will not be usable.".into());
    }

    println!();
    println!("Wrote wycheproof x25519 testvectors to '{}'.", output_file);

    Ok(())
}

pub(crate) fn track_language_changes(last_lang: &str) -> Result<(), crate::DynError> {
    let last_config = "target/LAST_LANG";
    let mut contents = String::new();

    let changed = match OpenOptions::new().read(true).open(last_config) {
        Ok(mut file) => {
            file.read_to_string(&mut contents).unwrap();
            contents != last_lang
        }
        _ => true,
    };
    if changed {
        println!("Locale language changed to {}", last_lang);
        let mut file = OpenOptions::new().create(true).write(true).truncate(true).open(last_config).unwrap();
        write!(file, "{}", last_lang).unwrap();
        generate_locales()?
    } else {
        println!("No change to the target locale language of {}", contents);
    }
    Ok(())
}
