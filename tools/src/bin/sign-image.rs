use std::io::{Error, ErrorKind};

use clap::{App, Arg, crate_version};
use tools::sign_image::{load_pem, sign_file};

const DEVKEY_PATH: &str = "devkey/dev.key";

use xous_semver::SemVer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = App::new("sign-image")
        .version(crate_version!())
        .author("Sean Cross <sean@xobs.io>")
        .about("Sign binary images for Precursor")
        .arg(
            Arg::with_name("loader-image")
                .long("loader-image")
                .help("loader image")
                .value_name("loader image")
                .takes_value(true)
                .required(true)
                .default_value("target/riscv32imac-unknown-none-elf/release/loader_presign.bin"),
        )
        .arg(
            Arg::with_name("kernel-image")
                .long("kernel-image")
                .help("kernel image")
                .value_name("kernel image")
                .takes_value(true)
                .default_value("target/riscv32imac-unknown-none-elf/release/xous_presign.img")
                .required(true),
        )
        .arg(
            Arg::with_name("loader-key")
                .long("loader-key")
                .help("loader signing key")
                .takes_value(true)
                .value_name("loader signing key")
                .default_value(DEVKEY_PATH),
        )
        .arg(
            Arg::with_name("kernel-key")
                .long("kernel-key")
                .takes_value(true)
                .required(true)
                .help("kernel signing key")
                .value_name("kernel signing key")
                .default_value(DEVKEY_PATH),
        )
        .arg(
            Arg::with_name("loader-output")
                .long("loader-output")
                .takes_value(true)
                .value_name("loader output image")
                .help("loader output image"),
        )
        .arg(
            Arg::with_name("kernel-output")
                .long("kernel-output")
                .takes_value(true)
                .value_name("kernel output image")
                .help("kernel output image"),
        )
        .arg(
            Arg::with_name("min-xous-ver")
                .long("min-xous-ver")
                .help("Minimum Xous version for cross-compatibility")
                .value_name("min xous ver")
                .takes_value(true)
                .default_value("v0.9.8-790")
                .required(true),
        )
        .arg(
            Arg::with_name("defile")
                .help("patch the resulting image, to create a test file to catch signature failure"),
        )
        .arg(
            Arg::with_name("with-jump")
                .long("with-jump")
                .takes_value(false)
                .help("Insert a jump instruction in the signature block"),
        )
        .get_matches();

    let minver =
        if let Some(minver_str) = matches.value_of("min-xous-ver") {
            Some(SemVer::from_str(minver_str).map_err(|_| {
                Error::new(ErrorKind::InvalidInput, "Minimum version semver format incorrect")
            })?)
        } else {
            None
        };

    // Sign the loader, if an output file was specified
    if let Some(loader_output) = matches.value_of("loader-output") {
        let loader_key = matches.value_of("loader-key").expect("no loader key specified");
        let loader_image = matches.value_of("loader-image").expect("no loader image specified");

        let loader_pkey = load_pem(loader_key)?;
        if loader_pkey.tag != "PRIVATE KEY" {
            println!("Loader key was a {}, not a PRIVATE KEY", loader_pkey.tag);
            Err("invalid loader private key type")?;
        }
        println!("Signing loader");
        sign_file(
            &loader_image,
            &loader_output,
            &loader_pkey,
            matches.is_present("defile"),
            &minver,
            false,
            matches.is_present("with-jump"),
        )?;
    }

    if let Some(kernel_output) = matches.value_of("kernel-output") {
        let kernel_key = matches.value_of("kernel-key").expect("no kernel key specified");
        let kernel_image = matches.value_of("kernel-image").expect("no kernel image specified");

        let kernel_pkey = load_pem(kernel_key)?;
        if kernel_pkey.tag != "PRIVATE KEY" {
            println!("Kernel key was a {}, not a PRIVATE KEY", kernel_pkey.tag);
            Err("invalid kernel private key type")?;
        }
        println!("Signing kernel");
        sign_file(
            &kernel_image,
            &kernel_output,
            &kernel_pkey,
            matches.is_present("defile"),
            &minver,
            true,
            matches.is_present("with-jump"),
        )?;
    }
    Ok(())
}
