use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::mem;
use std::path::Path;
use std::path::PathBuf;

use bao1x_api::signatures::SignatureInFlash;
use base64::{Engine as _, engine::general_purpose};
use clap::{ArgGroup, Parser};
use ctap_hid_fido2::fidokey::get_assertion::get_assertion_params::Assertion;
use ctap_hid_fido2::{Cfg, FidoKeyHidFactory, fidokey::GetAssertionArgsBuilder};
use digest::Digest;
use serde::{Deserialize, Serialize};
use sha2::Sha512;

/// FIDO Signer - Sign messages or files using FIDO credentials
#[derive(Parser, Debug)]
#[command(
    name = "fido-signer",
    author = "bunnie",
    about = "Sign messages and bao1x image files with a FIDO token"
)]
#[command(version, long_about = None)]
#[command(group(
    ArgGroup::new("input")
        .required(true)
        .args(["message", "file"])
))]
struct Args {
    /// Path to the credential file (JSON format)
    #[arg(short = 'c', long, value_name = "FILE")]
    credential_file: PathBuf,

    /// Message to sign (base64 encoded)
    #[arg(short = 'm', long, value_name = "BASE64_MESSAGE", conflicts_with = "file")]
    message: Option<String>,

    /// File to sign
    #[arg(short = 'f', long, value_name = "FILE", conflicts_with = "message")]
    file: Option<PathBuf>,

    /// Function code e.g. partition - if provided, creates a uf2 file
    #[arg(short = 'p', long = "function-code", value_name = "CODE")]
    function_code: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Credentials {
    credential_id: String, // Base64 encoded, will be decoded to Vec<u8>
    pin: String,           // UTF-8 string
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Ed25519 Signing with FIDO2 Token");
    println!("================================\n");

    // Parse command line arguments
    let args = Args::parse();

    // Load and parse the credential file
    let credential_data = fs::read_to_string(&args.credential_file)
        .map_err(|e| format!("Failed to read credential file: {}", e))?;

    let credentials: Credentials = serde_json::from_str(&credential_data)
        .map_err(|e| format!("Failed to parse credential JSON: {}", e))?;

    // Decode the credential_id from base64
    let credential_id: Vec<u8> = general_purpose::STANDARD
        .decode(&credentials.credential_id)
        .map_err(|e| format!("Failed to decode credential_id: {}", e))?;

    // Get the PIN as a string (already UTF-8)
    let pin: String = credentials.pin;

    // Determine what to sign
    let mut is_bao1x = false;
    let mut offset = 0;
    let data_to_sign: Vec<u8> = if let Some(message_b64) = args.message {
        // Decode message from base64 - used for testing signing operations with small pieces of temporary
        // data
        general_purpose::STANDARD
            .decode(&message_b64)
            .map_err(|e| format!("Failed to decode message: {}", e))?
    } else if let Some(ref file_path) = args.file {
        // Read file contents.
        let file = fs::read(&file_path)
            .map_err(|e| format!("Failed to read file '{}': {}", file_path.display(), e))?;

        if let Some(ref code) = args.function_code {
            // swap is a special case, its signature header is shifted in the image due to the presence
            // of the swap metadata header
            if code == "swap" {
                offset = bao1x_api::offsets::baosec::SWAP_HEADER_LEN - bao1x_api::signatures::SIGBLOCK_LEN;
            }
        }

        // Check to see if this is a signed bao1x binary, by looking at the magic number.
        let mut sig = SignatureInFlash::default();
        sig.as_mut().copy_from_slice(&file[offset..offset + size_of::<SignatureInFlash>()]);
        if sig.sealed_data.magic == bao1x_api::signatures::MAGIC_NUMBER {
            is_bao1x = true;
            let mut h: Sha512 = Sha512::new();
            // hash the sealed region
            h.update(&file[offset + SignatureInFlash::sealed_data_offset()..]);
            // finalize it and send it on for signing
            h.finalize().as_slice().to_vec()
        } else {
            file
        }
    } else {
        // This shouldn't happen due to clap's ArgGroup, but handle it just in case
        return Err("Either --message or --file must be provided".into());
    };

    println!("Credential file: {:?}", args.credential_file);
    println!("Credential ID (decoded): {} bytes", credential_id.len());
    println!("PIN: [REDACTED]"); // Don't print the actual PIN
    println!("Data to sign: {} bytes", data_to_sign.len());

    // Sign the hash
    // println!("Signing {:x?}", data_to_sign);
    // let mut h_debug = sha2::Sha256::new();
    // h_debug.update(&data_to_sign);
    // let h_debug_final = h_debug.finalize();
    // println!("hashed hash: {:x?}", h_debug_final.as_slice());
    let assertion = sign_ed25519_hash(&credential_id, &data_to_sign, &pin)?;

    println!("\n* Signing successful!");
    println!("Signature length: {} bytes", assertion.signature.len());

    if assertion.signature.len() == 64 {
        println!("* Valid Ed25519 signature");
        // Ed25519 signature components
        let (r, s) = assertion.signature.split_at(32);
        println!("  R (hex): {}", hex::encode(r));
        println!("  S (hex): {}", hex::encode(s));
        println!("  auth_data (hex): {}", hex::encode(&assertion.auth_data));

        println!("  signature (b64): {}", general_purpose::STANDARD.encode(&assertion.signature));
        println!("  auth_data (b64): {}", general_purpose::STANDARD.encode(&assertion.auth_data));
    }

    if is_bao1x {
        if let Some(ref file_path) = args.file {
            println!("Patching image file with signature...");
            patch_signature_in_file(&file_path, &assertion.signature, &assertion.auth_data, offset)?;
        }
        if let Some(function_code) = args.function_code {
            if let Some(input) = args.file {
                let output = input.with_extension("uf2");
                println!("Building image file for partition {}, writing to {:?}...", function_code, output);
                convert_to_uf2(&input, &output, Some(&function_code), None)?
            }
        }
    }

    Ok(())
}

// Using the ctap-hid crate (Pure Rust, no C dependencies)
// #[cfg(feature = "pure-rust")]
fn sign_ed25519_hash(
    credential_id: &[u8],
    hash: &[u8],
    pin: &str,
) -> Result<Assertion, Box<dyn std::error::Error>> {
    let cfg = Cfg::init();

    let mut devices = ctap_hid_fido2::get_fidokey_devices();
    if devices.is_empty() {
        return Err("No FIDO2 devices found".into());
    }

    let device = devices.pop().unwrap();
    let fidokey = FidoKeyHidFactory::create_by_params(&[device.param], &cfg).unwrap();
    // println!("Using key {:?}", fidokey.get_info().unwrap());

    let assertion = GetAssertionArgsBuilder::new("ssh:", hash).pin(pin).credential_id(credential_id).build();
    match fidokey.get_assertion_with_args(&assertion) {
        Ok(mut a) => {
            println!("Key used {} times", a[0].sign_count);
            Ok(a.pop().unwrap())
        }
        Err(e) => Err(e.try_into().unwrap()),
    }
}

fn patch_signature_in_file(
    file_path: &PathBuf,
    signature: &Vec<u8>,
    auth_data: &Vec<u8>,
    offset: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    // Open file with read and write permissions
    let mut file = OpenOptions::new().read(true).write(true).open(file_path)?;

    let mut signature_struct = read_header_from_file(&mut file, offset)?;

    signature_struct.signature.copy_from_slice(&signature);
    if auth_data.len() > signature_struct.aad.len() {
        println!(
            "Auth data returned by key is {} bytes and exceeds the capacity {} of the signature record",
            auth_data.len(),
            signature_struct.aad.len()
        );
        return Err(String::from("Auth data exceeds capacity of the signature field").into());
    }
    signature_struct.aad[..auth_data.len()].copy_from_slice(&auth_data);
    signature_struct.aad_len = auth_data.len() as u32;

    write_header_to_file(&mut file, &signature_struct, offset)?;

    Ok(())
}

fn read_header_from_file(file: &mut std::fs::File, offset: usize) -> Result<SignatureInFlash, Box<dyn std::error::Error>> {
    // Ensure we're at the beginning of the file
    file.seek(SeekFrom::Start(offset as u64))?;

    // Create a buffer for the struct
    let struct_size = mem::size_of::<SignatureInFlash>();
    let mut buffer = vec![0u8; struct_size];

    // Read exactly the size of the struct
    file.read_exact(&mut buffer)?;

    let mut signature_struct = SignatureInFlash::default();
    assert!(
        signature_struct.sealed_data.magic == bao1x_api::signatures::MAGIC_NUMBER,
        "Magic number does not match in bao header file!"
    );

    signature_struct.as_mut().copy_from_slice(&buffer);

    Ok(signature_struct)
}

fn write_header_to_file(
    file: &mut std::fs::File,
    signature: &SignatureInFlash,
    offset: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    // Seek back to the beginning where the struct starts
    file.seek(SeekFrom::Start(offset as u64))?;

    // Write the bytes back to the file
    file.write_all(signature.as_ref())?;

    // Ensure the data is flushed to disk
    file.flush()?;

    Ok(())
}

// ----- code below is vendored in from tools/src/sign_image.rs. Maybe this should be a library,
// but let's see what the e2e integration looks like before we puzzle on how to librarify-this.
pub fn convert_to_uf2<S, T>(
    input: &S,
    output: &T,
    function_code: Option<&str>,
    offset: Option<usize>,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: AsRef<Path>,
    T: AsRef<Path>,
{
    let mut source = vec![];
    let mut source_file = std::fs::File::open(input)?;
    let mut dest_file = std::fs::File::create(output)?;
    source_file.read_to_end(&mut source)?;

    let app_start_addr = match function_code {
        Some("boot0") => bao1x_api::BOOT0_START,
        Some("boot1") => bao1x_api::BOOT1_START,
        Some("loader") => bao1x_api::LOADER_START,
        Some("baremetal") => bao1x_api::BAREMETAL_START,
        Some("kernel") => bao1x_api::KERNEL_START,
        Some("swap") => bao1x_api::SWAP_START_UF2,
        Some("app") => bao1x_api::dabao::APP_RRAM_START,
        _ => return Err(String::from("UF2 Image Requires a function code").into()),
    };

    match bin_to_uf2(
        &source,
        bao1x_api::BAOCHIP_1X_UF2_FAMILY,
        app_start_addr as u32 + offset.unwrap_or(0) as u32,
    ) {
        Ok(u2f_blob) => {
            dest_file.write_all(&u2f_blob)?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

use byteorder::{LittleEndian, WriteBytesExt};

const UF2_MAGIC_START0: u32 = 0x0A324655; // "UF2\n"
const UF2_MAGIC_START1: u32 = 0x9E5D5157; // Randomly selected
const UF2_MAGIC_END: u32 = 0x0AB16F30; // Ditto
// Vendored in from https://github.com/sajattack/uf2conv-rs/blob/master/lib/src/lib.rs
// The code is MIT-licensed, and authored by Paul Sajna
pub fn bin_to_uf2(bytes: &Vec<u8>, family_id: u32, app_start_addr: u32) -> Result<Vec<u8>, std::io::Error> {
    let datapadding = [0u8; 512 - 256 - 32 - 4];
    let nblocks: u32 = ((bytes.len() + 255) / 256) as u32;
    let mut outp: Vec<u8> = Vec::new();
    for blockno in 0..nblocks {
        let ptr = 256 * blockno;
        let chunk = match bytes.get(ptr as usize..ptr as usize + 256) {
            Some(bytes) => bytes.to_vec(),
            None => {
                let mut chunk = bytes[ptr as usize..bytes.len()].to_vec();
                while chunk.len() < 256 {
                    chunk.push(0);
                }
                chunk
            }
        };
        let mut flags: u32 = 0;
        if family_id != 0 {
            flags |= 0x2000
        }

        // header
        outp.write_u32::<LittleEndian>(UF2_MAGIC_START0)?;
        outp.write_u32::<LittleEndian>(UF2_MAGIC_START1)?;
        outp.write_u32::<LittleEndian>(flags)?;
        outp.write_u32::<LittleEndian>(ptr + app_start_addr)?;
        outp.write_u32::<LittleEndian>(256)?;
        outp.write_u32::<LittleEndian>(blockno)?;
        outp.write_u32::<LittleEndian>(nblocks)?;
        outp.write_u32::<LittleEndian>(family_id)?;

        // data
        outp.write(&chunk)?;
        outp.write(&datapadding)?;

        // footer
        outp.write_u32::<LittleEndian>(UF2_MAGIC_END)?;
    }
    Ok(outp)
}
