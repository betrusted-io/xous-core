use std::io::{Read, Write};
use std::path::Path;

use ed25519_dalek::{DigestSigner, SigningKey};
use pkcs8::PrivateKeyInfo;
use pkcs8::der::Decodable;
use ring::signature::Ed25519KeyPair;
use sha2::{Digest, Sha512};

const LOADER_VERSION: u32 = 1;
const LOADER_PREHASH_VERSION: u32 = 2;

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
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut source = source.to_owned();
    let mut dest_file = vec![];

    // Append version information to the binary. Appending it here means it is part
    // of the signed bundle.
    let minver_bytes = if let Some(mv) = minver { mv.into() } else { [0u8; 16] };
    let semver: [u8; 16] = match semver {
        Some(semver) => semver,
        None => SemVer::from_git()?.into(),
    };

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
    let signing_key =
        Ed25519KeyPair::from_pkcs8_maybe_unchecked(&private_key.contents).map_err(|e| format!("{}", e))?;
    let signature = signing_key.sign(&source);

    let jal = generate_jal_x0(length as isize)?;
    println!("offset {:x}, jal {:x}", length, jal);
    let extra_pad = if with_jump {
        dest_file.write_all(&jal.to_le_bytes())?;
        4
    } else {
        0
    };

    dest_file.write_all(&LOADER_VERSION.to_le_bytes())?;
    dest_file.write_all(&(source.len() as u32).to_le_bytes())?;

    // Write the signature data
    let signature_u8 = &signature.as_ref();
    dest_file.write_all(signature_u8)?;

    // Pad the first sector to length bytes.
    let mut v = vec![];
    v.resize(length - 4 - 4 - signature_u8.len() - extra_pad, 0);
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

pub fn sign_image_prehash(
    source: &[u8],
    private_key: &pem::Pem,
    defile: bool,
    minver: &Option<SemVer>,
    semver: Option<[u8; 16]>,
    with_jump: bool,
    length: usize,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut source = source.to_owned();
    let mut dest_file = vec![];

    // Append version information to the binary. Appending it here means it is part
    // of the signed bundle.
    let minver_bytes = if let Some(mv) = minver { mv.into() } else { [0u8; 16] };
    let semver: [u8; 16] = match semver {
        Some(semver) => semver,
        None => SemVer::from_git()?.into(),
    };

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
    for &b in LOADER_PREHASH_VERSION.to_le_bytes().iter() {
        source.push(b);
    }
    for &b in (source.len() as u32).to_le_bytes().iter() {
        source.push(b);
    }

    // pre-hash the message
    let mut h: Sha512 = Sha512::new();
    h.update(&source);

    let private_key = PrivateKeyInfo::from_der(&private_key.contents).map_err(|e| format!("{}", e))?;
    // First 2 bytes of the `private_key` are a record specifier and length field. Check they are correct.
    assert!(private_key.private_key[0] == 0x4);
    assert!(private_key.private_key[1] == 0x20);
    let mut secbytes = [0u8; 32];
    secbytes.copy_from_slice(&private_key.private_key[2..]);
    // Now we can use the private key data.
    let signing_key = SigningKey::from_bytes(&secbytes);
    let signature = signing_key.sign_digest(h);

    let jal = generate_jal_x0(length as isize)?;
    let extra_pad = if with_jump {
        dest_file.write_all(&jal.to_le_bytes())?;
        4
    } else {
        0
    };

    dest_file.write_all(&LOADER_PREHASH_VERSION.to_le_bytes())?;
    dest_file.write_all(&(source.len() as u32).to_le_bytes())?;

    // Write the signature data
    let signature_u8 = &signature.to_bytes();
    dest_file.write_all(signature_u8)?;

    // Pad the first sector to length bytes.
    let mut v = vec![];
    v.resize(length - 4 - 4 - signature_u8.len() - extra_pad, 0);
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

pub fn sign_file<S, T>(
    input: &S,
    output: &T,
    private_key: &pem::Pem,
    defile: bool,
    minver: &Option<SemVer>,
    use_prehash: bool,
    with_jump: bool,
    sector_length: usize,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: AsRef<Path>,
    T: AsRef<Path>,
{
    let mut source = vec![];
    let mut source_file = std::fs::File::open(input)?;
    let mut dest_file = std::fs::File::create(output)?;
    source_file.read_to_end(&mut source)?;

    let result = if use_prehash {
        sign_image_prehash(&source, private_key, defile, minver, None, with_jump, sector_length)?
    } else {
        sign_image(&source, private_key, defile, minver, None, with_jump, sector_length)?
    };
    dest_file.write_all(&result)?;
    Ok(())
}
