use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use base64::{Engine as _, engine::general_purpose};
use clap::{ArgGroup, Parser};
use ctap_hid_fido2::fidokey::get_assertion::get_assertion_params::Assertion;
use ctap_hid_fido2::{Cfg, FidoKeyHidFactory, fidokey::GetAssertionArgsBuilder};
use serde::{Deserialize, Serialize};

/// FIDO Signer - Sign messages or files using FIDO credentials
#[derive(Parser, Debug)]
#[command(name = "fido-signer")]
#[command(author, version, about, long_about = None)]
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
    let data_to_sign: Vec<u8> = if let Some(message_b64) = args.message {
        // Decode message from base64
        general_purpose::STANDARD
            .decode(&message_b64)
            .map_err(|e| format!("Failed to decode message: {}", e))?
    } else if let Some(file_path) = args.file {
        // Read file contents
        fs::read(&file_path).map_err(|e| format!("Failed to read file '{}': {}", file_path.display(), e))?
    } else {
        // This shouldn't happen due to clap's ArgGroup, but handle it just in case
        return Err("Either --message or --file must be provided".into());
    };

    println!("Credential file: {:?}", args.credential_file);
    println!("Credential ID (decoded): {} bytes", credential_id.len());
    println!("PIN: [REDACTED]"); // Don't print the actual PIN
    println!("Data to sign: {} bytes", data_to_sign.len());

    // Sign the hash
    let assertion = sign_ed25519_hash(&credential_id, &data_to_sign, &pin)?;

    println!("\n✓ Signing successful!");
    println!("Signature length: {} bytes", assertion.signature.len());

    if assertion.signature.len() == 64 {
        println!("✓ Valid Ed25519 signature");
        // Ed25519 signature components
        let (r, s) = assertion.signature.split_at(32);
        println!("  R: {}", hex::encode(r));
        println!("  S: {}", hex::encode(s));
        println!("  auth_data: {}", hex::encode(&assertion.auth_data));

        println!("  signature: {}", general_purpose::STANDARD.encode(assertion.signature));
        println!("  auth_data: {}", general_purpose::STANDARD.encode(&assertion.auth_data));
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
