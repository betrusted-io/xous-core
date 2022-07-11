use clap::{crate_version, App, Arg};
use std::io::{Read, Write, ErrorKind, Error};
use std::path::Path;

use ring::signature::Ed25519KeyPair;

const DEVKEY_PATH: &str = "devkey/dev.key";
const LOADER_VERSION: u32 = 1;

use xous_semver::SemVer;

fn image_sign<S, T>(
    input: &S,
    output: &T,
    private_key: &pem::Pem,
    defile: bool,
    minver: &Option<SemVer>,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: AsRef<Path>,
    T: AsRef<Path>,
{
    let mut source = vec![];
    let mut source_file = std::fs::File::open(input)?;
    let mut dest_file = std::fs::File::create(output)?;
    source_file.read_to_end(&mut source)?;

    // Append version information to the binary. Appending it here means it is part
    // of the signed bundle.
    let minver_bytes = if let Some(mv) = minver {mv.into()} else {[0u8; 16]};
    let semver: [u8; 16] = SemVer::from_git()
        .map_err(|_| Error::new(ErrorKind::Other, "error parsing current Git rev"))?
        .into();
    // extra data appended here needs to be reflected in two places in Xous:
    // 1. root-keys/src/implementation.rs @ sign-loader()
    // 2. graphics-server/src/main.rs @ Some(Opcode::BulkReadfonts)
    // This is because memory ownership is split between two crates for performance reasons:
    // the direct memory page of fonts belongs to the graphics server, to avoid having to send
    // a message on every font lookup. However, the keys reside in root-keys, so therefore,
    // a bulk read operation has to shuttle font data back to the root-keys crate. Of course,
    // the appended metadata is in the font region, so, this data has to be shuttled back.
    // The graphics server is also entirely naive to how much cryptographic data is in the font
    // region, and I think it's probably better for it to stay that way.
    source.append(&mut minver_bytes.to_vec());
    source.append(&mut semver.to_vec());
    for &b in LOADER_VERSION.to_le_bytes().iter() {
        source.push(b);
    }
    for &b in (source.len() as u32).to_le_bytes().iter() {
        source.push(b);
    }

    // NOTE NOTE NOTE
    // can't find a good ASN.1 ED25519 key decoder, just relying on the fact that the last
    // 32 bytes are "always" the private key. always? the private key?
    let signing_key = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&private_key.contents)
        .map_err(|e| format!("{}", e))?;
    let signature = signing_key.sign(&source);

    dest_file.write_all(&LOADER_VERSION.to_le_bytes())?;
    dest_file.write_all(&(source.len() as u32).to_le_bytes())?;

    // Write the signature data
    let signature_u8 = &signature.as_ref();
    dest_file.write_all(signature_u8)?;

    // Pad the first sector to 4096 bytes.
    let mut v = vec![];
    v.resize(4096 - 4 - 4 - signature_u8.len(), 0);
    dest_file.write_all(&v)?;

    // Fill the remainder of the source data

    if defile {
        println!("WARNING: defiling the loader image. This corrupts the binary and should cause it to fail the signature check.");
        source[16778] ^= 0x1 // flip one bit at some random offset
    }

    dest_file.write_all(&source)?;

    Ok(())
}

fn load_pem(src: &str) -> Result<pem::Pem, Box<dyn std::error::Error>> {
    let mut input = vec![];
    let mut pemfile = std::fs::File::open(src)?;
    pemfile.read_to_end(&mut input)?;

    Ok(pem::parse(input)?)
}

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
            Arg::with_name("defile").help(
                "patch the resulting image, to create a test file to catch signature failure",
            ),
        )
        .get_matches();

    let minver = if let Some(minver_str) = matches.value_of("min-xous-ver") {
        Some(
            SemVer::from_str(minver_str)
            .map_err(|_| Error::new(ErrorKind::InvalidInput, "Minimum version semver format incorrect"))?
        )
    } else {
        None
    };

    // Sign the loader, if an output file was specified
    if let Some(loader_output) = matches.value_of("loader-output") {
        let loader_key = matches
            .value_of("loader-key")
            .expect("no loader key specified");
        let loader_image = matches
            .value_of("loader-image")
            .expect("no loader image specified");

        let loader_pkey = load_pem(loader_key)?;
        if loader_pkey.tag != "PRIVATE KEY" {
            println!("Loader key was a {}, not a PRIVATE KEY", loader_pkey.tag);
            Err("invalid loader private key type")?;
        }
        println!("Signing loader");
        image_sign(
            &loader_image,
            &loader_output,
            &loader_pkey,
            matches.is_present("defile"),
            &minver,
        )?;
    }

    if let Some(kernel_output) = matches.value_of("kernel-output") {
        let kernel_key = matches
            .value_of("kernel-key")
            .expect("no kernel key specified");
        let kernel_image = matches
            .value_of("kernel-image")
            .expect("no kernel image specified");

        let kernel_pkey = load_pem(kernel_key)?;
        if kernel_pkey.tag != "PRIVATE KEY" {
            println!("Kernel key was a {}, not a PRIVATE KEY", kernel_pkey.tag);
            Err("invalid kernel private key type")?;
        }
        println!("Signing kernel");
        image_sign(
            &kernel_image,
            &kernel_output,
            &kernel_pkey,
            matches.is_present("defile"),
            &minver,
        )?;
    }
    Ok(())
}
