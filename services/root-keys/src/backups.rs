use zeroize::Zeroize;
use core::mem::size_of;
use core::ops::{Deref, DerefMut};
use aes_gcm_siv::{
    aead::{KeyInit, Aead, Payload},
    Aes256GcmSiv, Tag, Nonce
};
use subtle::ConstantTimeEq;
use crate::{BackupHeader, BackupOp};
use rand_core::RngCore;

const BACKUP_AAD: &'static str = "PDDB backup v0.1.0";

#[derive(Zeroize, Default)]
#[zeroize(drop)]
pub(crate) struct BackupKey(pub [u8;32]);

#[derive(Zeroize)]
#[zeroize(drop)]
pub(crate) struct KeyRomExport(pub [u32; 256]);
impl Default for KeyRomExport {
    fn default() -> Self {
        KeyRomExport([0u32; 256])
    }
}

/// This is the plaintext portion of the backup header
#[derive(Zeroize)]
#[zeroize(drop)]
#[repr(C, align(8))]
pub(crate) struct BackupDataPt {
    /// A sealed copy of the plaintext header, for validation purposes (plaintext can be tampered with)
    /// Note: the `op` field is allowed to be manipulated
    pub header: BackupHeader, // 144 bytes => 9 aes blocks
    /// exact copy of the KEYROM structure
    pub keyrom: [u32; 256], // 1024 bytes
    /// some reserved space for future things
    pub _reserved: [u8; 64],
}
impl Default for BackupDataPt {
    fn default() -> Self {
        BackupDataPt {
            header: BackupHeader::default(),
            keyrom: [0u32; 256],
            _reserved: [0u8; 64],
        }
    }
}
impl Deref for BackupDataPt {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self as *const BackupDataPt as *const u8, size_of::<BackupDataPt>())
                as &[u8]
        }
    }
}
impl DerefMut for BackupDataPt {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self as *mut BackupDataPt as *mut u8, size_of::<BackupDataPt>())
                as &mut [u8]
        }
    }
}

/// This is the ciphertext portion of the backup header. Note that there is an additional
/// 32-byte section that is a SHA512/256 hash of the pt+ct region, appended directly to
/// the end of the ct region.
#[repr(C, align(8))]
pub(crate) struct BackupDataCt {
    pub nonce: [u8; 12],
    pub ct_plus_mac: [u8; size_of::<BackupDataPt>() + size_of::<Tag>()], // should be 1232 + 16
    pub commit_nonce: [u8; 32],
    pub commitment: [u8; 32],
}
impl Default for BackupDataCt {
    fn default() -> Self {
        BackupDataCt {
            nonce: [0u8; 12],
            ct_plus_mac: [0u8; size_of::<BackupDataPt>() + size_of::<Tag>()],
            commit_nonce: [0u8; 32],
            commitment: [0u8; 32],
        }
    }
}
impl Deref for BackupDataCt {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self as *const BackupDataCt as *const u8, size_of::<BackupDataCt>())
                as &[u8]
        }
    }
}
impl DerefMut for BackupDataCt {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self as *mut BackupDataCt as *mut u8, size_of::<BackupDataCt>())
                as &mut [u8]
        }
    }
}

/// Derive a key commitment. This takes in a base `key`, which is 256 bits;
/// a `nonce` which is the 96-bit nonce used in the AES-GCM-SIV for a given block;
/// and `nonce_com` which is the commitment nonce, set at 256 bits.
/// The result is two tuples, (kenc, kcom).
fn kcom_func(
    key: &[u8; 32],
    nonce_com: &[u8; 32]
) -> (BackupKey, BackupKey) {
    use sha2::{FallbackStrategy, Sha512Trunc256};
    use digest::Digest;

    let mut h_enc = Sha512Trunc256::new_with_strategy(FallbackStrategy::SoftwareOnly);
    h_enc.update(key);
    // per https://eprint.iacr.org/2020/1456.pdf Table 4 on page 13 Type I Lenc
    h_enc.update([0x43, 0x6f, 0x6, 0xd6, 0xd, 0x69, 0x74, 0x01, 0x01]);
    h_enc.update(nonce_com);
    let k_enc = h_enc.finalize();

    let mut h_com = Sha512Trunc256::new_with_strategy(FallbackStrategy::SoftwareOnly);
    h_com.update(key);
    // per https://eprint.iacr.org/2020/1456.pdf Table 4 on page 13 Type I Lcom. Note one-bit difference in last byte.
    h_com.update([0x43, 0x6f, 0x6, 0xd6, 0xd, 0x69, 0x74, 0x01, 0x02]);
    h_com.update(nonce_com);
    let k_com = h_com.finalize();
    let mut kenc = BackupKey::default();
    let mut kcom = BackupKey::default();
    kenc.0.copy_from_slice(k_enc.as_slice());
    kcom.0.copy_from_slice(k_com.as_slice());
    (kenc, kcom)
}

pub(crate) fn create_backup(
    key: BackupKey,
    header: BackupHeader,
    keyrom: KeyRomExport,
) -> BackupDataCt {
    let xns = xous_names::XousNames::new().unwrap();
    let mut trng = trng::Trng::new(&xns).unwrap();

    // AES-GCM-SIV nonce
    let mut nonce: [u8; size_of::<Nonce>()] = [0; size_of::<Nonce>()];
    trng.fill_bytes(&mut nonce);

    // key commitment nonce
    let mut kcom_nonce = [0u8; 32];
    trng.fill_bytes(&mut kcom_nonce);

    let (kenc, kcom) = kcom_func(&key.0, &kcom_nonce);
    let cipher = Aes256GcmSiv::new(&kenc.0.into());

    // the backup data
    let mut backup_data_pt = BackupDataPt::default();
    backup_data_pt.header.deref_mut().copy_from_slice(header.deref());
    backup_data_pt.header.op = BackupOp::Archive;

    backup_data_pt.keyrom.copy_from_slice(&keyrom.0);

    // encrypt the backup data
    let ciphertext = cipher.encrypt(
        &nonce.into(),
        Payload {
            aad: BACKUP_AAD.as_bytes(),
            msg: backup_data_pt.deref(),
        }
    ).expect("couldn't encrypt data");

    log::debug!("commit_nonce: {:?}", &kcom_nonce);
    log::debug!("commit_nonce: {:x?}", &kcom_nonce);
    log::debug!("commit_key: {:?}", &kcom.0);
    log::debug!("commit_key: {:x?}", &kcom.0);
    log::debug!("header {}, ptlen {}, ctlen {}", size_of::<BackupHeader>(), size_of::<BackupDataPt>(), size_of::<BackupDataCt>());

    // copy to a ciphertext record
    let mut backup_ct = BackupDataCt::default();
    backup_ct.nonce.copy_from_slice(&nonce);
    backup_ct.ct_plus_mac.copy_from_slice(&ciphertext); // this will panic if we have the wrong CT size, and that's exactly what we want.
    backup_ct.commit_nonce.copy_from_slice(&kcom_nonce);
    backup_ct.commitment.copy_from_slice(&kcom.0);

    backup_ct
}

/// Returns `None` if the MAC or key commitment fail.
/// It is up to the caller to validate if the plaintext header matches the decrypted
/// version embedded in the return data.
pub(crate) fn restore_backup(
    key: &BackupKey,
    backup: &BackupDataCt
) -> Option<BackupDataPt> {
    let (kenc, kcom) = kcom_func(&key.0, &backup.commit_nonce);
    let cipher = Aes256GcmSiv::new(&kenc.0.into());

    // Attempt decryption. This is None on failure
    let plaintext = cipher.decrypt(
        Nonce::from_slice(&backup.nonce),
        Payload {
            aad: BACKUP_AAD.as_bytes(),
            msg: &backup.ct_plus_mac,
        }
    ).ok();

    if kcom.0.ct_eq(&backup.commitment).into() {
        if let Some(p) = plaintext {
            let mut pt = BackupDataPt::default();
            pt.deref_mut().copy_from_slice(&p); // panics if pt is the wrong length. we want that.
            Some(pt)
        } else {
            None
        }
    } else {
        None
    }
}