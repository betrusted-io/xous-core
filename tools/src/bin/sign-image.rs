use clap::{crate_version, App, Arg};
use std::io::{Read, Write};
use std::path::Path;

use ring::signature::Ed25519KeyPair;

const DEVKEY_PATH: &str = "devkey/dev.key";
const LOADER_VERSION: u32 = 1;

fn loader_sign<S, T>(
    input: &S,
    output: &T,
    private_key: &pem::Pem,
    defile: bool,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: AsRef<Path>,
    T: AsRef<Path>,
{
    let mut source = vec![];
    let mut source_file = std::fs::File::open(input)?;
    let mut dest_file = std::fs::File::create(output)?;
    source_file.read_to_end(&mut source)?;
    for b in LOADER_VERSION.to_le_bytes() {
        source.push(b);
    }
    for b in (source.len() as u32).to_le_bytes() {
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
                .takes_value(true)
                .required(true)
                .help("kernel signing key")
                .value_name("kernel signing key")
                .default_value(DEVKEY_PATH),
        )
        .arg(
            Arg::with_name("loader-output")
                .takes_value(true)
                .required(true)
                .value_name("loader output image")
                .help("loader output image")
                .default_value("target/riscv32imac-unknown-none-elf/release/loader.bin"),
        )
        .arg(
            Arg::with_name("defile").help(
                "patch the resulting image, to create a test file to catch signature failure",
            ),
        )
        .get_matches();

    let loader_key = matches
        .value_of("loader-key")
        .expect("no loader key specified");
    let loader_output = matches
        .value_of("loader-output")
        .expect("no output specified");
    let loader_image = matches
        .value_of("loader-image")
        .expect("no loader image specified");

    let loader_pkey = load_pem(loader_key)?;
    if loader_pkey.tag != "PRIVATE KEY" {
        println!("Loader key was a {}, not a PRIVATE KEY", loader_pkey.tag);
        Err("invalid pkey type")?;
    }

    loader_sign(
        &loader_image,
        &loader_output,
        &loader_pkey,
        matches.is_present("defile"),
    )?;
    Ok(())
}
