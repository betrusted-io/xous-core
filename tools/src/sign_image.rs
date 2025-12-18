use std::io::{Read, Write};
use std::path::Path;

use bao1x_api::signatures::{
    FunctionCode, PADDING_LEN, SIGBLOCK_LEN, SealedFields, SignatureInFlash, UNSIGNED_LEN,
};
use ed25519_dalek::{DigestSigner, SigningKey};
use pkcs8::PrivateKeyInfo;
use pkcs8::der::Decodable;
use ring::signature::{Ed25519KeyPair, KeyPair};
use sha2::{Digest, Sha512};

#[repr(u32)]
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Version {
    Loader = 1,
    LoaderPrehash = 2,
    Bao1xV1 = 0x1_00,
}
use xous_semver::SemVer;

pub fn generate_jal_x0(signed_offset: isize) -> Result<u32, String> {
    // Check that offset is 2-byte aligned (even)
    if signed_offset & 1 != 0 {
        return Err("JAL offset must be 2-byte aligned (even)".to_string());
    }

    // Check that offset fits in 21-bit signed range
    // JAL can encode offsets from -2^20 to 2^20 - 2
    const MIN_OFFSET: isize = -(1 << 20); // -1048576
    const MAX_OFFSET: isize = (1 << 20) - 2; // 1048574

    if signed_offset < MIN_OFFSET || signed_offset > MAX_OFFSET {
        return Err(format!("JAL offset {} is out of range [{}, {}]", signed_offset, MIN_OFFSET, MAX_OFFSET));
    }

    let imm = signed_offset as u32;

    // Extract bit fields for JAL J-type encoding
    // JAL immediate format: [20|10:1|11|19:12]
    let imm_20 = (imm >> 20) & 1; // bit 20 -> instruction bit 31
    let imm_19_12 = (imm >> 12) & 0xFF; // bits 19:12 -> instruction bits 19:12
    let imm_11 = (imm >> 11) & 1; // bit 11 -> instruction bit 20
    let imm_10_1 = (imm >> 1) & 0x3FF; // bits 10:1 -> instruction bits 30:21

    // Assemble the JAL instruction
    // Format: imm[20] | imm[10:1] | imm[11] | imm[19:12] | rd | opcode
    let instruction = (imm_20 << 31) |      // imm[20] at bit 31
                     (imm_10_1 << 21) |    // imm[10:1] at bits 30:21
                     (imm_11 << 20) |      // imm[11] at bit 20
                     (imm_19_12 << 12) |   // imm[19:12] at bits 19:12
                     // rd = x0 = 0 (bits 11:7)
                     0x6F; // JAL opcode (0b1101111)

    Ok(instruction)
}

pub fn load_pem(src: &str) -> Result<pem::Pem, Box<dyn std::error::Error>> {
    let mut input = vec![];
    let mut pemfile = std::fs::File::open(src)?;
    pemfile.read_to_end(&mut input)?;

    Ok(pem::parse(input)?)
}

pub fn sign_image(
    source: &[u8],
    private_key: &pem::Pem,
    defile: bool,
    minver: &Option<SemVer>,
    semver: Option<[u8; 16]>,
    with_jump: bool,
    length: usize,
    version: Version,
    function_code: Option<&str>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut dest_file = vec![];

    // Append version information to the binary. Appending it here means it is part
    // of the signed bundle.
    let minver_bytes = if let Some(mv) = minver { mv.into() } else { [0u8; 16] };
    let semver: [u8; 16] = match semver {
        Some(semver) => semver,
        None => SemVer::from_git()?.into(),
    };

    match version {
        Version::Loader | Version::LoaderPrehash => {
            let mut source = source.to_owned();
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
            let prehash = match version {
                Version::Loader => false,
                Version::LoaderPrehash => true,
                _ => return Err(String::from("Unhandled image version").into()),
            };
            for &b in (version as u32).to_le_bytes().iter() {
                source.push(b);
            }
            for &b in (source.len() as u32).to_le_bytes().iter() {
                source.push(b);
            }

            let (signature, pubkey) = if prehash {
                // pre-hash the message
                let mut h: Sha512 = Sha512::new();
                h.update(&source);

                let pkinfo = PrivateKeyInfo::from_der(&private_key.contents).map_err(|e| format!("{}", e))?;
                // First 2 bytes of the `private_key` are a record specifier and length field. Check they are
                // correct.
                assert!(pkinfo.private_key[0] == 0x4);
                assert!(pkinfo.private_key[1] == 0x20);
                let mut secbytes = [0u8; 32];
                secbytes.copy_from_slice(&pkinfo.private_key[2..]);
                // Now we can use the private key data.
                let signing_key = SigningKey::from_bytes(&secbytes);

                // derive a private key
                let sk = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&private_key.contents)
                    .map_err(|e| format!("{}", e))?;
                let mut pubkey_bytes = [0u8; 32];
                pubkey_bytes.copy_from_slice(sk.public_key().as_ref());

                let sig = signing_key.sign_digest(h.clone()).to_bytes();
                (sig, pubkey_bytes)
            } else {
                // NOTE NOTE NOTE
                // can't find a good ASN.1 ED25519 key decoder, just relying on the fact that the last
                // 32 bytes are "always" the private key. always? the private key?
                let signing_key = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&private_key.contents)
                    .map_err(|e| format!("{}", e))?;
                let mut sig = [0u8; 64];
                sig.copy_from_slice(signing_key.sign(&source).as_ref());
                let mut pubkey_bytes = [0u8; 32];
                pubkey_bytes.copy_from_slice(signing_key.public_key().as_ref());
                (sig, pubkey_bytes)
            };

            let jal = generate_jal_x0(length as isize)?;
            // println!("offset {:x}, jal {:x}", length, jal);
            let extra_pad = if with_jump {
                dest_file.write_all(&jal.to_le_bytes())?;
                4
            } else {
                0
            };

            dest_file.write_all(&(version as u32).to_le_bytes())?;
            dest_file.write_all(&(source.len() as u32).to_le_bytes())?;

            // Write the signature data
            dest_file.write_all(&signature)?;

            // Write the public key - for now it's just an interim key, but this should be replaced with
            // the actual code signing key eventually. This is only relevant for baochip targets, Precursor
            // has this pre-burned into its KEYROM.
            dest_file.write_all(&pubkey)?;

            // Pad the first sector to length bytes.
            let mut v = vec![];
            v.resize(length - 4 - 4 - signature.len() - extra_pad - pubkey.len(), 0);
            dest_file.write_all(&v)?;

            // Fill the remainder of the source data

            if defile {
                println!(
                    "WARNING: defiling the loader image. This corrupts the binary and should cause it to fail the signature check."
                );
                source[16778] ^= 0x1 // flip one bit at some random offset
            }

            dest_file.write_all(&source)?;

            Ok(dest_file)
        }
        Version::Bao1xV1 => {
            let pkinfo = PrivateKeyInfo::from_der(&private_key.contents).map_err(|e| format!("{}", e))?;
            // First 2 bytes of the `private_key` are a record specifier and length field. Check they are
            // correct.
            assert!(pkinfo.private_key[0] == 0x4);
            assert!(pkinfo.private_key[1] == 0x20);
            let mut secbytes = [0u8; 32];
            secbytes.copy_from_slice(&pkinfo.private_key[2..]);
            // Now we can use the private key data.
            let signing_key = SigningKey::from_bytes(&secbytes);

            // This is handy code to remember - a quick way to get the public key from the private key. Just
            // in case I need this in the future to sanity check some values.
            /*
                let sk = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&private_key.contents)
                    .map_err(|e| format!("{}", e))?;
                let pubkey = sk.public_key();
            */

            let function_code = match function_code {
                Some("boot0") => FunctionCode::Boot0,
                Some("boot1") => FunctionCode::Boot1,
                Some("loader") => FunctionCode::Loader,
                Some("kernel") => FunctionCode::Kernel,
                Some("app") => FunctionCode::App,
                Some("swap") => FunctionCode::Swap,
                Some("baremetal") => FunctionCode::Baremetal,
                _ => panic!("Invalid function code"),
            };
            let mut header = SignatureInFlash::default();
            header.sealed_data.version = version as u32;
            header.sealed_data.signed_len = (source.len() + SIGBLOCK_LEN - UNSIGNED_LEN) as u32;
            header.sealed_data.function_code = function_code as u32;
            header.sealed_data.reserved = 0;
            header.sealed_data.min_semver = minver_bytes;
            header.sealed_data.semver = semver;

            // whack in all the public keys, defined in the bao1x-api crate
            for (dst, src) in
                header.sealed_data.pubkeys.iter_mut().zip(bao1x_api::pubkeys::PUBKEY_HEADER.iter())
            {
                dst.populate_from(src);
            }

            let mut protected = Vec::new();
            protected.extend_from_slice(header.sealed_data.as_ref());
            protected.resize(protected.len() + PADDING_LEN, 0);
            protected.extend_from_slice(&source);

            // pre-hash the message
            let mut h: Sha512 = Sha512::new();
            h.update(&protected);

            let sig = signing_key.sign_digest(h.clone()).to_bytes();
            header._jal_instruction = generate_jal_x0(SIGBLOCK_LEN as isize)?;
            header.signature.copy_from_slice(&sig);
            // no AAD on this type of signature
            header.aad_len = 0;

            // Write the header
            dest_file.write_all(&header.as_ref()[..UNSIGNED_LEN])?;
            // println!("dest_file wrote {}, {}", dest_file.len(), UNSIGNED_LEN);

            if defile {
                println!(
                    "WARNING: defiling the loader image. This corrupts the binary and should cause it to fail the signature check."
                );
                protected[size_of::<SealedFields>() + 16] ^= 0x1 // flip one bit in the zero-padding region
            }

            dest_file.write_all(&protected)?;

            if function_code == FunctionCode::Kernel {
                let target = bao1x_api::RRAM_STORAGE_LEN - (bao1x_api::KERNEL_START - bao1x_api::BOOT0_START);
                if dest_file.len() > target {
                    println!(
                        "ERROR: Xous RRAM image is too big to fit: {} bytes too large ({} bytes; {} limit)",
                        dest_file.len() - target,
                        dest_file.len(),
                        target,
                    );
                    return Err(String::from("Image doesn't fit").into());
                }
            }
            Ok(dest_file)
        }
    }
}

pub fn sign_file<S, T>(
    input: &S,
    output: &T,
    private_key: &pem::Pem,
    defile: bool,
    minver: &Option<SemVer>,
    version: Version,
    with_jump: bool,
    sector_length: usize,
    function_code: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: AsRef<Path>,
    T: AsRef<Path>,
{
    let mut source = vec![];
    let mut source_file = std::fs::File::open(input)?;
    let mut dest_file = std::fs::File::create(output)?;
    source_file.read_to_end(&mut source)?;

    let result = sign_image(
        &source,
        private_key,
        defile,
        minver,
        None,
        with_jump,
        sector_length,
        version,
        function_code,
    )?;
    dest_file.write_all(&result)?;
    Ok(())
}

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

    // maintenance note: there is a mirror of this code in signing/fido-signer/src/main.rs
    // if you're editing this, maybe consider also trying to librarify these routines?
    // the key challenge is that fido-signer has to be out of workspace because it relies
    // on cryptographic primitives that are pinned to versions that are not compatible
    // with the baochip hardware, e.g. the FIDO2 libraries pin to an incompatible version
    // with xous-core, so it's a little awkward spanning/sharing crates this way.
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
