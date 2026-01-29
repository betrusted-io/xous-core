use std::io::{Error, ErrorKind};

use clap::{App, Arg, crate_version};
use xous_tools::sign_image::{convert_to_uf2, load_pem, sign_file};

const DEVKEY_PATH: &str = "devkey/dev.key";

use std::str::FromStr;

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
        .arg(
            Arg::with_name("sig-length")
                .long("sig-length")
                .takes_value(true)
                .default_value("4096")
                .help("Change the length of the signature block. Defaults to 4096.")
                .required(false),
        )
        .arg(Arg::with_name("bao1x").long("bao1x").help("Generate images for the bao1x target"))
        .arg(
            Arg::with_name("function-code")
            .long("function-code")
            .takes_value(true)
            .help("Function code to embed in the signature block. Only meaningful in combination with --bao1x")
            .required(false)
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

    let sig_length = usize::from_str_radix(matches.value_of("sig-length").unwrap_or("4096"), 10)
        .expect("sig-length should be a decimal number");
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
        // bao1x can use pre-hash signatures because it's a clean-sheet bootloader
        // precursor uses the older style because we are avoiding updating the boot ROM in the SoC to avoid
        // bricking.
        let version = if matches.is_present("bao1x") {
            xous_tools::sign_image::Version::Bao1xV1
        } else {
            xous_tools::sign_image::Version::Loader
        };
        sign_file(
            &loader_image,
            &loader_output,
            &loader_pkey,
            matches.is_present("defile"),
            &minver,
            version,
            matches.is_present("with-jump"),
            sig_length,
            matches.value_of("function-code"),
        )?;

        if matches.is_present("bao1x") {
            if loader_output.ends_with(".img") || loader_output.ends_with(".bin") {
                let loader_uf2 = format!("{}uf2", &loader_output[..loader_output.len() - 3]);
                // generate a uf2 file
                convert_to_uf2(&loader_output, &loader_uf2, matches.value_of("function-code"), None)?;
                println!("Created UF2 at {}", loader_uf2);
            } else {
                Err(
                    "Can't generate UF2 file because the output file is not specified with a .img/.bin suffix",
                )?;
            }
        }
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
        let version = if matches.is_present("bao1x") {
            xous_tools::sign_image::Version::Bao1xV1
        } else {
            xous_tools::sign_image::Version::LoaderPrehash
        };
        sign_file(
            &kernel_image,
            &kernel_output,
            &kernel_pkey,
            matches.is_present("defile"),
            &minver,
            version,
            matches.is_present("with-jump"),
            sig_length,
            Some(matches.value_of("function-code").unwrap_or("kernel")),
        )?;

        if matches.is_present("bao1x") {
            if kernel_output.ends_with(".img") {
                let kernel_uf2 = format!("{}uf2", &kernel_output[..kernel_output.len() - 3]);
                // generate a uf2 file
                convert_to_uf2(&kernel_output, &kernel_uf2, matches.value_of("function-code"), None)?;
                println!("Created UF2 at {}", kernel_uf2);
            } else {
                Err(
                    "Can't generate UF2 file because the kernel output file is not specified with a .img suffix",
                )?;
            }
        }
    }
    Ok(())
}
