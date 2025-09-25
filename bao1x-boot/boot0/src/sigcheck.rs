use alloc::string::String;
use core::convert::TryInto;

use digest::Digest;
use sha2_bao1x::Sha512;

#[repr(C)]
struct SignatureInFlash {
    pub _jal_instruction: u32,
    pub version: u32,
    pub signed_len: u32,
    pub signature: [u8; 64],
    pub pubkey: [u8; 32],
}

pub fn validate_image(img_offset: *const u32, fs_prehash: &mut [u8; 64]) -> Result<(), String> {
    // conjure the signature struct directly out of memory. super unsafe.
    let sig_ptr = img_offset as *const SignatureInFlash;
    let sig: &SignatureInFlash = unsafe { sig_ptr.as_ref().unwrap() };

    let signed_len = sig.signed_len;
    let image: &[u8] = unsafe {
        core::slice::from_raw_parts(
            (img_offset as usize + crate::SIGBLOCK_LEN + size_of::<bao1x_api::StaticsInRom>()) as *const u8,
            signed_len as usize,
        )
    };

    let verifying_key =
        ed25519_dalek::VerifyingKey::from_bytes(&sig.pubkey).or(Err(String::from("invalid public key")))?;

    // extract the version and length from the signed region
    crate::println!("signed_len: {}, image_len: {}", signed_len, image.len());
    let protected_version =
        u32::from_le_bytes(image[signed_len as usize - 8..signed_len as usize - 4].try_into().unwrap());
    let protected_len = u32::from_le_bytes(image[signed_len as usize - 4..].try_into().unwrap());
    // check that the signed versions match the version reported in the header
    if sig.version != 2 || (sig.version != protected_version) {
        crate::println!("version {}, protected_version {}", sig.version, protected_version);
        return Err(String::from("invalid sigblock version"));
    }
    if protected_len != signed_len - 4 {
        return Err(String::from("header length doesn't match protected length"));
    }

    let ed25519_signature = ed25519_dalek::Signature::from(sig.signature);
    crate::println!("Checking signature...");
    let mut h: Sha512 = Sha512::new();
    h.update(&image);
    // The prehash needs to be finalized before we create a new hasher instance. We
    // only have one hardware hasher available.
    if verifying_key.verify_prehashed(h.clone(), None, &ed25519_signature).is_ok() {
        let prehash = h.finalize();
        fs_prehash.copy_from_slice(prehash.as_slice());
        Ok(())
    } else {
        Err(String::from("sigcheck failed"))
    }
}
