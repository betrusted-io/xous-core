mod oracle;
use oracle::*;
mod keywrap;
use keywrap::*;
pub use oracle::FpgaKeySource;

use utralib::generated::*;
use xous::KERNEL_BACKUP_OFFSET;
use crate::{api::*, backups};
use core::num::NonZeroUsize;
use num_traits::*;

use gam::modal::{Modal, Slider, ProgressBar, ActionType};
use locales::t;
use xous_semver::SemVer;

use crate::bcrypt::*;
use crate::api::PasswordType;

use core::convert::TryInto;
use ed25519_dalek::{Keypair, Signature, PublicKey, Signer, ExpandedSecretKey, SecretKey};
use curve25519_dalek::scalar::Scalar;
use curve25519_dalek::constants;
use sha2::{FallbackStrategy, Sha256, Sha512, Sha512Trunc256};
use digest::Digest;
use graphics_server::BulkRead;
use core::mem::size_of;
use core::cell::RefCell;

use aes::Aes256;
use aes::cipher::{KeyInit, BlockDecrypt, BlockEncrypt};
use cipher::generic_array::GenericArray;
use rand_core::RngCore;

use crate::{SignatureResult, GatewareRegion, MetadataInFlash, UpdateType};

use root_keys::key2bits::*;

// TODO: add hardware acceleration for BCRYPT so we can hit the OWASP target without excessive UX delay
const BCRYPT_COST: u32 = 7;   // 10 is the minimum recommended by OWASP; takes 5696 ms to verify @ 10 rounds; 804 ms to verify 7 rounds

/// Maximum number of times the global rollback limiter can be updated. Every time this is updated,
/// the firmware has to be re-signed, the gateware ROM re-injected, and the PDDB system key updated.
///
/// N.B.: As of Xous 0.9.6 we don't have a call to update the anti-rollback count, we have only provisioned for
/// that call to exist sometime in the future.
const MAX_ROLLBACK_LIMIT: u8 = 255;

/// Size of the total area allocated for signatures. It is equal to the size of one FLASH sector, which is the smallest
/// increment that can be erased.
const SIGBLOCK_SIZE: u32 = 0x1000;
/// location of the csr.csv that's appended on gateware images, used for USB updates.
const METADATA_OFFSET: usize = 0x27_6000;
#[allow(dead_code)]
/// location of the csr.csv that's appended on gateware images, used for USB updates.
const CSR_CSV_OFFSET: usize  = 0x27_7000;
/// offset of the gateware self-signature area
const SELFSIG_OFFSET: usize  = 0x27_F000;

/// This structure is mapped into the password cache page and can be zero-ized at any time
/// we avoid using fancy Rust structures because everything has to "make sense" after a forced zero-ization
/// The "password" here is generated as follows:
///   `user plaintext (up to first 72 bytes) -> bcrypt (24 bytes) -> sha512trunc256 -> [u8; 32]`
/// The final sha512trunc256 expansion is because we will use this to XOR against secret keys stored in
/// the KEYROM that may be up to 256 bits in length. For shorter keys, the hashed password is simply truncated.
#[repr(C)]
struct PasswordCache {
    hashed_boot_pw: [u8; 32],
    hashed_boot_pw_valid: u32, // non-zero for valid
    hashed_update_pw: [u8; 32],
    hashed_update_pw_valid: u32,
    fpga_key: [u8; 32],
    fpga_key_valid: u32,
}

#[repr(C)]
struct SignatureInFlash {
    pub version: u32,
    pub signed_len: u32,
    pub signature: [u8; 64],
}
pub enum SignatureType {
    Loader,
    Gateware,
    Kernel,
}

struct KeyRomLocs {}
#[allow(dead_code)]
impl KeyRomLocs {
    const FPGA_KEY:            u8 = 0x00;
    const SELFSIGN_PRIVKEY:    u8 = 0x08;
    const SELFSIGN_PUBKEY:     u8 = 0x10;
    const DEVELOPER_PUBKEY:    u8 = 0x18;
    const THIRDPARTY_PUBKEY:   u8 = 0x20;
    const USER_KEY:   u8 = 0x28;
    const PEPPER:     u8 = 0xf8;
    const FPGA_MIN_REV:   u8 = 0xfc;
    const LOADER_MIN_REV: u8 = 0xfd;
    const GLOBAL_ROLLBACK: u8 = 0xfe;
    const CONFIG:     u8 = 0xff;
}

pub struct KeyField {
    mask: u32,
    offset: u32,
}
impl KeyField {
    pub const fn new(width: u32, offset: u32) -> Self {
        let mask = (1 << width) - 1;
        KeyField {
            mask,
            offset,
        }
    }
    pub fn ms(&self, value: u32) -> u32 {
        let ms_le = (value & self.mask) << self.offset;
        ms_le.to_be()
    }
}
#[allow(dead_code)]
pub(crate) mod keyrom_config {
    use crate::KeyField;
    pub const VERSION_MINOR:       KeyField = KeyField::new(8, 0 );
    pub const VERSION_MAJOR:       KeyField = KeyField::new(8, 8 );
    pub const DEVBOOT_DISABLE:     KeyField = KeyField::new(1, 16);
    pub const ANTIROLLBACK_ENA:    KeyField = KeyField::new(1, 17);
    pub const ANTIROLLFORW_ENA:    KeyField = KeyField::new(1, 18);
    pub const FORWARD_REV_LIMIT:   KeyField = KeyField::new(4, 19);
    pub const FORWARD_MINOR_LIMIT: KeyField = KeyField::new(4, 23);
    pub const INITIALIZED:         KeyField = KeyField::new(1, 27);
}

/// helper routine that will reverse the order of bits. uses a divide-and-conquer approach.
pub(crate) fn bitflip(input: &[u8], output: &mut [u8]) {
    assert!((input.len() % 4 == 0) && (output.len() % 4 == 0) && (input.len() == output.len()));
    for (src, dst) in
    input.chunks(4).into_iter()
    .zip(output.chunks_mut(4).into_iter()) {
        let mut word = u32::from_le_bytes(src.try_into().unwrap()); // read in as LE
        word = ((word >> 1) & 0x5555_5555) | ((word & (0x5555_5555)) << 1);
        word = ((word >> 2) & 0x3333_3333) | ((word & (0x3333_3333)) << 2);
        word = ((word >> 4) & 0x0F0F_0F0F) | ((word & (0x0F0F_0F0F)) << 4);
        // copy out as BE, this performs the final byte-level swap needed by the divde-and-conquer algorithm
        for (&s, d) in word.to_be_bytes().iter().zip(dst.iter_mut()) {
            *d = s;
        }
    }
}

pub(crate) struct RootKeys {
    keyrom: utralib::CSR<u32>,
    gateware_mr: xous::MemoryRange,
    gateware_base: u32,
    staging_mr: xous::MemoryRange,
    staging_base: u32,
    loader_code_mr: xous::MemoryRange,
    loader_code_base: u32,
    kernel_mr: xous::MemoryRange,
    kernel_base: u32,
    /// regions of RAM that holds all plaintext passwords, keys, and temp data. stuck in two well-defined page so we can
    /// zero-ize it upon demand, without guessing about stack frames and/or Rust optimizers removing writes
    sensitive_data: RefCell<xous::MemoryRange>, // this gets purged at least on every suspend, but ideally purged sooner than that
    pass_cache: xous::MemoryRange,  // this can be purged based on a policy, as set below
    boot_password_policy: PasswordRetentionPolicy,
    update_password_policy: PasswordRetentionPolicy,
    cur_password_type: Option<PasswordType>, // for tracking which password we're dealing with at the UX layer
    susres: susres::Susres, // for disabling suspend/resume
    trng: trng::Trng,
    gfx: graphics_server::Gfx, // for reading out font planes for signing verification
    spinor: spinor::Spinor,
    ticktimer: ticktimer_server::Ticktimer,
    gam: gam::Gam,
    xns: xous_names::XousNames,
    jtag: jtag::Jtag,
    fake_key: [u8; 32], // a base set of random numbers used to respond to invalid keyloc requests in AES operations
    restore_running: bool,
}

impl<'a> RootKeys {
    pub fn new() -> RootKeys {
        let xns = xous_names::XousNames::new().unwrap();
        let keyrom = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::keyrom::HW_KEYROM_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map keyrom CSR range");
        // read-only memory maps. even if we don't refer to them, we map them into our process
        // so that no other processes can claim them
        let gateware = xous::syscall::map_memory(
            Some(NonZeroUsize::new((xous::SOC_MAIN_GW_LOC + xous::FLASH_PHYS_BASE) as usize).unwrap()),
            None,
            xous::SOC_MAIN_GW_LEN as usize,
            xous::MemoryFlags::R,
        ).expect("couldn't map in the SoC gateware region");
        let staging = xous::syscall::map_memory(
            Some(NonZeroUsize::new((xous::SOC_STAGING_GW_LOC + xous::FLASH_PHYS_BASE) as usize).unwrap()),
            None,
            xous::SOC_STAGING_GW_LEN as usize,
            xous::MemoryFlags::R,
        ).expect("couldn't map in the SoC staging region");
        let loader_code = xous::syscall::map_memory(
            Some(NonZeroUsize::new((xous::LOADER_LOC + xous::FLASH_PHYS_BASE) as usize).unwrap()),
            None,
            xous::LOADER_CODE_LEN as usize,
            xous::MemoryFlags::R,
        ).expect("couldn't map in the loader code region");
        let kernel = xous::syscall::map_memory(
            Some(NonZeroUsize::new((xous::KERNEL_LOC + xous::FLASH_PHYS_BASE) as usize).unwrap()),
            None,
            xous::KERNEL_LEN as usize,
            xous::MemoryFlags::R,
        ).expect("couldn't map in the kernel region");

        let mut sensitive_data = xous::syscall::map_memory(
            None,
            None,
            0x1000,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        ).expect("couldn't map sensitive data page");
        let mut pass_cache = xous::syscall::map_memory(
            None,
            None,
            0x1000,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        ).expect("couldn't map sensitive data page");
        // make sure the caches start out as zeros
        for w in pass_cache.as_slice_mut::<u32>().iter_mut() {
            *w = 0;
        }
        for w in sensitive_data.as_slice_mut::<u32>().iter_mut() {
            *w = 0;
        }

        let spinor = spinor::Spinor::new(&xns).expect("couldn't connect to spinor server");
        spinor.register_soc_token().expect("couldn't register rootkeys as the one authorized writer to the gateware update area!");
        let jtag = jtag::Jtag::new(&xns).expect("couldn't connect to JTAG server");
        let trng = trng::Trng::new(&xns).expect("couldn't connect to TRNG server");
        // respond to invalid key indices with a "fake" AES key. We try to foil attempts to "probe out" the
        // oracle to discover the presence of null keys.
        let mut fake_key: [u8; 32] = [0; 32];
        for k in fake_key.chunks_exact_mut(8) {
            k.clone_from_slice(&trng.get_u64().unwrap().to_be_bytes());
        }

        let keys = RootKeys {
            keyrom: CSR::new(keyrom.as_mut_ptr() as *mut u32),
            gateware_mr: gateware,
            gateware_base: xous::SOC_MAIN_GW_LOC,
            staging_mr: staging,
            staging_base: xous::SOC_STAGING_GW_LOC,
            loader_code_mr: loader_code,
            loader_code_base: xous::LOADER_LOC,
            kernel_mr: kernel,
            kernel_base: xous::KERNEL_LOC,
            sensitive_data: RefCell::new(sensitive_data),
            pass_cache,
            update_password_policy: PasswordRetentionPolicy::AlwaysPurge,
            boot_password_policy: PasswordRetentionPolicy::AlwaysKeep,
            cur_password_type: None,
            susres: susres::Susres::new_without_hook(&xns).expect("couldn't connect to susres without hook"),
            trng,
            gfx: graphics_server::Gfx::new(&xns).expect("couldn't connect to gfx"),
            spinor,
            ticktimer: ticktimer_server::Ticktimer::new().expect("couldn't connect to ticktimer"),
            gam: gam::Gam::new(&xns).expect("couldn't connect to GAM"),
            xns,
            jtag,
            fake_key,
            restore_running: false,
        };
        /*
        // dumps the key enclave -- in a format for Renode integration. Or if you just wanted to steal all the keys.
        // PS: you still need to know the passwords to decrypt the keys
        for i in 0..256 {
            keys.keyrom.wfo(utra::keyrom::ADDRESS_ADDRESS, i);
            log::info!("this.data[0x{:x}] = 0x{:x};", i, keys.keyrom.rf(utra::keyrom::DATA_DATA));
        } */

        keys
    }
    pub fn gateware(&self) -> &[u8] {
        self.gateware_mr.as_slice::<u8>()
    }
    pub fn gateware_base(&self) -> u32 { self.gateware_base }
    pub fn staging(&self) -> &[u8] {
        self.staging_mr.as_slice::<u8>()
    }
    pub fn staging_base(&self) -> u32 { self.staging_base }
    pub fn loader_code(&self) -> &[u8] {
        self.loader_code_mr.as_slice::<u8>()
    }
    pub fn loader_base(&self) -> u32 { self.loader_code_base }
    pub fn kernel(&self) -> &[u8] {
        self.kernel_mr.as_slice::<u8>()
    }
    pub fn kernel_base(&self) -> u32 { self.kernel_base }

    /// takes a root key and computes the current rollback state of the key by hashing it
    /// MAX_ROLLBACK_LIMIT - GLOBAL_ROLLBACK times.
    fn compute_key_rollback(&mut self, key: &mut [u8]) {
        assert!(key.len() == 32, "Key length is incorrect");
        self.keyrom.wfo(utra::keyrom::ADDRESS_ADDRESS, KeyRomLocs::GLOBAL_ROLLBACK as u32);
        let mut rollback_limit = self.keyrom.rf(utra::keyrom::DATA_DATA);
        if rollback_limit > 255 { rollback_limit = 255; } // prevent increment-up attacks that roll over
        log::debug!("rollback_limit: {}", rollback_limit);
        for _i in 0..MAX_ROLLBACK_LIMIT - rollback_limit as u8 {
            let mut hasher = Sha512Trunc256::new_with_strategy(FallbackStrategy::SoftwareOnly);
            hasher.update(&key);
            let digest = hasher.finalize();
            assert!(digest.len() == 32, "Digest had an incorrect length");
            key.copy_from_slice(&digest);
            #[cfg(feature = "hazardous-debug")]
            if _i >= (MAX_ROLLBACK_LIMIT - 3) {
                log::info!("iter {} key {:x?}", _i, key);
            }
        }
    }
    /// This implementation creates and destroys the AES key schedule on every function call
    /// However, Rootkey operations are not meant to be used for streaming operations; they are typically
    /// used to secure subkeys, so a bit of overhead on each call is OK in order to not keep excess secret
    /// data laying around.
    /// ASSUME: the caller has confirmed that the user password is valid and in cache
    pub fn aes_op(&mut self, key_index: u8, op_type: AesOpType, block: &mut [u8; 16]) {
        let mut key = match key_index {
            KeyRomLocs::USER_KEY => {
                let mut key_enc = self.read_key_256(KeyRomLocs::USER_KEY);
                let pcache: &PasswordCache = unsafe{& *(self.pass_cache.as_ptr() as *const PasswordCache)};
                if pcache.hashed_boot_pw_valid == 0 {
                    self.purge_password(PasswordType::Boot);
                    log::warn!("boot password isn't valid! Returning bogus results.");
                }
                for (key, &pw) in
                key_enc.iter_mut().zip(pcache.hashed_boot_pw.iter()) {
                    *key = *key ^ pw;
                }
                if self.boot_password_policy == PasswordRetentionPolicy::AlwaysPurge {
                    self.purge_password(PasswordType::Boot);
                }
                key_enc
            },
            _ => {
                // within a single boot, return a stable, non-changing fake key based off of a single root
                // fake key. This will make it a bit harder for an attacker to "probe out" an oracle and see
                // which keys are null or which are populated.
                self.fake_key[0] = key_index;
                self.fake_key
            }
        };
        self.compute_key_rollback(&mut key);
        let cipher = Aes256::new(GenericArray::from_slice(&key));
        match op_type {
            AesOpType::Decrypt => cipher.decrypt_block(block.try_into().unwrap()),
            AesOpType::Encrypt => cipher.encrypt_block(block.try_into().unwrap())
        }
    }
    pub fn aes_par_op(&mut self, key_index: u8, op_type: AesOpType, blocks: &mut[[u8; 16]; PAR_BLOCKS]) {
        let mut key = match key_index {
            KeyRomLocs::USER_KEY => {
                let mut key_enc = self.read_key_256(KeyRomLocs::USER_KEY);
                let pcache: &PasswordCache = unsafe{& *(self.pass_cache.as_ptr() as *const PasswordCache)};
                if pcache.hashed_boot_pw_valid == 0 {
                    self.purge_password(PasswordType::Boot);
                    log::warn!("boot password isn't valid! Returning bogus results.");
                }
                for (key, &pw) in
                key_enc.iter_mut().zip(pcache.hashed_boot_pw.iter()) {
                    *key = *key ^ pw;
                }
                if self.boot_password_policy == PasswordRetentionPolicy::AlwaysPurge {
                    self.purge_password(PasswordType::Boot);
                }
                key_enc
            },
            _ => {
                self.fake_key[0] = key_index;
                self.fake_key
            }
        };
        self.compute_key_rollback(&mut key);
        let cipher = Aes256::new(GenericArray::from_slice(&key));
        match op_type {
            AesOpType::Decrypt => {
                for block in blocks.iter_mut() {
                    cipher.decrypt_block(block.try_into().unwrap());
                }
            },
            AesOpType::Encrypt => {
                for block in blocks.iter_mut() {
                    cipher.encrypt_block(block.try_into().unwrap());
                }
            }
        }
    }
    pub fn kwp_op(&mut self, kwp: &mut KeyWrapper) {
        let mut key = match kwp.key_index {
            KeyRomLocs::USER_KEY => {
                let mut key_enc = self.read_key_256(KeyRomLocs::USER_KEY);
                let pcache: &PasswordCache = unsafe{& *(self.pass_cache.as_ptr() as *const PasswordCache)};
                if pcache.hashed_boot_pw_valid == 0 {
                    self.purge_password(PasswordType::Boot);
                    log::warn!("boot password isn't valid! Returning bogus results.");
                }
                for (key, &pw) in
                key_enc.iter_mut().zip(pcache.hashed_boot_pw.iter()) {
                    *key = *key ^ pw;
                }
                if self.boot_password_policy == PasswordRetentionPolicy::AlwaysPurge {
                    self.purge_password(PasswordType::Boot);
                }
                key_enc
            },
            _ => {
                self.fake_key[0] = kwp.key_index;
                self.fake_key
            }
        };
        #[cfg(feature = "hazardous-debug")]
        log::debug!("root user key: {:x?}", key);
        self.compute_key_rollback(&mut key);
        #[cfg(feature = "hazardous-debug")]
        log::debug!("root user key (anti-rollback): {:x?}", key);
        use aes_kw::Kek;
        use aes_kw::KekAes256;
        let keywrapper: KekAes256 = Kek::from(key);
        match kwp.op {
            KeyWrapOp::Wrap => {
                match keywrapper.wrap_with_padding_vec(&kwp.data[..kwp.len as usize]) {
                    Ok(wrapped) => {
                        for (&src, dst) in wrapped.iter().zip(kwp.data.iter_mut()) {
                            *dst = src;
                        }
                        kwp.len = wrapped.len() as u32;
                        kwp.result = None;
                        // this is an un-used field but...why not?
                        kwp.expected_len = wrapped.len() as u32;
                    }
                    Err(e) => {
                        kwp.result = Some(match e {
                            aes_kw::Error::IntegrityCheckFailed => KeywrapError::IntegrityCheckFailed,
                            aes_kw::Error::InvalidDataSize => KeywrapError::InvalidDataSize,
                            aes_kw::Error::InvalidKekSize { size } => {
                                log::info!("invalid size {}", size); // weird. can't name this _size
                                KeywrapError::InvalidKekSize
                            },
                            aes_kw::Error::InvalidOutputSize { expected } => {
                                log::info!("invalid output size {}", expected);
                                KeywrapError::InvalidOutputSize
                            },
                        });
                    }
                }
            }
            KeyWrapOp::Unwrap => {
                match keywrapper.unwrap_with_padding_vec(&kwp.data[..kwp.len as usize]) {
                    Ok(wrapped) => {
                        for (&src, dst) in wrapped.iter().zip(kwp.data.iter_mut()) {
                            *dst = src;
                        }
                        kwp.len = wrapped.len() as u32;
                        kwp.result = None;
                        // this is an un-used field but...why not?
                        kwp.expected_len = wrapped.len() as u32;
                    }
                    Err(e) => {
                        kwp.result = Some(match e {
                            aes_kw::Error::IntegrityCheckFailed => {
                                // try the legacy version, if it unwraps, send back the key + an error that indicates the caller needs to update their version
                                let legacy_kw = Aes256KeyWrap::new(&key);
                                match legacy_kw.decapsulate(&kwp.data[..kwp.len as usize], kwp.expected_len as usize) {
                                    Ok(unwrapped) => {
                                        for (&src, dst) in unwrapped.iter().zip(kwp.data.iter_mut()) {
                                            *dst = src;
                                        }
                                        kwp.len = unwrapped.len() as u32;
                                        // hand the caller back a new version of the wrapped key that meets NIST specs.
                                        let corrected = keywrapper.wrap_with_padding_vec(&unwrapped).expect("couldn't convert to correct version of AES keywrapping");
                                        let mut upgrade = [0u8; 40];
                                        assert!(corrected.len() == 40, "Correctly wrapped key has a different length than the legacy wrapped key");
                                        upgrade.copy_from_slice(&corrected);

                                        let mut unwrapped_key = [0u8; 32];
                                        assert!(kwp.len == 32, "Unwrapped key from legacy version of algorithm has the wrong length");
                                        unwrapped_key.copy_from_slice(&unwrapped);
                                        log::warn!("Keywrap from incorrect version of algorithm; sending message to correct the problem");
                                        KeywrapError::UpgradeToNew((unwrapped_key, upgrade))
                                    }
                                    Err(e) => {
                                        e
                                    }
                                }
                            },
                            aes_kw::Error::InvalidDataSize => KeywrapError::InvalidDataSize,
                            aes_kw::Error::InvalidKekSize { size } => {
                                log::info!("invalid size {}", size); // weird. can't name this _size
                                KeywrapError::InvalidKekSize
                            },
                            aes_kw::Error::InvalidOutputSize { expected } => {
                                log::info!("invalid output size {}", expected);
                                KeywrapError::InvalidOutputSize
                            },
                        });
                    }
                }
            }
        }
    }

    /// returns None if there is an obvious problem with the JTAG interface
    /// otherwise returns the result. "secured" would be the most paranoid setting
    /// which is all the bits burned. There are other combinations that are also
    /// totally valid based on your usage scenario, however, but the have yet to
    /// be implemented (see the JTAG crate for more info); however, we reflect
    /// all of the calls through rootkeys so we aren't exposing JTAG attack surface
    /// to the rest of the world.
    pub fn is_efuse_secured(&self) -> Option<bool> {
        if self.jtag.get_id().unwrap() != jtag::XCS750_IDCODE {
            return None;
        }
        if (self.jtag.get_raw_control_bits().expect("couldn't get control bits") & 0x3f) != 0x3F {
            return Some(false)
        } else {
            return Some(true)
        }
    }
    pub fn fpga_key_source(&self) -> FpgaKeySource {
        let mut words = self.gateware()[..4096].chunks(4);
        loop {
            if let Some(word) = words.next() {
                let cwd = u32::from_be_bytes(word[0..4].try_into().unwrap());
                if cwd == BITSTREAM_CTL0_CMD {
                    let ctl0 = u32::from_be_bytes(words.next().unwrap()[0..4].try_into().unwrap());
                    if ctl0 & 0x8000_0000 == 0 {
                        return FpgaKeySource::Bbram
                    } else {
                        return FpgaKeySource::Efuse
                    }
                }
            } else {
                log::error!("didn't find FpgaKeySource in plaintext header");
                panic!("didn't find FpgaKeySource in plaintext header");
            }
        }
    }
    pub fn is_zero_key(&self) -> Option<bool> {
        if let Some(secured) = self.is_efuse_secured() {
            if !secured {
                if self.fpga_key_source() == FpgaKeySource::Efuse {
                    match self.jtag.efuse_fetch() {
                        Ok(record) => {
                            if record.key == [0u8; 32] {
                                Some(true) // yep, we booted from this and it's 0.
                            } else {
                                log::warn!("Efuse key was set, and we're booting from it, but the readback protection was NOT enabled. The key is not secured.");
                                Some(false) // we booted from this, and we can definitively say it's not 0 (but also, we could read it out!!!)
                            }
                        }
                        _ => None // error fetching key. can't say anything
                    }
                } else {
                    None // booting from BBRAM. maybe it's zero, but we can't read BBRAM keys.
                }
            } else {
                // this is borderline. Someone bothered to burn the readback protection fuses.
                // we can't prove it's not zero, but for purposes of updates and provisioning, we should treat it as non-zero.
                Some(false)
            }
        } else {
            None // couldn't read anything, so we can't be sure
        }
    }
    pub fn is_jtag_working(&self) -> bool {
        if self.jtag.get_id().unwrap() == jtag::XCS750_IDCODE {
            true
        } else {
            false
        }
    }

    /// Checks that various registries are "fully populated", to ensure that the trusted set of servers
    /// have completely loaded before trying to move on. Many of the security properties of the system
    /// rely upon a trusted set of servers claiming unique and/or enumerated tokens or slots, and then
    /// disallowing any new registrations after that point. This call prevents trusted operations from
    /// occurring if some of these servers have failed to check in.
    fn xous_init_interlock(&self) {
        loop {
            if self.xns.trusted_init_done().expect("couldn't query init done status on xous-names") {
                break;
            } else {
                log::warn!("trusted init of xous-names not finished, rootkeys is holding off on sensitive operations");
                self.ticktimer.sleep_ms(650).expect("couldn't sleep");
            }
        }
        loop {
            if self.gam.trusted_init_done().expect("couldn't query init done status on GAM") {
                break;
            } else {
                log::warn!("trusted init of GAM not finished, rootkeys is holding off on sensitive operations");
                self.ticktimer.sleep_ms(650).expect("couldn't sleep");
            }
        }
    }
    pub fn purge_user_password(&mut self, pw_type: AesRootkeyType) {
        match pw_type {
            AesRootkeyType::User0 => self.purge_password(PasswordType::Boot),
            _ => log::warn!("Requested to purge a password for a key that we don't have. Ignoring."),
        }
    }
    pub fn purge_password(&mut self, pw_type: PasswordType) {
        unsafe {
            let pcache_ptr: *mut PasswordCache = self.pass_cache.as_mut_ptr() as *mut PasswordCache;
            match pw_type {
                PasswordType::Boot => {
                    for p in (*pcache_ptr).hashed_boot_pw.iter_mut() {
                        *p = 0;
                    }
                    (*pcache_ptr).hashed_boot_pw_valid = 0;
                }
                PasswordType::Update => {
                    for p in (*pcache_ptr).hashed_update_pw.iter_mut() {
                        *p = 0;
                    }
                    (*pcache_ptr).hashed_update_pw_valid = 0;

                    for p in (*pcache_ptr).fpga_key.iter_mut() {
                        *p = 0;
                    }
                    (*pcache_ptr).fpga_key_valid = 0;
                }
            }
        }
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
    pub fn purge_sensitive_data(&mut self) {
        for d in self.sensitive_data.borrow_mut().as_slice_mut::<u32>().iter_mut() {
            *d = 0;
        }
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
    fn populate_sensitive_data(&mut self) {
        for (addr, d) in self.sensitive_data.borrow_mut().as_slice_mut::<u32>().iter_mut().enumerate() {
            if addr > 255 {
                break;
            }
            self.keyrom.wfo(utra::keyrom::ADDRESS_ADDRESS, addr as u32);
            let keyword = self.keyrom.rf(utra::keyrom::DATA_DATA);
            *d = keyword;
        }
    }
    fn replace_fpga_key(&mut self) {
        let pcache: &mut PasswordCache = unsafe{&mut *(self.pass_cache.as_mut_ptr() as *mut PasswordCache)};
        if self.is_initialized() {
            assert!(pcache.hashed_update_pw_valid != 0, "update password was not set before calling replace_fpga_key");
        }
        // build a new key into the pcache
        for dst_lw in pcache.fpga_key.chunks_mut(8).into_iter() { // 64 bit chunks
            for (dst, &src) in dst_lw.iter_mut().zip(self.trng.get_u64().unwrap().to_be_bytes().iter()) {
                *dst = src;
            }
        }

        // now encrypt the key, and store it into the sensitive_data for access later on by the patching routine
        if self.is_initialized() {
            for (dst, (key, pw)) in
            self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[KeyRomLocs::FPGA_KEY as usize .. KeyRomLocs::FPGA_KEY as usize + 8].iter_mut().zip(
            pcache.fpga_key.chunks(4).into_iter().zip(pcache.hashed_update_pw.chunks(4).into_iter())) {
                *dst = u32::from_be_bytes(key[0..4].try_into().unwrap()) ^ u32::from_be_bytes(pw[0..4].try_into().unwrap());
            }
        } else {
            for (dst, key) in
            self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[KeyRomLocs::FPGA_KEY as usize .. KeyRomLocs::FPGA_KEY as usize + 8].iter_mut().zip(
            pcache.fpga_key.chunks(4).into_iter()) {
                *dst = u32::from_be_bytes(key[0..4].try_into().unwrap());
            }
        }
    }

    pub fn suspend(&mut self) {
        match self.boot_password_policy {
            PasswordRetentionPolicy::AlwaysKeep => {
                ()
            },
            _ => {
                self.purge_password(PasswordType::Boot);
            }
        }
        match self.update_password_policy {
            PasswordRetentionPolicy::AlwaysKeep => {
                ()
            },
            _ => {
                self.purge_password(PasswordType::Update);
            }
        }
        self.purge_sensitive_data();
    }
    pub fn resume(&mut self) {
    }

    pub fn update_policy(&mut self, policy: Option<PasswordRetentionPolicy>) {
        let pw_type = if let Some(cur_type) = self.cur_password_type {
            cur_type
        } else {
            log::error!("got an unexpected policy update from the UX");
            return;
        };
        if let Some(p) = policy {
            match pw_type {
                PasswordType::Boot => self.boot_password_policy = p,
                PasswordType::Update => self.update_password_policy = p,
            };
        } else {
            match pw_type {
                PasswordType::Boot => PasswordRetentionPolicy::AlwaysPurge,
                PasswordType::Update => PasswordRetentionPolicy::AlwaysPurge,
            };
        }
        // once the policy has been set, revert the current type to None
        self.cur_password_type = None;
    }

    /// Plaintext password is passed as a &str. Any copies internally are destroyed. Caller is responsible for destroying the &str original.
    /// Performs a bcrypt hash of the password, with the currently set salt; does not store the plaintext after exit.
    pub fn hash_and_save_password(&mut self, pw: &str, verify: bool) -> bool {
        let pw_type = if let Some(cur_type) = self.cur_password_type {
            cur_type
        } else {
            log::error!("got an unexpected password from the UX");
            return false;
        };
        let mut hashed_password: [u8; 24] = [0; 24];
        let mut salt = self.get_salt();
        // we change the salt ever-so-slightly for every password. This doesn't make any one password more secure;
        // but it disallows guessing all the passwords with a single off-the-shelf hashcat run.
        salt[0] ^= pw_type as u8;

        let timer = ticktimer_server::Ticktimer::new().expect("couldn't connect to ticktimer");
        // the bcrypt function takes the plaintext password and makes one copy to prime the blowfish bcrypt
        // cipher. It is responsible for erasing this state.
        let start_time = timer.elapsed_ms();
        bcrypt(BCRYPT_COST, &salt, pw, &mut hashed_password); // note: this internally makes a copy of the password, and destroys it
        let elapsed = timer.elapsed_ms() - start_time;
        log::info!("bcrypt cost: {} time: {}ms", BCRYPT_COST, elapsed); // benchmark to figure out how to set cost parameter

        // expand the 24-byte (192-bit) bcrypt result into 256 bits, so we can use it directly as XOR key material
        // against 256-bit AES and curve25519 keys
        // for such a small hash, software is the most performant choice
        let mut hasher = Sha512Trunc256::new_with_strategy(FallbackStrategy::SoftwareOnly);
        hasher.update(hashed_password);
        let digest = hasher.finalize();

        let pcache_ptr: *mut PasswordCache = self.pass_cache.as_mut_ptr() as *mut PasswordCache;
        if !verify {
            unsafe {
                match pw_type {
                    PasswordType::Boot => {
                        for (&src, dst) in digest.iter().zip((*pcache_ptr).hashed_boot_pw.iter_mut()) {
                            *dst = src;
                        }
                        (*pcache_ptr).hashed_boot_pw_valid = 1;
                    }
                    PasswordType::Update => {
                        for (&src, dst) in digest.iter().zip((*pcache_ptr).hashed_update_pw.iter_mut()) {
                            *dst = src;
                        }
                        (*pcache_ptr).hashed_update_pw_valid = 1;
                    }
                }
            }
            true
        } else {
            unsafe {
                match pw_type {
                    PasswordType::Boot => {
                        if (*pcache_ptr).hashed_boot_pw_valid == 1 {
                            for (&src, &dst) in digest.iter().zip((*pcache_ptr).hashed_boot_pw.iter()) {
                                if dst != src {
                                    return false
                                }
                            }
                            true
                        } else {
                            false
                        }
                    }
                    PasswordType::Update => {
                        if (*pcache_ptr).hashed_update_pw_valid == 1 {
                            for (&src, &dst) in digest.iter().zip((*pcache_ptr).hashed_update_pw.iter()) {
                                if dst != src {
                                    return false
                                }
                            }
                            true
                        } else {
                            false
                        }
                    }
                }
            }
        }
    }

    /// Reads a 256-bit key at a given index offset
    fn read_key_256(&mut self, index: u8) -> [u8; 32] {
        let mut key: [u8; 32] = [0; 32];
        for (addr, word) in key.chunks_mut(4).into_iter().enumerate() {
            self.keyrom.wfo(utra::keyrom::ADDRESS_ADDRESS, index as u32 + addr as u32);
            let keyword = self.keyrom.rf(utra::keyrom::DATA_DATA);
            for (&byte, dst) in keyword.to_be_bytes().iter().zip(word.iter_mut()) {
                *dst = byte;
            }
        }
        key
    }
    /// Reads a 128-bit key at a given index offset
    fn read_key_128(&mut self, index: u8) -> [u8; 16] {
        let mut key: [u8; 16] = [0; 16];
        for (addr, word) in key.chunks_mut(4).into_iter().enumerate() {
            self.keyrom.wfo(utra::keyrom::ADDRESS_ADDRESS, index as u32 + addr as u32);
            let keyword = self.keyrom.rf(utra::keyrom::DATA_DATA);
            for (&byte, dst) in keyword.to_be_bytes().iter().zip(word.iter_mut()) {
                *dst = byte;
            }
        }
        key
    }
    /// Reads a 256-bit key at a given index offset
    fn read_staged_key_256(&mut self, index: u8) -> [u8; 32] {
        let mut key: [u8; 32] = [0; 32];
        for (addr, word) in key.chunks_mut(4).into_iter().enumerate() {
            let keyword = self.sensitive_data.borrow().as_slice::<u32>()[index as usize + addr];
            for (&byte, dst) in keyword.to_be_bytes().iter().zip(word.iter_mut()) {
                *dst = byte;
            }
        }
        key
    }

    /// Returns the `salt` needed for the `bcrypt` routine.
    /// This routine handles the special-case of being uninitialized: in that case, we need to get
    /// salt from a staging area, and not our KEYROM. However, `setup_key_init` must be called
    /// first to ensure that the staging area has a valid salt.
    fn get_salt(&mut self) -> [u8; 16] {
        if !self.is_initialized() || self.restore_running {
            // we're not initialized, use the salt that should already be in the staging area
            let mut key: [u8; 16] = [0; 16];
            for (word, &keyword) in key.chunks_mut(4).into_iter()
            .zip(self.sensitive_data.borrow_mut().as_slice::<u32>() // get the sensitive_data as a slice &mut[u32]
            [KeyRomLocs::PEPPER as usize..KeyRomLocs::PEPPER as usize + 128/(size_of::<u32>()*8)].iter()) {
                for (&byte, dst) in keyword.to_be_bytes().iter().zip(word.iter_mut()) {
                    *dst = byte;
                }
            }
            key
        } else {
            self.read_key_128(KeyRomLocs::PEPPER)
        }
    }

    /// Called by the UX layer to track which password we're currently requesting
    pub fn set_ux_password_type(&mut self, cur_type: Option<PasswordType>) {
        self.cur_password_type = cur_type;
    }
    /// Called by the UX layer to check which password request is in progress
    #[allow(dead_code)]
    pub fn get_ux_password_type(&self) -> Option<PasswordType> {self.cur_password_type}

    pub fn is_initialized(&mut self) -> bool {
        self.keyrom.wfo(utra::keyrom::ADDRESS_ADDRESS, KeyRomLocs::CONFIG as u32);
        let config = self.keyrom.rf(utra::keyrom::DATA_DATA);
        if config & keyrom_config::INITIALIZED.ms(1) != 0 {
            true
        } else {
            false
        }
    }

    pub fn is_pcache_update_password_valid(&self) -> bool {
        let pcache: &mut PasswordCache = unsafe{&mut *(self.pass_cache.as_mut_ptr() as *mut PasswordCache)};
        if pcache.hashed_update_pw_valid == 0 {
            false
        } else {
            true
        }
    }
    pub fn is_pcache_boot_password_valid(&self) -> bool {
        let pcache: &mut PasswordCache = unsafe{&mut *(self.pass_cache.as_mut_ptr() as *mut PasswordCache)};
        if pcache.hashed_boot_pw_valid == 0 {
            false
        } else {
            true
        }
    }

    /// Called by the UX layer to set up a key init run. It disables suspend/resume for the duration
    /// of the run, and also sets up some missing fields of KEYROM necessary to encrypt passwords.
    pub fn setup_key_init(&mut self) {
        self.xous_init_interlock();
        // block suspend/resume ops during security-sensitive operations
        self.susres.set_suspendable(false).expect("couldn't block suspend/resume");
        // in this block, keyrom data is copied into RAM.
        // make a copy of the KEYROM to hold the new mods, in the sensitive data area
        for addr in 0..256 {
            self.keyrom.wfo(utra::keyrom::ADDRESS_ADDRESS, addr);
            self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[addr as usize] = self.keyrom.rf(utra::keyrom::DATA_DATA);
        }

        // provision the pepper
        for keyword in self.sensitive_data.borrow_mut().as_slice_mut::<u32>()
        [KeyRomLocs::PEPPER as usize..KeyRomLocs::PEPPER as usize + 128/(size_of::<u32>()*8)].iter_mut() {
            *keyword = self.trng.get_u32().expect("couldn't get random number");
        }
    }
    pub fn setup_restore_init(&mut self, key: backups::BackupKey, rom: backups::KeyRomExport) {
        self.xous_init_interlock();
        // block suspend/resume ops during security-sensitive operations
        self.susres.set_suspendable(false).expect("couldn't block suspend/resume");

        // populate the staging area, in particular we are interested in the "pepper" so passwords work correctly.
        self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[..256]
        .copy_from_slice(&rom.0);

        let pcache: &mut PasswordCache = unsafe{&mut *(self.pass_cache.as_mut_ptr() as *mut PasswordCache)};
        // copy the plaintext FPGA key to the pcache
        pcache.fpga_key.copy_from_slice(&key.0);
        pcache.fpga_key_valid = 1;

        // stage the plaintext FPGA key into the keyrom area for encryption by the key_init routine.
        self.sensitive_data.borrow_mut().as_slice_mut::<u8>()[KeyRomLocs::FPGA_KEY as usize..KeyRomLocs::FPGA_KEY as usize + 32]
            .copy_from_slice(&key.0);

        self.restore_running = true;
    }

    /// used to recycle a PDDB after a key init event
    pub fn pddb_recycle(&mut self) {
        // erase the page table, which should effectively trigger a reformat on the next boot
        self.spinor.bulk_erase(xous::PDDB_LOC, 512 * 1024).expect("couldn't erase page table");
    }
    /// Core of the key initialization routine. Requires a `progress_modal` dialog box that has been set
    /// up with the appropriate notification messages by the UX layer, and a `Slider` type action which
    /// is used to report the progress of the initialization routine. We assume the `Slider` box is set
    /// up to report progress on a range of 0-100%.
    ///
    /// IMPORTANT ASSUMPTION: It is assumed that all the progress messages in the translations do not
    /// end up changing the size of the dialog box. This is because such changes in size would trigger
    /// a redraw message that cannot be handled by the thread (because it's busy doing this), and
    /// if enough of them get issued, eventually the thread deadlocks. If you are doing translations,
    /// review your messages and add \n characters on shorter messages so that the overall height
    /// of the dialog box remains constant throughout the operation!
    ///
    /// This routine dispatches the following activities:
    /// - generate signing private key (encrypted with update password)
    /// - generate rootkey (encrypted with boot password)
    /// - generate signing public key
    /// - set the init bit
    /// - sign the loader
    /// - sign the kernel
    /// - compute the patch set for the FPGA bitstream
    /// - do the patch (whatever that means - gotta deal with the AES key, HMAC etc.)
    /// - verify the FPGA image hmac
    /// - sign the FPGA image
    /// - get ready for a reboot
    ///
    /// Note to future self: this terrible syntax `self.sensitive_data.borrow_mut().as_slice_mut::<u32>()`
    /// is embedded all over the place as a substitute for `sensitive_slice` because we need interior mutability
    /// of the data within the MemoryRange (which is not mutable) to change the contents of the MemoryRange
    /// (a mutable operation). We can't bind `sensitive_slice` to `self.sensitive_data.borrow_mut().as_slice_mut::<u32>()`
    /// because this creates a temporary that has the wrong lifetime, and thus, we have to embed that terrible piece
    /// of unmaintainable syntax all over the place in the code below to solve this problem.
    ///
    /// If `restore` is `Some`, don't generate keys, but restore from backup. The key provided to this routine
    /// is *always* the correct key for the FPGA to boot from. The entry in the KeyRom will be adjusted accordingly.
    pub fn do_key_init(&mut self,
        rootkeys_modal: &mut Modal,
        main_cid: xous::CID,
    ) -> Result<(), RootkeyResult> {
        self.xous_init_interlock();
        self.spinor.set_staging_write_protect(true).expect("couldn't protect the staging area");

        let mut progress_action = Slider::new(main_cid, Opcode::UxGutter.to_u32().unwrap(),
        0, 100, 10, Some("%"), 0, true, true
        );
        progress_action.set_is_password(true);
        // now show the init wait note...
        rootkeys_modal.modify(
            Some(ActionType::Slider(progress_action)),
            Some(t!("rootkeys.setup_wait", xous::LANG)), false,
            None, true, None);
        rootkeys_modal.activate();

        xous::yield_slice(); // give some time to the GAM to render

        // Capture the progress bar elements in a convenience structure.
        // NOTE: This is documented in the structure itself, but, it's worth repeating here:
        // this routine only works because we don't resize the rootkeys_modal box as we
        // advance progress. This allows us to do a local-only redraw without triggering
        // a global GAM redraw operation. If we were to resize the dialog box, this would
        // trigger the defacing algorithm to send back "redraw" messages into the main
        // loop's queue. However, because the main loop is currently stuck running the code
        // in this routine, the "redraw" messages never get serviced (even if they are
        // effectively NOPs), and eventually, these messages would fill up the queue and can cause
        // the system to deadlock once the queue is full.
        let mut pb = ProgressBar::new(rootkeys_modal, &mut progress_action);

        // kick the progress bar to indicate we've entered the routine
        pb.set_percentage(1);

        // get access to the pcache and generate a keypair
        let pcache: &mut PasswordCache = unsafe{&mut *(self.pass_cache.as_mut_ptr() as *mut PasswordCache)};
        // initialize the global rollback constant to 0
        self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[KeyRomLocs::GLOBAL_ROLLBACK as usize] = 0;
        let mut root_sk = [0u8; ed25519_dalek::SECRET_KEY_LENGTH];
        self.trng.fill_bytes(&mut root_sk);
        #[cfg(feature = "hazardous-debug")]
        log::debug!("root privkey: {:x?}", root_sk);
        let keypair = if true { // true for production, false for debug (uses dev keys, so we can compare results)
            // we use software hashing because for short keys its faster
            let mut derived_sk = [0u8; ed25519_dalek::SECRET_KEY_LENGTH];
            derived_sk.copy_from_slice(&root_sk);
            // hash a derived secret key based on the maximum rollback limit we anticipate less the current global rollback limit (which is 0)
            // this allows us to work with a "final" key that generates a private signing key, which can't be predicted to future versions
            // given the current key (due to the irreversibility of Sha512/256), but once derived to a more recent version can be willfully
            // computed to an older version to recover e.g. old data encrypted with a prior key
            //
            // We don't use the `compute_key_rollback` function because the value of the GLOBAL_ROLLBACK in the KeyROM
            // has not yet been set (it should be zero, but there is no reason for it to be at this point).
            for _i in 0..MAX_ROLLBACK_LIMIT {
                let mut hasher = Sha512Trunc256::new_with_strategy(FallbackStrategy::SoftwareOnly);
                hasher.update(&derived_sk);
                let digest = hasher.finalize();
                derived_sk.copy_from_slice(&digest);
                #[cfg(feature = "hazardous-debug")]
                if _i >= (MAX_ROLLBACK_LIMIT - 3) {
                    log::info!("iter {} key {:x?}", _i, derived_sk);
                }
            }
            let sk: SecretKey = SecretKey::from_bytes(&derived_sk).expect("couldn't construct secret key");
            let pk: PublicKey = (&sk).into();
            // keypair zeroizes on drop
            Keypair{public: pk, secret: sk}
        } else {
            Keypair::from_bytes(
                &[168, 167, 118, 92, 141, 162, 215, 147, 134, 43, 8, 176, 0, 222, 188, 167, 178, 14, 137, 237, 82, 199, 133, 162, 179, 235, 161, 219, 156, 182, 42, 39,
                28, 155, 234, 227, 42, 234, 200, 117, 7, 193, 128, 148, 56, 126, 255, 28, 116, 97, 66, 130, 175, 253, 129, 82, 216, 113, 53, 46, 223, 63, 88, 187
                ]
            ).unwrap()
        };
        log::debug!("keypair pubkey: {:?}", keypair.public.to_bytes());
        log::debug!("keypair pubkey: {:x?}", keypair.public.to_bytes());
        #[cfg(feature = "hazardous-debug")]
        log::debug!("keypair privkey: {:?}", keypair.secret.to_bytes());
        #[cfg(feature = "hazardous-debug")]
        log::debug!("keypair privkey (after anti-rollback): {:x?}", keypair.secret.to_bytes());

        // encrypt the FPGA key using the update password. in an un-init system, it is provided to us in plaintext format
        // e.g. in the case that we're doing a BBRAM boot (eFuse flow would give us a 0's key and we'd later on set it)
        #[cfg(feature = "hazardous-debug")]
        self.debug_print_key(KeyRomLocs::FPGA_KEY as usize, 256, "FPGA key before encryption: ");
        // before we encrypt it, stash a copy in our password cache, as we'll need it later on to encrypt the bitstream
        for (&src, dst) in self.read_key_256(KeyRomLocs::FPGA_KEY).iter()
        .zip(pcache.fpga_key.iter_mut()) {
            *dst = src;
        }
        pcache.fpga_key_valid = 1;

        // now encrypt it in the staging area
        for (word, hashed_pass) in self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[KeyRomLocs::FPGA_KEY as usize..KeyRomLocs::FPGA_KEY as usize + 256/(size_of::<u32>()*8)].iter_mut()
        .zip(pcache.hashed_update_pw.chunks(4).into_iter()) {
            *word = *word ^ u32::from_be_bytes(hashed_pass.try_into().unwrap());
        }

        // allocate a decryption oracle for the FPGA bitstream. This will fail early if the FPGA key is wrong.
        assert!(pcache.fpga_key_valid == 1);
        log::debug!("making destination oracle");
        let mut dst_oracle = match BitstreamOracle::new(&pcache.fpga_key, &pcache.fpga_key, self.gateware(), self.gateware_base()) {
            Ok(o) => o,
            Err(e) => {
                log::error!("couldn't create oracle (most likely FPGA key mismatch): {:?}", e);
                return Err(e);
            }
        };

        pb.set_percentage(5);

        // pub key is easy, no need to encrypt
        let public_key: [u8; ed25519_dalek::PUBLIC_KEY_LENGTH] = keypair.public.to_bytes();
        for (src, dst) in public_key.chunks(4).into_iter()
        .zip(self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[KeyRomLocs::SELFSIGN_PUBKEY as usize..KeyRomLocs::SELFSIGN_PUBKEY as usize + 256/(size_of::<u32>()*8)].iter_mut()) {
            *dst = u32::from_be_bytes(src.try_into().unwrap())
        }
        log::debug!("public key as computed: {:x?}", public_key);

        // extract the update password key from the cache, and apply it to the private key
        #[cfg(feature = "hazardous-debug")]
        {
            log::debug!("cached update password: {:x?}", pcache.hashed_update_pw);
        }
        // private key must XOR with password before storing
        let mut private_key_enc: [u8; ed25519_dalek::SECRET_KEY_LENGTH] = [0; ed25519_dalek::SECRET_KEY_LENGTH];
        // I don't think this loop should make any extra copies of the secret key, but might be good to check in godbolt!
        for (dst, (plain, key)) in
        private_key_enc.iter_mut()
        .zip(root_sk.iter() // we encrypt the root sk, not the derived sk
        .zip(pcache.hashed_update_pw.iter())) {
            *dst = plain ^ key;
        }

        // store the private key to the keyrom staging area
        for (src, dst) in private_key_enc.chunks(4).into_iter()
        .zip(self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[KeyRomLocs::SELFSIGN_PRIVKEY as usize..KeyRomLocs::SELFSIGN_PRIVKEY as usize + 256/(size_of::<u32>()*8)].iter_mut()) {
            *dst = u32::from_be_bytes(src.try_into().unwrap())
        }
        #[cfg(feature = "hazardous-debug")]
        log::debug!("private_key_enc: {:?}", private_key_enc);

        pb.set_percentage(10);

        // generate the "boot key". This is just a random number that eventually gets XOR'd with a PIN
        // by the PDDB to derive the final PDDB .System unlock key.
        let mut boot_key_enc: [u8; 32] = [0; 32];
        for dst in
        boot_key_enc.chunks_mut(4).into_iter() {
            let key_word = self.trng.get_u32().unwrap().to_be_bytes();
            // just unroll this loop, it's fast and easy enough
            (*dst)[0] = key_word[0];
            (*dst)[1] = key_word[1];
            (*dst)[2] = key_word[2];
            (*dst)[3] = key_word[3];
        }
        #[cfg(feature = "hazardous-debug")]
        log::debug!("boot_key_enc: {:?}", boot_key_enc);

        // store the boot key to the keyrom staging area
        for (src, dst) in boot_key_enc.chunks(4).into_iter()
        .zip(self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[KeyRomLocs::USER_KEY as usize..KeyRomLocs::USER_KEY as usize + 256/(size_of::<u32>()*8)].iter_mut()) {
            *dst = u32::from_be_bytes(src.try_into().unwrap())
        }

        // sign the kernel
        pb.update_text(t!("rootkeys.init.signing_kernel", xous::LANG));
        pb.set_percentage(15);
        let (kernel_sig, kernel_len) = self.sign_kernel(&keypair);

        // sign the loader
        pb.update_text(t!("rootkeys.init.signing_loader", xous::LANG));
        pb.rebase_subtask_percentage(20, 30);
        let (loader_sig, loader_len) = self.sign_loader(&keypair, Some(&mut pb));
        log::debug!("loader signature: {:x?}", loader_sig.to_bytes());
        log::debug!("loader len: {} bytes", loader_len);

        // set the "init" bit in the staging area
        self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[KeyRomLocs::CONFIG as usize] |= keyrom_config::INITIALIZED.ms(1);

        #[cfg(feature = "hazardous-debug")]
        {
            log::debug!("Self private key: {:x?}", keypair.secret.to_bytes());
            log::debug!("Self public key: {:x?}", keypair.public.to_bytes());
            self.debug_staging();
        }

        // Because we're initializing keys for the *first* time, make a backup copy of the bitstream to
        // the staging area. Note that if we're doing an update, the update target would already be
        // in the staging area, so this step should be skipped.
        pb.update_text(t!("rootkeys.init.backup_gateware", xous::LANG));
        pb.rebase_subtask_percentage(30, 50);
        self.make_gateware_backup(Some(&mut pb), false)?;

        log::debug!("making source oracle");
        let mut src_oracle = match BitstreamOracle::new(&pcache.fpga_key, &pcache.fpga_key, self.staging(), self.staging_base()) {
            Ok(o) => o,
            Err(e) => {
                log::error!("couldn't create oracle (most likely FPGA key mismatch): {:?}", e);
                dst_oracle.clear();
                return Err(e);
            }
        };

        // compute the keyrom patch set for the bitstream
        // at this point the KEYROM as replicated in sensitive_slice should have all its assets in place
        pb.update_text(t!("rootkeys.init.patching_keys", xous::LANG));
        pb.rebase_subtask_percentage(50, 70);

        self.gateware_copy_and_patch(&src_oracle, &dst_oracle, Some(&mut pb))?;

        // make a copy of the plaintext metadata and csr records
        self.spinor.patch(self.gateware(), self.gateware_base(),
        &self.staging()[METADATA_OFFSET..SELFSIG_OFFSET], METADATA_OFFSET as u32
        ).map_err(|_| RootkeyResult::FlashError)?;

        // verify that the patch worked
        pb.update_text(t!("rootkeys.init.verifying_gateware", xous::LANG));
        pb.rebase_subtask_percentage(70, 90);
        self.verify_gateware(&dst_oracle, Some(&mut pb))?;

        // sign the image, commit the signature
        pb.update_text(t!("rootkeys.init.commit_signatures", xous::LANG));
        self.commit_signature(loader_sig, loader_len, SignatureType::Loader)?;
        log::debug!("loader {} bytes, sig: {:x?}", loader_len, loader_sig.to_bytes());
        pb.set_percentage(92);
        self.commit_signature(kernel_sig, kernel_len, SignatureType::Kernel)?;
        pb.set_percentage(95);

        // clean up the oracles as soon as we're done to avoid some borrow checker issues
        src_oracle.clear();
        dst_oracle.clear();

        // as a sanity check, check the kernel self signature
        let pubkey = PublicKey::from_bytes(&public_key).expect("public key was not valid");
        let ret = if !self.verify_selfsign_kernel(Some(&pubkey)) {
            log::error!("kernel signature failed to verify, probably should not try to reboot!");
            Err(RootkeyResult::IntegrityError)
        } else {
            // sign the gateware
            pb.set_percentage(98);
            let (gateware_sig, gateware_len) = self.sign_gateware(&keypair);
            log::debug!("gateware signature ({}): {:x?}", gateware_len, gateware_sig.to_bytes());
            self.commit_signature(gateware_sig, gateware_len, SignatureType::Gateware)?;
            Ok(())
        };

        // clear the write protects
        self.spinor.set_staging_write_protect(false).expect("couldn't un-protect the staging area");

        // finalize the progress bar on exit -- always leave at 100%
        pb.set_percentage(100);

        self.ticktimer.sleep_ms(500).expect("couldn't show final message");

        // always purge, we're going to reboot; and if we don't, then there's shenanigans
        for w in private_key_enc.iter_mut() { // it's encrypted. but i still want it turned to zeroes.
            *w = 0;
        }
        self.purge_password(PasswordType::Boot);
        self.purge_password(PasswordType::Update);
        self.purge_sensitive_data();
        // ed25519 keypair zeroizes on drop

        ret
    }

    #[allow(dead_code)]
    #[cfg(feature = "hazardous-debug")]
    pub fn printkeys(&mut self) {
        // dump the keystore -- used to confirm that patching worked right. does not get compiled in when hazardous-debug is not enable.
        for addr in 0..256 {
            self.keyrom.wfo(utra::keyrom::ADDRESS_ADDRESS, addr);
            self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[addr as usize] = self.keyrom.rf(utra::keyrom::DATA_DATA);
            log::info!("{:02x}: 0x{:08x}", addr, self.sensitive_data.borrow_mut().as_slice::<u32>()[addr as usize]);
        }
    }

    pub fn do_gateware_update(&mut self,
        rootkeys_modal: &mut Modal,
        modals: &modals::Modals,
        main_cid: xous::CID,
        update_type: UpdateType,
    ) -> Result<(), RootkeyResult> {
        // log::set_max_level(log::LevelFilter::Debug);
        // make sure the system is sane
        self.xous_init_interlock();
        self.spinor.set_staging_write_protect(true).expect("couldn't protect the staging area");

        // setup Ux
        let mut progress_action = Slider::new(main_cid, Opcode::UxGutter.to_u32().unwrap(),
        0, 100, 10, Some("%"), 0, true, true
        );
        progress_action.set_is_password(true);

        let pcache: &mut PasswordCache = unsafe{&mut *(self.pass_cache.as_mut_ptr() as *mut PasswordCache)};
        let mut keypair_bytes: [u8; ed25519_dalek::KEYPAIR_LENGTH] = [0; ed25519_dalek::KEYPAIR_LENGTH];
        let mut old_key = [0u8; 32];
        let mut pb = if update_type == UpdateType::Restore {
            // now show the init wait note...
            rootkeys_modal.modify(
                Some(ActionType::Slider(progress_action)),
                Some(t!("rootkeys.gwup_starting", xous::LANG)), false,
                None, true, None);
            rootkeys_modal.activate();
            xous::yield_slice(); // give some time to the GAM to render
            let mut pb = ProgressBar::new(rootkeys_modal, &mut progress_action);
            pb.set_percentage(1);

            // ASSUME:
            //   - the sensitive_data has been set up correctly
            //   - sensitive_data's FPGA_KEY is a *plaintext* version of the FPGA key
            //   - the pcache.fpga_key also contains a *plaintext* version of the FPGA key
            // Note: these are handled by the `setup_restore_init()` routine.

            //------ test that the restore provided password is valid for the source keyrom block
            // derive signing key
            if pcache.hashed_update_pw_valid == 0 {
                self.purge_password(PasswordType::Update);
                log::error!("no password was set going into the update routine");
                #[cfg(feature = "hazardous-debug")]
                log::debug!("key: {:x?}", pcache.hashed_update_pw);
                log::debug!("valid: {}", pcache.hashed_update_pw_valid);

                return Err(RootkeyResult::KeyError);
            }
            let enc_signing_key = self.read_staged_key_256(KeyRomLocs::SELFSIGN_PRIVKEY);
            for (key, (&enc_key, &pw)) in
            keypair_bytes[..ed25519_dalek::SECRET_KEY_LENGTH].iter_mut()
            .zip(enc_signing_key.iter().zip(pcache.hashed_update_pw.iter())) {
                *key = enc_key ^ pw;
            }
            self.compute_key_rollback(&mut keypair_bytes[..ed25519_dalek::SECRET_KEY_LENGTH]);
            #[cfg(feature = "hazardous-debug")]
            log::debug!("keypair privkey (after anti-rollback): {:x?}", &keypair_bytes[..ed25519_dalek::SECRET_KEY_LENGTH]);
            // read in the public key from the staged data
            for (key, &src) in keypair_bytes[ed25519_dalek::SECRET_KEY_LENGTH..].iter_mut()
            .zip(self.read_staged_key_256(KeyRomLocs::SELFSIGN_PUBKEY).iter()) {
                *key = src;
            }
            #[cfg(feature = "hazardous-debug")]
            log::debug!("keypair_bytes {:x?}", keypair_bytes);
            // Keypair zeroizes the secret key on drop.
            let keypair = Keypair::from_bytes(&keypair_bytes).map_err(|_| RootkeyResult::KeyError)?;
            #[cfg(feature = "hazardous-debug")]
            log::debug!("keypair privkey (after anti-rollback + conversion): {:x?}", keypair.secret.to_bytes());

            // check if the keypair is valid by signing and verifying a short message
            let test_data = "whiskey made me do it";
            let test_sig = keypair.sign(test_data.as_bytes());
            match keypair.verify(&test_data.as_bytes(), &test_sig) {
                Ok(_) => (),
                Err(e) => {
                    log::warn!("update password was not connect ({:?})", e);
                    self.purge_password(PasswordType::Update);
                    for b in keypair_bytes.iter_mut() {
                        *b = 0;
                    }
                    return Err(RootkeyResult::KeyError);
                }
            }

            //------ test that the provided encryption key can actually decrypt the boot image
            // this ensures that we don't brick the FPGA in case something weird happened with a difference
            // between the backup FPGA's keying state, and the destination device's keying state.
            // we do this by creating an oracle that can decrypt the boot gateware using the provided key.
            // if we can create the oracle, it means we were able to decrypt the first little bit of the boot image
            // and we're good to go!
            match BitstreamOracle::new(
                &pcache.fpga_key, &pcache.fpga_key, self.gateware(), self.gateware_base()) {
                Ok(_o) => log::debug!("Provided restore key could also decrypt the boot image."),
                Err(e) => {
                    log::error!("couldn't create oracle (most likely FPGA key mismatch): {:?}", e);
                    self.purge_password(PasswordType::Update);
                    return Err(e);
                }
            };
            old_key.copy_from_slice(&pcache.fpga_key);

            // now encrypt the FPGA key for the Keyrom in-place to the provided password
            for (word, hashed_pass) in self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[KeyRomLocs::FPGA_KEY as usize..KeyRomLocs::FPGA_KEY as usize + 256/(size_of::<u32>()*8)].iter_mut()
            .zip(pcache.hashed_update_pw.chunks(4).into_iter()) {
                *word = *word ^ u32::from_be_bytes(hashed_pass.try_into().unwrap());
            }

            pb.set_percentage(3);
            pb
        } else { // regular and bbram flow
            // don't show the rootkey modals until after the key is computed, so we can pop up
            // the BIP-39 dialog box.

            // decrypt the FPGA key using the stored password
            if pcache.hashed_update_pw_valid == 0 && self.is_initialized() {
                self.purge_password(PasswordType::Update);
                log::error!("no password was set going into the update routine");
                #[cfg(feature = "hazardous-debug")]
                log::debug!("key: {:x?}", pcache.hashed_update_pw);
                log::debug!("valid: {}", pcache.hashed_update_pw_valid);

                return Err(RootkeyResult::KeyError);
            }
            for (&src, dst) in self.read_key_256(KeyRomLocs::FPGA_KEY).iter().zip(pcache.fpga_key.iter_mut()) {
                *dst = src;
            }
            log::debug!("fpga key (encrypted): {:x?}", &pcache.fpga_key);
            for (fkey, &pw) in pcache.fpga_key.iter_mut().zip(pcache.hashed_update_pw.iter()) {
                *fkey = *fkey ^ pw;
            }
            pcache.fpga_key_valid = 1;
            #[cfg(feature = "hazardous-debug")]
            log::debug!("fpga key (reconstituted): {:x?}", &pcache.fpga_key);

            // derive signing key
            let enc_signing_key = self.read_key_256(KeyRomLocs::SELFSIGN_PRIVKEY);
            #[cfg(feature = "hazardous-debug")]
            log::debug!("encrypted root privkey: {:x?}", enc_signing_key);
            for (key, (&enc_key, &pw)) in
            keypair_bytes[..ed25519_dalek::SECRET_KEY_LENGTH].iter_mut()
            .zip(enc_signing_key.iter().zip(pcache.hashed_update_pw.iter())) {
                *key = enc_key ^ pw;
            }
            #[cfg(feature = "hazardous-debug")]
            log::debug!("decrypted root privkey: {:x?}", &keypair_bytes[..ed25519_dalek::SECRET_KEY_LENGTH]);
            // derived_sk now holds the "Root" secret key. It needs to be hashed (MAX_ROLLBACK_LIMIT - GLOBAL_ROLLBACK) times to get the current signing key.
            self.compute_key_rollback(&mut keypair_bytes[..ed25519_dalek::SECRET_KEY_LENGTH]);
            // now populate the public key portion. that's just in the plain.
            // note that this would have been updated in the case of an update to GLOBAL_ROLLBACK -- the purpose of
            // this routine is to sign software in the current rollback count, not to increment the rollback count
            for (key, &src) in keypair_bytes[ed25519_dalek::SECRET_KEY_LENGTH..].iter_mut()
            .zip(self.read_key_256(KeyRomLocs::SELFSIGN_PUBKEY).iter()) {
                *key = src;
            }

            // stage the keyrom data for patching
            self.populate_sensitive_data();
            if update_type == UpdateType::BbramProvision || update_type == UpdateType::EfuseProvision {
                if self.is_initialized() {
                    // make a backup copy of the old key, so we can use it to decrypt the gateware before re-encrypting it
                    old_key.copy_from_slice(&pcache.fpga_key);
                }
                self.replace_fpga_key();

                // we transmit the BBRAM key at this point -- because if there's going to be a failure,
                // we'd rather know it now before moving on. Three copies are sent to provide some
                // check on the integrity of the key.
                if update_type == UpdateType::BbramProvision {
                    log::info!("BBKEY|: {:?}", &pcache.fpga_key);
                    log::info!("BBKEY|: {:?}", &pcache.fpga_key);
                    log::info!("BBKEY|: {:?}", &pcache.fpga_key);
                    log::info!("{}", crate::CONSOLE_SENTINEL);
                } else if update_type == UpdateType::EfuseProvision {
                    // share the backup key with the user so it can be saved somewhere safe
                    modals.show_bip39(Some(t!("rootkeys.backup_key", xous::LANG)), &pcache.fpga_key.to_vec()).ok();
                    loop {
                        modals.add_list_item(t!("rootkeys.gwup.yes", xous::LANG)).expect("modals error");
                        modals.add_list_item(t!("rootkeys.gwup.no", xous::LANG)).expect("modals error");
                        log::info!("{}ROOTKEY.CONFIRM,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                        match modals.get_radiobutton(t!("rootkeys.backup_verify", xous::LANG)) {
                            Ok(response) => {
                                if response == t!("rootkeys.gwup.yes", xous::LANG) {
                                    match modals.input_bip39(Some(t!("rootkeys.backup_key_enter", xous::LANG))) {
                                        Ok(verify) => {
                                            log::debug!("got bip39 verification: {:x?}", verify);
                                            if &verify == &pcache.fpga_key {
                                                log::debug!("verify succeeded");
                                                modals.show_notification(t!("rootkeys.backup_key_match", xous::LANG), None).ok();
                                                break;
                                            } else {
                                                log::debug!("verify failed");
                                                modals.show_bip39(Some(t!("rootkeys.backup_key_mismatch", xous::LANG)), &pcache.fpga_key.to_vec()).ok();
                                            }
                                        }
                                        _ => {
                                            log::debug!("bip39 verification aborted");
                                            modals.show_bip39(Some(t!("rootkeys.backup_key_mismatch", xous::LANG)), &pcache.fpga_key.to_vec()).ok();
                                        }
                                    }
                                } else {
                                    break;
                                }
                            }
                            _ => break,
                        }
                    }
                }
            } else {
                old_key.copy_from_slice(&pcache.fpga_key);
            };

            // *now* show the progress bar...
            rootkeys_modal.modify(
                Some(ActionType::Slider(progress_action)),
                Some(t!("rootkeys.gwup_starting", xous::LANG)), false,
                None, true, None);
            rootkeys_modal.activate();
            xous::yield_slice(); // give some time to the GAM to render
            let mut pb = ProgressBar::new(rootkeys_modal, &mut progress_action);
            pb.set_percentage(3);
            pb
        };
        #[cfg(feature = "hazardous-debug")]
        log::debug!("anti-rollback privkey: {:x?}", &keypair_bytes[..ed25519_dalek::SECRET_KEY_LENGTH]);
        #[cfg(feature = "hazardous-debug")]
        log::debug!("trying to make a keypair from {:x?}", keypair_bytes);
        // Keypair zeroizes on drop
        let keypair: Option<Keypair> = if
        ((update_type == UpdateType::BbramProvision) || (update_type == UpdateType::EfuseProvision))
         && !self.is_initialized() {
            // don't try to derive signing keys if we're doing BBRAM provisioning on an otherwise blank device
            None
        } else {
            Some(Keypair::from_bytes(&keypair_bytes).map_err(|_| RootkeyResult::KeyError)?)
        };
        log::debug!("keypair success");
        #[cfg(feature = "hazardous-debug")]
        if keypair.is_some() {
            log::debug!("keypair privkey (after anti-rollback): {:x?}", keypair.as_ref().unwrap().secret.to_bytes());
        }

        pb.set_percentage(4);
        log::debug!("making destination oracle");
        let mut dst_oracle =
        match BitstreamOracle::new(
            &old_key,
            &pcache.fpga_key,
            self.gateware(),
            self.gateware_base())
        {
            Ok(o) => o,
            Err(e) => {
                self.purge_password(PasswordType::Update);
                log::error!("couldn't create oracle (most likely FPGA key mismatch): {:?}", e);
                return Err(e);
            }
        };

        let mut next_progress = if (update_type == UpdateType::BbramProvision) || (update_type == UpdateType::EfuseProvision) {
            pb.update_text(t!("rootkeys.init.backup_gateware", xous::LANG));
            pb.rebase_subtask_percentage(5, 25);
            self.make_gateware_backup(Some(&mut pb), false)?;
            25
        } else {
            10
        };
        log::debug!("destination oracle success");
        if update_type == UpdateType::BbramProvision {
            dst_oracle.set_target_key_type(FpgaKeySource::Bbram);
        } else if update_type == UpdateType::EfuseProvision {
            dst_oracle.set_target_key_type(FpgaKeySource::Efuse);
        } else {
            let keysource = dst_oracle.get_original_key_type();
            dst_oracle.set_target_key_type(keysource);
        }
        pb.set_percentage(next_progress);
        next_progress += 2;
        // updates are always encrypted with the null key.
        let dummy_key: [u8; 32] = [0; 32];
        log::debug!("making source oracle");
        let mut src_oracle = match BitstreamOracle::new(
            &dummy_key, &pcache.fpga_key,
            self.staging(), self.staging_base())
        {
            Ok(o) => o,
            Err(e) => {
                log::error!("couldn't create oracle (most likely FPGA key mismatch): {:?}", e);
                dst_oracle.clear();
                self.purge_password(PasswordType::Update);
                return Err(e);
            }
        };
        log::debug!("source oracle success");

        log::info!("source key type: {:?}", src_oracle.get_original_key_type());
        log::info!("destination key type: {:?}", dst_oracle.get_target_key_type());

        pb.set_percentage(next_progress);
        pb.update_text(t!("rootkeys.init.patching_keys", xous::LANG));
        pb.rebase_subtask_percentage(next_progress, 60);
        self.gateware_copy_and_patch(&src_oracle, &dst_oracle, Some(&mut pb))?;

        // make a copy of the plaintext metadata and csr records
        self.spinor.patch(self.gateware(), self.gateware_base(),
        &self.staging()[METADATA_OFFSET..SELFSIG_OFFSET], METADATA_OFFSET as u32
        ).map_err(|_| RootkeyResult::FlashError)?;

        // verify that the patch worked
        pb.update_text(t!("rootkeys.init.verifying_gateware", xous::LANG));
        pb.rebase_subtask_percentage(60, 75);
        log::debug!("making verification oracle");
        let verify_oracle = match BitstreamOracle::new(
            &pcache.fpga_key, &pcache.fpga_key, self.gateware(), self.gateware_base()
        ) {
            Ok(o) => o,
            Err(e) => {
                log::error!("couldn't create oracle (most likely FPGA key mismatch): {:?}", e);
                return Err(e);
            }
        };
        self.verify_gateware(&verify_oracle, Some(&mut pb))?;

        pb.set_percentage(76);

        // commit signatures
        let keypair = if let Some(kp) = keypair {
            pb.update_text(t!("rootkeys.init.signing_gateware", xous::LANG));
            let (gateware_sig, gateware_len) = self.sign_gateware(&kp);
            log::debug!("gateware signature ({}): {:x?}", gateware_len, gateware_sig.to_bytes());
            self.commit_signature(gateware_sig, gateware_len, SignatureType::Gateware)?;

            // sign the kernel
            pb.update_text(t!("rootkeys.init.signing_kernel", xous::LANG));
            pb.set_percentage(80);
            let (kernel_sig, kernel_len) = self.sign_kernel(&kp);

            // sign the loader
            pb.update_text(t!("rootkeys.init.signing_loader", xous::LANG));
            pb.rebase_subtask_percentage(85, 92);
            let (loader_sig, loader_len) = self.sign_loader(&kp, Some(&mut pb));

            // commit the signatures
            pb.update_text(t!("rootkeys.init.commit_signatures", xous::LANG));
            self.commit_signature(loader_sig, loader_len, SignatureType::Loader)?;
            log::debug!("loader {} bytes, sig: {:x?}", loader_len, loader_sig.to_bytes());
            pb.set_percentage(88);
            self.commit_signature(kernel_sig, kernel_len, SignatureType::Kernel)?;
            pb.set_percentage(89);

            // pass the kp back into the original variable. keypair does not implement copy...for good reasons.
            Some(kp)
        } else {
            None
        };

        // clean up the oracles
        pb.set_percentage(90);
        src_oracle.clear();
        dst_oracle.clear();
        // make a backup copy of the public key before we purge it. Pubkey is...public, so that's fine!
        let pubkey = match update_type {
            UpdateType::Restore => {
                log::info!("Restore process is verifying using staged public key");
                PublicKey::from_bytes(&self.read_staged_key_256(KeyRomLocs::SELFSIGN_PUBKEY)).expect("public key was not valid")
            }
            _ => PublicKey::from_bytes(&self.read_key_256(KeyRomLocs::SELFSIGN_PUBKEY)).expect("public key was not valid")
        };
        self.purge_sensitive_data();
        self.spinor.set_staging_write_protect(false).expect("couldn't un-protect the staging area");
        for b in keypair_bytes.iter_mut() {
            *b = 0;
        }
        for b in old_key.iter_mut() {
            *b = 0;
        }
        // ed25519 keypair zeroizes on drop

        // check signatures
        if keypair.is_some() {
            pb.set_percentage(92);
            if !self.verify_gateware_self_signature(Some(&pubkey)) {
                return Err(RootkeyResult::IntegrityError);
            }
            // as a sanity check, check the kernel self signature
            if !self.verify_selfsign_kernel(Some(&pubkey)) {
                log::error!("kernel signature failed to verify, probably should not try to reboot!");
                return Err(RootkeyResult::IntegrityError);
            }
        }

        if update_type == UpdateType::BbramProvision {
            pb.set_percentage(95);
            self.ticktimer.sleep_ms(500).unwrap();
            // this will kick off the programming
            log::info!("BURN_NOW");
            log::info!("{}", crate::CONSOLE_SENTINEL);

            // the key burning routine should finish before this timeout happens, and the system should have been rebooted
            self.ticktimer.sleep_ms(10_000).unwrap();

            pb.update_text(t!("rootkeys.bbram.failed_restore", xous::LANG));
            pb.set_percentage(0);
            pb.rebase_subtask_percentage(0, 100);
            self.make_gateware_backup(Some(&mut pb), true)?;
        } else if update_type == UpdateType::EfuseProvision {
            pb.update_text(t!("rootkeys.efuse_burning", xous::LANG));
            self.ticktimer.sleep_ms(300).ok();
            pb.set_percentage(93);
            self.ticktimer.sleep_ms(300).ok();
            #[cfg(feature="hazardous-debug")]
            match self.jtag.efuse_fetch() {
                Ok(rec) => {
                    log::info!("Efuse record before burn: {:?}", rec);
                }
                Err(e) => {
                    log::error!("failed to fetch jtag record: {:?}", e);
                    return Err(RootkeyResult::IntegrityError);
                }
            }
            log::info!("{}EFUSE.BURN,{}", xous::BOOKEND_START, xous::BOOKEND_END);
            // burn the key here to eFuses
            match self.jtag.efuse_key_burn(pcache.fpga_key) {
                Ok(result) => {
                    if !result {
                        pb.update_text(t!("rootkeys.efuse_burn_fail", xous::LANG));
                        self.ticktimer.sleep_ms(2000).ok();
                        return Err(RootkeyResult::KeyError)
                    } else {
                        log::info!("{}EFUSE.BURN_OK,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                    }
                }
                Err(e) => {
                    pb.update_text(&format!("{}\n{:?}", t!("rootkeys.efuse_internal_error", xous::LANG), e));
                    self.ticktimer.sleep_ms(2000).ok();
                    return Err(RootkeyResult::StateError)
                }
            }
            log::info!("{}EFUSE.BURN_DONE,{}", xous::BOOKEND_START, xous::BOOKEND_END);

            #[cfg(feature="hazardous-debug")]
            match self.jtag.efuse_fetch() {
                Ok(rec) => {
                    log::info!("Efuse record readback: {:?}", rec);
                }
                Err(e) => {
                    log::error!("failed to fetch jtag record: {:?}", e);
                    return Err(RootkeyResult::IntegrityError);
                }
            }

            // seal the device from key readout, force encrypted boot
            pb.update_text(t!("rootkeys.efuse_sealing", xous::LANG));
            pb.set_percentage(96);
            log::info!("{}EFUSE.SEAL,{}", xous::BOOKEND_START, xous::BOOKEND_END);
            match self.jtag.seal_device() {
                Ok(result) => {
                    if !result {
                        pb.update_text(t!("rootkeys.efuse_seal_fail", xous::LANG));
                        self.ticktimer.sleep_ms(2000).ok();
                        return Err(RootkeyResult::FlashError)
                    }
                }
                Err(e) => {
                    pb.update_text(&format!("{}\n{:?}", t!("rootkeys.efuse_internal_error", xous::LANG), e));
                    self.ticktimer.sleep_ms(2000).ok();
                    return Err(RootkeyResult::StateError)
                }
            }
            log::info!("{}EFUSE.SEAL_OK,{}", xous::BOOKEND_START, xous::BOOKEND_END);

            #[cfg(feature="hazardous-debug")]
            match self.jtag.efuse_fetch() {
                Ok(rec) => {
                    log::info!("Efuse record after seal: {:?}", rec);
                }
                Err(e) => {
                    log::error!("failed to fetch jtag record: {:?}", e);
                    return Err(RootkeyResult::IntegrityError);
                }
            }
        }
        pb.set_percentage(100);

        // check if we're to purge the password on completion
        if self.update_password_policy == PasswordRetentionPolicy::AlwaysPurge {
            self.purge_password(PasswordType::Update);
        }
        // log::set_max_level(log::LevelFilter::Info);
        Ok(())
    }

    pub fn do_sign_xous(&mut self, rootkeys_modal: &mut Modal, main_cid: xous::CID) -> Result<(), RootkeyResult> {
        // make sure the system is sane
        self.xous_init_interlock();

        // setup Ux
        let mut progress_action = Slider::new(main_cid, Opcode::UxGutter.to_u32().unwrap(),
        0, 100, 10, Some("%"), 0, true, true
        );
        progress_action.set_is_password(true);
        // now show the init wait note...
        rootkeys_modal.modify(
            Some(ActionType::Slider(progress_action)),
            Some(t!("rootkeys.gwup_starting", xous::LANG)), false,
            None, true, None);
        rootkeys_modal.activate();
        xous::yield_slice(); // give some time to the GAM to render
        let mut pb = ProgressBar::new(rootkeys_modal, &mut progress_action);
        pb.set_percentage(1);

        // derive signing key
        let pcache: &mut PasswordCache = unsafe{&mut *(self.pass_cache.as_mut_ptr() as *mut PasswordCache)};
        if pcache.hashed_update_pw_valid == 0 {
            self.purge_password(PasswordType::Update);
            log::error!("no password was set going into the update routine");
            #[cfg(feature = "hazardous-debug")]
            log::debug!("key: {:x?}", pcache.hashed_update_pw);
            log::debug!("valid: {}", pcache.hashed_update_pw_valid);

            return Err(RootkeyResult::KeyError);
        }
        let mut keypair_bytes: [u8; ed25519_dalek::KEYPAIR_LENGTH] = [0; ed25519_dalek::KEYPAIR_LENGTH];
        let enc_signing_key = self.read_key_256(KeyRomLocs::SELFSIGN_PRIVKEY);
        for (key, (&enc_key, &pw)) in
        keypair_bytes[..ed25519_dalek::SECRET_KEY_LENGTH].iter_mut()
        .zip(enc_signing_key.iter().zip(pcache.hashed_update_pw.iter())) {
            *key = enc_key ^ pw;
        }
        self.compute_key_rollback(&mut keypair_bytes[..ed25519_dalek::SECRET_KEY_LENGTH]);
        #[cfg(feature = "hazardous-debug")]
        log::debug!("keypair privkey (after anti-rollback): {:x?}", &keypair_bytes[..ed25519_dalek::SECRET_KEY_LENGTH]);
        for (key, &src) in keypair_bytes[ed25519_dalek::SECRET_KEY_LENGTH..].iter_mut()
        .zip(self.read_key_256(KeyRomLocs::SELFSIGN_PUBKEY).iter()) {
            *key = src;
        }
        // Keypair zeroizes the secret key on drop.
        let keypair = Keypair::from_bytes(&keypair_bytes).map_err(|_| RootkeyResult::KeyError)?;
        #[cfg(feature = "hazardous-debug")]
        log::debug!("keypair privkey (after anti-rollback + conversion): {:x?}", keypair.secret.to_bytes());

        // check if the keypair is valid by signing and verifying a short message
        let test_data = "whiskey made me do it";
        let test_sig = keypair.sign(test_data.as_bytes());
        match keypair.verify(&test_data.as_bytes(), &test_sig) {
            Ok(_) => (),
            Err(e) => {
                log::warn!("update password was not connect ({:?})", e);
                self.purge_password(PasswordType::Update);
                for b in keypair_bytes.iter_mut() {
                    *b = 0;
                }
                return Err(RootkeyResult::KeyError);
            }
        }

        // Question: do we want to re-verify the kernel and loader's devkey sign immediately before
        // re-signing them? Nominally, they are checked on boot, but there is an opportunity for
        // a TOCTOU by not re-verifying them.

        // sign the kernel
        pb.update_text(t!("rootkeys.init.signing_kernel", xous::LANG));
        pb.set_percentage(35);
        let (kernel_sig, kernel_len) = self.sign_kernel(&keypair);

        // sign the loader
        pb.update_text(t!("rootkeys.init.signing_loader", xous::LANG));
        pb.rebase_subtask_percentage(35, 85);
        let (loader_sig, loader_len) = self.sign_loader(&keypair, Some(&mut pb));
        log::info!("loader signature: {:x?}", loader_sig.to_bytes());
        log::info!("loader len: {} bytes", loader_len);

        // commit the signatures
        pb.update_text(t!("rootkeys.init.commit_signatures", xous::LANG));
        self.commit_signature(loader_sig, loader_len, SignatureType::Loader)?;
        log::debug!("loader {} bytes, sig: {:x?}", loader_len, loader_sig.to_bytes());
        pb.set_percentage(90);
        self.commit_signature(kernel_sig, kernel_len, SignatureType::Kernel)?;
        pb.set_percentage(92);

        // as a sanity check, check the kernel self signature
        let ret = if !self.verify_selfsign_kernel(None) {
            log::error!("kernel signature failed to verify, probably should not try to reboot!");
            Err(RootkeyResult::IntegrityError)
        } else {
            Ok(())
        };

        // check if we're to purge the password on completion
        if self.update_password_policy == PasswordRetentionPolicy::AlwaysPurge {
            self.purge_password(PasswordType::Update);
        }
        // purge the temporaries that we can
        for b in keypair_bytes.iter_mut() {
            *b = 0;
        }
        // ed25519 keypair zeroizes on drop

        pb.set_percentage(100);
        self.ticktimer.sleep_ms(250).expect("couldn't show final message");

        ret
    }


    pub fn test(&mut self, rootkeys_modal: &mut Modal, main_cid: xous::CID) -> Result<(), RootkeyResult> {
        let mut progress_action = Slider::new(main_cid, Opcode::UxGutter.to_u32().unwrap(),
        0, 100, 10, Some("%"), 0, true, true
        );
        progress_action.set_is_password(true);
        // now show the init wait note...
        rootkeys_modal.modify(
            Some(ActionType::Slider(progress_action)),
            Some(t!("rootkeys.setup_wait", xous::LANG)), false,
            None, true, None);
        rootkeys_modal.activate();

        xous::yield_slice(); // give some time to the GAM to render
        // capture the progress bar elements in a convenience structure
        let mut pb = ProgressBar::new(rootkeys_modal, &mut progress_action);

        // kick the progress bar to indicate we've entered the routine
        for i in 1..100 {
            pb.set_percentage(i);
            self.ticktimer.sleep_ms(50).unwrap();
        }
        self.ticktimer.sleep_ms(1000).unwrap();
        // the stop emoji, when sent to the slider action bar in progress mode, will cause it to close and relinquish focus
        rootkeys_modal.key_event(['🛑', '\u{0000}', '\u{0000}', '\u{0000}']);

        #[cfg(feature = "hazardous-debug")]
        {
            let pcache: &mut PasswordCache = unsafe{&mut *(self.pass_cache.as_mut_ptr() as *mut PasswordCache)};

            // setup the local cache
            for addr in 0..256 {
                self.keyrom.wfo(utra::keyrom::ADDRESS_ADDRESS, addr);
                self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[addr as usize] = self.keyrom.rf(utra::keyrom::DATA_DATA);
                log::info!("{:02x}: 0x{:08x}", addr, self.sensitive_data.borrow_mut().as_slice::<u32>()[addr as usize]);
            }

            for (word, key) in self.sensitive_data.borrow().as_slice::<u32>()[KeyRomLocs::FPGA_KEY as usize..KeyRomLocs::FPGA_KEY as usize + 256/(size_of::<u32>()*8)].iter()
            .zip(pcache.fpga_key.chunks_mut(4).into_iter()) {
                for (&s, d) in word.to_be_bytes().iter().zip(key.iter_mut()) {
                    *d = s;
                }
            }
            pcache.fpga_key_valid = 1;
            self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[KeyRomLocs::CONFIG as usize] |= keyrom_config::INITIALIZED.ms(1);
            self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[0x30] = 0xc0de_600d;
            self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[0x31] = 0x1234_5678;
            self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[0x32] = 0x8000_0000;
            self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[0x33] = 0x5555_3333;

            // one time only
            /*
            match self.make_gateware_backup(30, 50, progress_modal, progress_action, false) {
                Err(e) => {
                    log::error!("got spinor error: {:?}", e);
                    panic!("got spinor error, halting");
                }
                _ => ()
            };*/

            // make the oracles
            assert!(pcache.fpga_key_valid == 1);
            // copy from staging
            let src_oracle = BitstreamOracle::new(&pcache.fpga_key, &pcache.fpga_key, self.staging(), self.staging_base()).unwrap();
            // write to gateware
            let dst_oracle = BitstreamOracle::new(&pcache.fpga_key, &pcache.fpga_key, self.gateware(), self.gateware_base()).unwrap();

            // TEST
            if self.gateware_copy_and_patch(&src_oracle, &dst_oracle, None).is_err() {
                log::error!("error occured in patch_keys.");
                return Err(RootkeyResult::FlashError);
            }

            if self.verify_gateware(&dst_oracle, None).is_err() {
                log::error!("error occurred in gateware verification");
                return Err(RootkeyResult::IntegrityError);
            }
        }

        Ok(())
    }

    /// blind-copy a staged gateware to the boot location. This can only be done if not root keys
    /// have been initialized. This routine exists just to make the UX flow consistent between
    /// unprovisioned and provisioned devices.
    pub(crate) fn do_gateware_provision_uninitialized(&mut self, rootkeys_modal: &mut Modal, main_cid: xous::CID) -> Result<(), RootkeyResult> {
        if self.is_initialized() {
            return Err(RootkeyResult::StateError);
        }
        let mut progress_action = Slider::new(main_cid, Opcode::UxGutter.to_u32().unwrap(),
            0, 100, 10, Some("%"), 0, true, true
        );
        progress_action.set_is_password(true);
        rootkeys_modal.modify(
            Some(ActionType::Slider(progress_action)),
            Some(t!("rootkeys.gwup_starting", xous::LANG)), false,
            None, true, None);
        rootkeys_modal.activate();
        xous::yield_slice(); // give some time to the GAM to render
        let mut pb = ProgressBar::new(rootkeys_modal, &mut progress_action);
        pb.set_percentage(1);
        self.ticktimer.sleep_ms(250).expect("couldn't show final message");
        self.make_gateware_backup(Some(&mut pb), true)?;
        pb.set_percentage(100);
        self.ticktimer.sleep_ms(250).expect("couldn't show final message");
        Ok(())
    }

    /// copy data from a source region to a destination region, re-encrypting and patching as we go along.
    /// the region/region_base should be specified for the destination oracle
    ///
    /// KEYROM patching data must previously have been staged in the sensitive area.
    /// failure to do so would result in the erasure of all secret data.
    /// ASSUME: CSR appendix does not change during the copy (it is not copied/updated)
    fn gateware_copy_and_patch(&self, src_oracle: &BitstreamOracle, dst_oracle: &BitstreamOracle,
    mut maybe_pb: Option<&mut ProgressBar>) -> Result<(), RootkeyResult> {
        log::debug!("sanity checks: src_offset {}, dst_offset {}, src_len {}, dst_len {}",
            src_oracle.ciphertext_offset(), dst_oracle.ciphertext_offset(), src_oracle.ciphertext_len(), dst_oracle.ciphertext_len());

        // start with a naive implementation that simple goes through and re-encrypts and patches everything.
        // later on, we could skip patching everything up to the first frame that needs patching;
        // this should optimize up to 15% of the time required to do a patch, although, because the patching
        // routine is "smart" and does not issue writes for unchanged data, this could actually be a much
        // smaller performance gain.

        let mut pt_sector: [u8; spinor::SPINOR_ERASE_SIZE as usize] = [0; spinor::SPINOR_ERASE_SIZE as usize];
        let mut ct_sector: [u8; spinor::SPINOR_ERASE_SIZE as usize] = [0; spinor::SPINOR_ERASE_SIZE as usize];
        let mut flipper: [u8; spinor::SPINOR_ERASE_SIZE as usize] = [0; spinor::SPINOR_ERASE_SIZE as usize];

        let mut hasher = Sha256::new();
        let hash_stop = src_oracle.ciphertext_len() - 320 - 160;

        // handle the special case of the first sector
        src_oracle.decrypt(0, &mut pt_sector);

        // initialize the hmac code for later use
        let mut hmac_code: [u8; 32] = [0; 32];
        for (dst, (&hm1, &mask)) in
        hmac_code.iter_mut()
        .zip(pt_sector[0..32].iter().zip(pt_sector[32..64].iter())) {
            *dst = hm1 ^ mask;
        }
        log::debug!("recovered hmac: {:x?}", hmac_code);
        log::debug!("hmac constant: {:x?}", &pt_sector[32..64]);
        let mut bytes_hashed = spinor::SPINOR_ERASE_SIZE as usize - src_oracle.ciphertext_offset();

        // encrypt and patch the data to disk
        dst_oracle.encrypt_sector(
            -(dst_oracle.ciphertext_offset() as i32),
            &mut pt_sector[..bytes_hashed],
            &mut ct_sector // full array, for space for plaintext header
        );

        // hash the first sector; the pt_sector could have been patched by encrypt_sector() to change the key soucre,
        // so it must be done *after* we call encrypt_sector()
        bitflip(&pt_sector[..bytes_hashed], &mut flipper[..bytes_hashed]);
        hasher.update(&flipper[..bytes_hashed]);

        log::debug!("sector 0 patch len: {}", bytes_hashed);
        log::debug!("sector 0 header: {:x?}", &ct_sector[..dst_oracle.ciphertext_offset()]);
        self.spinor.patch(dst_oracle.bitstream(), dst_oracle.base(), &ct_sector, 0)
        .map_err(|_| RootkeyResult::FlashError)?;

        // now we can patch the rest of the sectors as a loop
        let mut from = spinor::SPINOR_ERASE_SIZE - src_oracle.ciphertext_offset() as u32;
        let mut dummy_consume = 0;
        if let Some(ref mut pb) = maybe_pb {
            pb.rebase_subtask_work(0,
                src_oracle.ciphertext()[from as usize..].chunks(spinor::SPINOR_ERASE_SIZE as usize).into_iter().count() as u32);
        }
        for _ in src_oracle.ciphertext()[from as usize..].chunks(spinor::SPINOR_ERASE_SIZE as usize).into_iter() {
            let decrypt_len = src_oracle.decrypt(from as usize, &mut pt_sector);

            // wrap the patch call with a range check, because the patch lookup search is pretty expensive
            if self.patch_in_range(src_oracle, from, from + spinor::SPINOR_ERASE_SIZE as u32) {
                dummy_consume ^= self.patch_sector(src_oracle, from, &mut pt_sector, &self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[0..256]);
            }

            let hash_len = if bytes_hashed + decrypt_len < hash_stop {
                decrypt_len
            } else {
                hash_stop - bytes_hashed
            };

            // encrypt before hashing, because a bit that selects the key type can be patched by the encryptor
            dst_oracle.encrypt_sector(
                from as i32,
                &mut pt_sector[..decrypt_len],
                &mut ct_sector[..decrypt_len],
            );

            if hash_len > 0 {
                bitflip(&pt_sector[..hash_len], &mut flipper[..hash_len]);
                hasher.update(&flipper[..hash_len]);
                bytes_hashed += hash_len;
                if hash_len != decrypt_len {
                    log::debug!("final short block len: {}", hash_len);
                }
            }

            self.spinor.patch(dst_oracle.bitstream(), dst_oracle.base(),
                &ct_sector[..decrypt_len], from + dst_oracle.ciphertext_offset() as u32)
            .map_err(|_| RootkeyResult::FlashError)?;

            from += decrypt_len as u32;
            if let Some(ref mut pb) = maybe_pb {
                pb.increment_work(1);
            }
        }
        // consume the dummy value, to ensure it's not optimized out by the compiler
        log::debug!("copy_and_patch dummy consume: 0x{:x}", dummy_consume);


        // at this point, we've patched & encrypted all the sectors, but we've
        // encrypted the wrong hash at the end, so the HMAC won't work out. let's patch that.
        let h1_digest: [u8; 32] = hasher.finalize().try_into().unwrap();
        log::debug!("bytes hashed: {}, computed h1 hash: {:x?}", bytes_hashed, h1_digest);

        let mut hasher2 = Sha256::new();
        let footer_mask: [u8; 32] = [0x3A; 32];
        let mut masked_footer: [u8; 32] = [0; 32];
        for (dst, (&hm2, &mask)) in
        masked_footer.iter_mut()
        .zip(hmac_code.iter().zip(footer_mask.iter())) {
            *dst = hm2 ^ mask;
        }
        log::debug!("masked_footer: {:x?}", masked_footer);
        log::debug!("footer_mask: {:x?}", footer_mask);
        let mut masked_footer_flipped: [u8; 32] = [0; 32];
        bitflip(&masked_footer, &mut masked_footer_flipped);
        let mut footer_mask_flipped: [u8; 32] = [0; 32];
        bitflip(&footer_mask, &mut footer_mask_flipped);
        hasher2.update(masked_footer_flipped);
        hasher2.update(footer_mask_flipped);
        hasher2.update(h1_digest);
        let h2_digest: [u8; 32] = hasher2.finalize().try_into().unwrap();
        log::debug!("h2 hash: {:x?}", h2_digest);

        // now encrypt and patch this final hashed value into the expected area
        let ct_end = dst_oracle.ciphertext_len();

        // the math below will have some problems if the block happens to end up exactly on an erase block boundary.
        // but it doesn't, so I'm going to leave that case unaddressed...
        let ct_last_block_loc = (ct_end & !(spinor::SPINOR_ERASE_SIZE as usize - 1)) - dst_oracle.ciphertext_offset();
        let pt_sector_len = ct_end - ct_last_block_loc;

        log::trace!("ct_end: {}, ct_last_block_loc {}, pt_sector_len {}", ct_end, ct_last_block_loc, pt_sector_len);
        src_oracle.decrypt(ct_last_block_loc, &mut pt_sector[..pt_sector_len]);

        // patch in the hash at the very end
        let mut h2_digest_flipped: [u8; 32] = [0; 32];
        bitflip(&h2_digest, &mut h2_digest_flipped);
        for (&src, dst) in h2_digest_flipped.iter()
        .zip(pt_sector[pt_sector_len - 32..pt_sector_len].iter_mut()) {
            *dst = src;
        }
        log::debug!("last bytes patched: {:x?}", &pt_sector[pt_sector_len-256..pt_sector_len]);

        dst_oracle.encrypt_sector(ct_last_block_loc as i32, &mut pt_sector[..pt_sector_len], &mut ct_sector[..pt_sector_len]);
        log::trace!("hash patching from 0x{:x} len {}", ct_last_block_loc, pt_sector_len);
        self.spinor.patch(dst_oracle.bitstream(), dst_oracle.base(),
            &ct_sector[..pt_sector_len], ct_last_block_loc as u32 + dst_oracle.ciphertext_offset() as u32)
        .map_err(|_| RootkeyResult::FlashError)?;

        Ok(())
    }

    fn patch_in_range(&self, oracle: &BitstreamOracle, range_ct_start: u32, range_ct_end: u32) -> bool {
        let (start_frame, _) = oracle.ciphertext_offset_to_frame(range_ct_start as usize);
        let (end_frame, _) = oracle.ciphertext_offset_to_frame(range_ct_end as usize);

        if end_frame < patch::PATCH_FRAMES[0] as usize || start_frame > *patch::PATCH_FRAMES.last().unwrap() as usize {
            false
        } else {
            true
        }
    }
    // data_ct_start is the ciphertext offset of the region we're searching for a patch in
    fn patch_sector(&self, oracle: &BitstreamOracle, data_ct_start: u32, data: &mut [u8], keyrom: &[u32]) -> u32 {
        assert!(keyrom.len() == 256, "KEYROM data is not whole");

        let mut dummy_consume = 0;
        for (offset, word) in data.chunks_mut(4).into_iter().enumerate() {
            let ct_offset = data_ct_start as usize + offset * 4;
            let (frame, frame_offset) = oracle.ciphertext_offset_to_frame(ct_offset);

            // the outer should_patch() call is a small optimization that replaces an O(N) search through
            // a frame array with a pair of limit comparisons.
            if patch::should_patch(frame as u32) {
                match patch::patch_frame(frame as u32, frame_offset as u32, keyrom) {
                    Some((patch, patch_inv)) => {
                        let mut flip: [u8; 4] = [0; 4];
                        bitflip(&patch.to_be_bytes(), &mut flip);
                        log::trace!("patching {} frame 0x{:x} offset {} 0x{:x} -> 0x{:x}", ct_offset, frame, frame_offset,
                            u32::from_be_bytes(word[0..4].try_into().unwrap()), u32::from_be_bytes(flip));
                        for (&s, d) in flip.iter().zip(word.iter_mut()) {
                            *d = s;
                        }
                        dummy_consume ^= patch_inv; // consume the dummy value (to keep constant time properties)
                    }
                    None => {}
                }
            }
        }
        dummy_consume
    }

    fn verify_gateware(&self, oracle: &BitstreamOracle, mut maybe_pb: Option<&mut ProgressBar>) -> Result<(), RootkeyResult> {
        let mut hmac_area = [0; 64];
        oracle.decrypt(0, &mut hmac_area);
        let mut hmac_code: [u8; 32] = [0; 32];
        for (dst, (&hm1, &mask)) in
        hmac_code.iter_mut()
        .zip(hmac_area[0..32].iter().zip(hmac_area[32..64].iter())) {
            *dst = hm1 ^ mask;
        }
        log::debug!("hmac code: {:x?}", hmac_code);

        log::debug!("verifying gateware");
        let mut hasher = Sha256::new();
        // magic number alert:
        // 160 = reserved space for the 2nd hash
        // 320 = some padding that is built into the file format, for whatever reason xilinx picked
        let tot_len = oracle.ciphertext_len() - 320 - 160;
        // slow but steady. we can optimize later.
        // for now, very tiny optimization - use blocksize * 2 because that happens to be divisible by the bitstream parameters
        let mut decrypt = [0; AES_BLOCKSIZE*2];
        let mut flipped = [0; AES_BLOCKSIZE*2];
        if let Some(ref mut pb) = maybe_pb {
            pb.rebase_subtask_work(0, tot_len as u32);
        }
        for index in (0..tot_len).step_by(AES_BLOCKSIZE*2) {
            oracle.decrypt(index, &mut decrypt);
            bitflip(&decrypt, &mut flipped);
            hasher.update(&flipped);

            if let Some(ref mut pb) = maybe_pb {
                pb.increment_work((AES_BLOCKSIZE*2) as u32);
            }
        }
        let h1_digest: [u8; 32] = hasher.finalize().try_into().unwrap();
        log::debug!("computed hash of {} bytes: {:x?}", tot_len, h1_digest);

        let mut hasher2 = Sha256::new();
        let footer_mask: [u8; 32] = [0x3A; 32];
        let mut masked_footer: [u8; 32] = [0; 32];
        for (dst, (&hm2, &mask)) in
        masked_footer.iter_mut()
        .zip(hmac_code.iter().zip(footer_mask.iter())) {
            *dst = hm2 ^ mask;
        }
        log::debug!("masked_footer: {:x?}", masked_footer);
        log::debug!("footer_mask: {:x?}", footer_mask);
        let mut masked_footer_flipped: [u8; 32] = [0; 32];
        bitflip(&masked_footer, &mut masked_footer_flipped);
        let mut footer_mask_flipped: [u8; 32] = [0; 32];
        bitflip(&footer_mask, &mut footer_mask_flipped);
        hasher2.update(masked_footer_flipped);
        hasher2.update(footer_mask_flipped);
        hasher2.update(h1_digest);
        let h2_digest: [u8; 32] = hasher2.finalize().try_into().unwrap();

        log::debug!("h2 hash: {:x?}", h2_digest);
        let mut ref_digest_flipped: [u8; 32] = [0; 32];
        oracle.decrypt(oracle.ciphertext_len() - 32, &mut ref_digest_flipped);
        let mut ref_digest: [u8; 32] = [0; 32];
        log::debug!("ref digest (flipped): {:x?}", ref_digest_flipped);
        bitflip(&ref_digest_flipped, &mut ref_digest);
        log::debug!("ref digest          : {:x?}", ref_digest);

        let mut matching = true;
        for (&l, &r) in ref_digest.iter().zip(h2_digest.iter()) {
            if l != r {
                matching = false;
            }
        }

        if matching {
            log::debug!("gateware verified");
            Ok(())
        } else {
            log::error!("gateware failed to verify");
            Err(RootkeyResult::IntegrityError)
        }
    }


    fn make_gateware_backup(&self, mut maybe_pb: Option<&mut ProgressBar>, do_restore: bool) -> Result<(), RootkeyResult> {
        let gateware_dest = if !do_restore {self.staging()} else {self.gateware()};
        let mut gateware_dest_base = if !do_restore {self.staging_base()} else {self.gateware_base()};
        let gateware_src = if !do_restore {self.gateware()} else {self.staging()};

        const PATCH_CHUNK: usize = 65536; // this controls the granularity of the erase operation
        let mut prog_ctr = 0;
        if let Some(ref mut pb) = maybe_pb {
            pb.rebase_subtask_work(0, xous::SOC_STAGING_GW_LEN);
        }

        for (dst, src) in
        gateware_dest.chunks(PATCH_CHUNK).into_iter()
        .zip(gateware_src.chunks(PATCH_CHUNK)) {
            log::debug!("writing {} backup bytes to offset 0x{:08x}", src.len(), prog_ctr);
            self.spinor.patch(dst, gateware_dest_base, src, 0)
                .map_err(|_| RootkeyResult::FlashError)?;
            gateware_dest_base += PATCH_CHUNK as u32;

            prog_ctr += PATCH_CHUNK as u32;
            if let Some(ref mut pb) = maybe_pb {
                pb.increment_work(PATCH_CHUNK as u32);
            }
        }

        Ok(())
    }

    #[cfg(feature = "hazardous-debug")]
    fn debug_staging(&self) {
        self.debug_print_key(KeyRomLocs::FPGA_KEY as usize, 256, "FPGA key: ");
        self.debug_print_key(KeyRomLocs::SELFSIGN_PRIVKEY as usize, 256, "Self private key: ");
        self.debug_print_key(KeyRomLocs::SELFSIGN_PUBKEY as usize, 256, "Self public key: ");
        self.debug_print_key(KeyRomLocs::DEVELOPER_PUBKEY as usize, 256, "Dev public key: ");
        self.debug_print_key(KeyRomLocs::THIRDPARTY_PUBKEY as usize, 256, "3rd party public key: ");
        self.debug_print_key(KeyRomLocs::USER_KEY as usize, 256, "Boot key: ");
        self.debug_print_key(KeyRomLocs::PEPPER as usize, 128, "Pepper: ");
        self.debug_print_key(KeyRomLocs::CONFIG as usize, 32, "Config (as BE): ");
        self.debug_print_key(KeyRomLocs::GLOBAL_ROLLBACK as usize, 32, "Global rollback state: ");
    }

    #[cfg(feature = "hazardous-debug")]
    fn debug_print_key(&self, offset: usize, num_bits: usize, name: &str) {
        use core::fmt::Write;
        let mut debugstr = xous_ipc::String::<4096>::new();
        write!(debugstr, "{}", name).unwrap();
        for word in self.sensitive_data.borrow_mut().as_slice::<u32>()[offset .. offset as usize + num_bits/(size_of::<u32>()*8)].iter() {
            for byte in word.to_be_bytes().iter() {
                write!(debugstr, "{:02x}", byte).unwrap();
            }
        }
        log::info!("{}", debugstr);
    }

    /// Unfortunately we have to re-implement the signature details manually here.
    /// these are based on copying the implementation out of the ed25519-dalek crate.
    /// why, may you ask, do we do such a dangerous thing?
    /// it's because the loader's memory space is shared between this crate, and the graphics crate
    /// the font glyph data is part of the loader, it was put there because:
    ///   - we need fonts early in boot to show data
    ///   - like the loader, it's a large static data set that rarely changes
    ///   - this helps keep kernel updates faster and lighter, at the trade-off of some inconvenience of loader updates
    ///   - because we have virtual memory, we must "borrow" copies of the font data for signing
    ///   - because we have limited physical memory, it's not a great idea to simply allocate a huge block of RAM
    ///     and copy the loader + font data there and sign it; it means in low memory conditions we can't do signatures
    /// The ideal solution for us is to pre-hash the entire loader region, and then have it signed.
    /// the ed25519-dalek crate does support this, but, it's also not a "standard" hash and sign, because
    /// it adds some good ideas to the signature. For entirely self-signed regions, we can use the ed25519-dalek's
    /// version of the signature, but for interoperability we have to stick with the traditional signature.
    /// Unfortunately, the traditional signature implementation only supports passing the entire region as a slice,
    /// and not as a pre-hash. This is because the hash has to be primed with a nonce that's derived from the
    /// secret key. So, we re-implement this, so we can interleave the hash as required to allow us to process
    /// the font data in page-sized chunks that don't use a huge amount of RAM.
    #[allow(non_snake_case)]
    pub fn sign_loader(&self, signing_key: &Keypair, maybe_pb: Option<&mut ProgressBar>) -> (Signature, u32) {
        let maybe_pb = maybe_pb.map(|pb| {pb.rebase_subtask_work(0, 2); pb});
        let loader_len =
            xous::LOADER_CODE_LEN
            - SIGBLOCK_SIZE
            + graphics_server::fontmap::FONT_TOTAL_LEN as u32
            // these also need to be updated in graphics-server/src/main.rs @ Some(Opcode::BulkReadfonts)
            + 16 // for the minimum compatible semver
            + 16 // for the current semver
            + 8; // two u32 words are appended to the end, which repeat the "version" and "length" fields encoded in the signature block

        // this is a huge hash, so, get a hardware hasher, even if it means waiting for it
        let mut hasher = Sha512::new_with_strategy(FallbackStrategy::WaitForHardware);

        // extract the secret key so we can prime the hash
        let expanded_key = ExpandedSecretKey::from(&signing_key.secret);
        let nonce = &(expanded_key.to_bytes()[32..]);
        let mut lower: [u8; 32] = [0; 32];
        lower.copy_from_slice(&(expanded_key.to_bytes()[..32]));
        let key = Scalar::from_bits(lower);
        hasher.update(nonce);

        { // this is the equivalent of hasher.update(&message)
            let loader_region = self.loader_code();
            // the loader data starts one page in; the first page is reserved for the signature itself
            hasher.update(&loader_region[SIGBLOCK_SIZE as usize..]);

            // now get the font plane data
            self.gfx.bulk_read_restart(); // reset the bulk read pointers on the gfx side
            let bulkread = BulkRead::default();
            let mut buf = xous_ipc::Buffer::into_buf(bulkread).expect("couldn't transform bulkread into aligned buffer");
            // this form of loop was chosen to avoid the multiple re-initializations and copies that would be entailed
            // in our usual idiom for pasing buffers around. instead, we create a single buffer, and re-use it for
            // every iteration of the loop.
            loop {
                buf.lend_mut(self.gfx.conn(), self.gfx.bulk_read_fontmap_op()).expect("couldn't do bulkread from gfx");
                let br = buf.as_flat::<BulkRead, _>().unwrap();
                hasher.update(&br.buf[..br.len as usize]);
                if br.len != bulkread.buf.len() as u32 {
                    log::trace!("non-full block len: {}", br.len);
                }
                if br.len < bulkread.buf.len() as u32 {
                    // read until we get a buffer that's not fully filled
                    break;
                }
            }
        }
        let maybe_pb = maybe_pb.map(|pb| {pb.increment_work(1); pb});

        let r = Scalar::from_hash(hasher);
        let R = (&r * &constants::ED25519_BASEPOINT_TABLE).compress();

        let mut hasher = Sha512::new_with_strategy(FallbackStrategy::WaitForHardware);
        hasher.update(R.as_bytes());
        hasher.update(signing_key.public.as_bytes());

        { // this is the equivalent of hasher.update(&message)
            let loader_region = self.loader_code();
            // the loader data starts one page in; the first page is reserved for the signature itself
            hasher.update(&loader_region[SIGBLOCK_SIZE as usize..]);

            // now get the font plane data
            self.gfx.bulk_read_restart(); // reset the bulk read pointers on the gfx side
            let bulkread = BulkRead::default();
            let mut buf = xous_ipc::Buffer::into_buf(bulkread).expect("couldn't transform bulkread into aligned buffer");
            // this form of loop was chosen to avoid the multiple re-initializations and copies that would be entailed
            // in our usual idiom for pasing buffers around. instead, we create a single buffer, and re-use it for
            // every iteration of the loop.
            loop {
                buf.lend_mut(self.gfx.conn(), self.gfx.bulk_read_fontmap_op()).expect("couldn't do bulkread from gfx");
                let br = buf.as_flat::<BulkRead, _>().unwrap();
                hasher.update(&br.buf[..br.len as usize]);
                if br.len != bulkread.buf.len() as u32 {
                    log::trace!("non-full block len: {}", br.len);
                }
                if br.len < bulkread.buf.len() as u32 {
                    // read until we get a buffer that's not fully filled
                    break;
                }
            }
        }
        if let Some(pb) = maybe_pb {
            pb.increment_work(1);
        }

        let k = Scalar::from_hash(hasher);
        let s = &(&k * &key) + &r;

        let mut signature_bytes: [u8; 64] = [0u8; 64];

        signature_bytes[..32].copy_from_slice(&R.as_bytes()[..]);
        signature_bytes[32..].copy_from_slice(&s.as_bytes()[..]);

        (ed25519_dalek::ed25519::signature::Signature::from_bytes(&signature_bytes).unwrap(), loader_len)
    }

    pub fn sign_kernel(&self, signing_key: &Keypair) -> (Signature, u32) {
        let kernel_region = self.kernel();

        // First, find the advertised length in the unchecked header, then, check it against the length in the signed region of the kernel
        let sig_region = &kernel_region[..core::mem::size_of::<SignatureInFlash>()];
        let sig_rec: &SignatureInFlash = unsafe{(sig_region.as_ptr() as *const SignatureInFlash).as_ref().unwrap()}; // this pointer better not be null, we just created it!

        let kernel_len = sig_rec.signed_len as usize; // this length is an unchecked guideline
        let protected_len = u32::from_le_bytes(
            kernel_region[
                SIGBLOCK_SIZE as usize + kernel_len as usize - 4 ..
                SIGBLOCK_SIZE as usize + kernel_len as usize
            ].try_into().unwrap());

        // we now have a checked length derived from a region of the kernel that is signed, confirm that it matches the advertised length
        log::info!("kernel total signed len 0x{:x}, kernel len inside signed region 0x{:x}", kernel_len, protected_len);
        // throw a panic -- this check should have passed during boot.
        assert!((kernel_len) - 4 == protected_len as usize, "The advertised kernel length does not match the signed length");

        // force the records to match our measured values
        let mut len_data = [0u8; 8];
        for (&src, dst) in SIG_VERSION.to_le_bytes().iter().zip(len_data[..4].iter_mut()) {
            *dst = src;
        }
        for (&src, dst) in (kernel_len as u32 - 4).to_le_bytes().iter().zip(len_data[4..].iter_mut()) {
            *dst = src;
        }
        log::info!("kernel len area before: {:x?}", &(self.kernel()[SIGBLOCK_SIZE as usize + kernel_len-8..SIGBLOCK_SIZE as usize + kernel_len]));
        self.spinor.patch(self.kernel(), self.kernel_base(), &len_data, SIGBLOCK_SIZE + kernel_len as u32 - 8)
            .expect("couldn't patch length area");
        log::info!("kernel len area after: {:x?}", &(self.kernel()[SIGBLOCK_SIZE as usize + kernel_len-8..SIGBLOCK_SIZE as usize + kernel_len]));

        (signing_key.sign(&kernel_region[SIGBLOCK_SIZE as usize..SIGBLOCK_SIZE as usize + kernel_len]), kernel_len as u32)
    }

    /// the public key must already be in the cache -- this version is used by the init routine, before the keys are written
    pub fn verify_selfsign_kernel(&mut self, maybe_pubkey: Option<&PublicKey>) -> bool {
        let local_pk = PublicKey::from_bytes(&self.read_key_256(KeyRomLocs::SELFSIGN_PUBKEY)).expect("public key was not valid");
        let pubkey = if let Some(pk) = maybe_pubkey {
            pk
        } else {
            &local_pk
        };
        log::debug!("pubkey as reconstituted: {:x?}", pubkey);

        let kernel_region = self.kernel();
        let sig_region = &kernel_region[..core::mem::size_of::<SignatureInFlash>()];
        let sig_rec: &SignatureInFlash = unsafe{(sig_region.as_ptr() as *const SignatureInFlash).as_ref().unwrap()}; // this pointer better not be null, we just created it!
        let sig = Signature::from_bytes(&sig_rec.signature).expect("Signature malformed");

        let kern_len = sig_rec.signed_len as usize;
        log::debug!("recorded kernel len: {} bytes", kern_len);
        log::debug!("verifying with signature {:x?}", sig_rec.signature);
        log::debug!("verifying with pubkey {:x?}", pubkey.to_bytes());

        match pubkey.verify_strict(&kernel_region[SIGBLOCK_SIZE as usize..SIGBLOCK_SIZE as usize + kern_len], &sig) {
            Ok(()) => true,
            Err(e) => {
                log::error!("error verifying signature: {:?}", e);
                false
            }
        }
    }

    pub fn sign_gateware(&self, signing_key: &Keypair) -> (Signature, u32) {
        let gateware_region = self.gateware();

        (signing_key.sign(&gateware_region[..SELFSIG_OFFSET]), SELFSIG_OFFSET as u32)
    }

    /// This is a fast check on the gateware meant to be called on boot just to confirm that we're using a self-signed gateware
    pub fn verify_gateware_self_signature(&mut self, maybe_pubkey: Option<&PublicKey>) -> bool {
        let local_pk = PublicKey::from_bytes(&self.read_key_256(KeyRomLocs::SELFSIGN_PUBKEY)).expect("public key was not valid");
        let pubkey = if let Some(pk) = maybe_pubkey {
            pk
        } else {
            &local_pk
        };
        // read the signature directly out of the keyrom
        let gateware_region = self.gateware();

        let mut sig_region: [u8; core::mem::size_of::<SignatureInFlash>()] = [0; core::mem::size_of::<SignatureInFlash>()];
        for (&src, dst) in gateware_region[SELFSIG_OFFSET..SELFSIG_OFFSET + core::mem::size_of::<SignatureInFlash>()].iter()
        .zip(sig_region.iter_mut()) {
            *dst = src;
        }
        let sig_rec: &SignatureInFlash = unsafe{(sig_region.as_ptr() as *const SignatureInFlash).as_ref().unwrap()}; // this pointer better not be null, we just created it!
        let sig = match Signature::from_bytes(&sig_rec.signature) {
            Ok(s) => s,
            Err(e) => {
                log::error!("Signature malformed: {:?}", e);
                log::debug!("Raw bytes: {:x?}", &sig_rec.signature);
                return false;
            }
        };
        log::debug!("sig_rec ({}): {:x?}", sig_rec.signed_len, sig_rec.signature);
        log::debug!("sig: {:x?}", sig.to_bytes());
        log::debug!("pubkey: {:x?}", pubkey.to_bytes());

        // note that we use the *prehash* version here, this has a different signature than a straightforward ed25519
        match pubkey.verify_strict(&gateware_region[..SELFSIG_OFFSET], &sig) {
            Ok(_) => {
                log::info!("gateware verified!");
                true
            }
            Err(e) => {
                log::warn!("gateware did not verify! {:?}", e);
                false
            }
        }
    }

    /// This function does a comprehensive check of all the possible signature types in a specified gateware region
    pub fn check_gateware_signature(&mut self, region_enum: GatewareRegion) -> SignatureResult {
        let mut sig_region: [u8; core::mem::size_of::<SignatureInFlash>()] = [0; core::mem::size_of::<SignatureInFlash>()];
        {
            let region = match region_enum {
                GatewareRegion::Boot => self.gateware(),
                GatewareRegion::Staging => self.staging(),
            };
            for (&src, dst) in region[SELFSIG_OFFSET..SELFSIG_OFFSET + core::mem::size_of::<SignatureInFlash>()].iter()
            .zip(sig_region.iter_mut()) {
                *dst = src;
            }
        }
        let sig_rec: &SignatureInFlash = unsafe{(sig_region.as_ptr() as *const SignatureInFlash).as_ref().unwrap()};
        let sig = match Signature::from_bytes(&sig_rec.signature) {
            Ok(sig) => sig,
            Err(_) => return SignatureResult::MalformedSignature,
        };
        let mut sigtype = SignatureResult::SelfSignOk;
        // check against all the known signature types in detail
        loop {
            let pubkey_bytes = match sigtype {
                SignatureResult::SelfSignOk => {
                    self.read_key_256(KeyRomLocs::SELFSIGN_PUBKEY)
                },
                SignatureResult::ThirdPartyOk => {
                    self.read_key_256(KeyRomLocs::THIRDPARTY_PUBKEY)
                },
                SignatureResult::DevKeyOk => {
                    self.read_key_256(KeyRomLocs::DEVELOPER_PUBKEY)
                },
                _ => {
                    return SignatureResult::InvalidSignatureType
                }
            };
            // check for uninitialized signature records
            let mut all_zero = true;
            for &b in pubkey_bytes.iter() {
                if b != 0 {
                    all_zero = false;
                    break;
                }
            }
            if all_zero {
                match sigtype {
                    SignatureResult::SelfSignOk => sigtype = SignatureResult::ThirdPartyOk,
                    SignatureResult::ThirdPartyOk => sigtype = SignatureResult::DevKeyOk,
                    _ => return SignatureResult::Invalid
                }
                continue;
            }
            let pubkey = match PublicKey::from_bytes(&pubkey_bytes) {
                Ok(pubkey) => pubkey,
                Err(_) => return SignatureResult::InvalidPubKey,
            };

            let region = match region_enum {
                GatewareRegion::Boot => self.gateware(),
                GatewareRegion::Staging => self.staging(),
            };
            match pubkey.verify_strict(&region[..SELFSIG_OFFSET], &sig) {
                Ok(_) => break,
                Err(_e) => {
                    match sigtype {
                        SignatureResult::SelfSignOk => sigtype = SignatureResult::ThirdPartyOk,
                        SignatureResult::ThirdPartyOk => sigtype = SignatureResult::DevKeyOk,
                        _ => return SignatureResult::Invalid
                    }
                    continue;
                }
            }
        }
        sigtype
    }

    pub fn fetch_gw_metadata(&self, region_enum: GatewareRegion) -> MetadataInFlash {
        let region = match region_enum {
            GatewareRegion::Boot => self.gateware(),
            GatewareRegion::Staging => self.staging(),
        };
        let md_ptr: *const MetadataInFlash =
            (&region[METADATA_OFFSET..METADATA_OFFSET + core::mem::size_of::<MetadataInFlash>()]).as_ptr() as *const MetadataInFlash;

        unsafe{*md_ptr}.clone()
    }

    pub fn commit_signature(&self, sig: Signature, len: u32, sig_type: SignatureType) -> Result<(), RootkeyResult> {
        let mut sig_region: [u8; core::mem::size_of::<SignatureInFlash>()] = [0; core::mem::size_of::<SignatureInFlash>()];
        // map a structure onto the signature region, so we can do something sane when writing stuff to it
        let mut signature: &mut SignatureInFlash = unsafe{(sig_region.as_mut_ptr() as *mut SignatureInFlash).as_mut().unwrap()}; // this pointer better not be null, we just created it!

        signature.version = SIG_VERSION;
        signature.signed_len = len;
        signature.signature = sig.to_bytes();
        log::debug!("sig: {:x?}", sig.to_bytes());
        log::debug!("signature region to patch: {:x?}", sig_region);

        match sig_type {
            SignatureType::Loader => {
                log::info!("loader sig area before: {:x?}", &(self.loader_code()[..0x80]));
                self.spinor.patch(self.loader_code(), self.loader_base(), &sig_region, 0)
                    .map_err(|_| RootkeyResult::FlashError)?;
                log::info!("loader sig area after: {:x?}", &(self.loader_code()[..0x80]));
            }
            SignatureType::Kernel => {
                log::info!("kernel sig area before: {:x?}", &(self.kernel()[..0x80]));
                self.spinor.patch(self.kernel(), self.kernel_base(), &sig_region, 0)
                    .map_err(|_| RootkeyResult::FlashError)?;
                log::info!("kernel sig area after: {:x?}", &(self.kernel()[..0x80]));
            }
            SignatureType::Gateware => {
                log::info!("gateware sig area before: {:x?}", &(self.gateware()[SELFSIG_OFFSET..SELFSIG_OFFSET + core::mem::size_of::<SignatureInFlash>()]));
                self.spinor.patch(self.gateware(), self.gateware_base(), &sig_region, (SELFSIG_OFFSET) as u32)
                    .map_err(|_| RootkeyResult::FlashError)?;
                log::info!("gateware sig area after: {:x?}", &(self.gateware()[SELFSIG_OFFSET..SELFSIG_OFFSET + core::mem::size_of::<SignatureInFlash>()]));
            }
        }
        Ok(())
    }

    /// Called by the UX layer at the epilogue of the initialization run. Allows suspend/resume to resume,
    /// and zero-izes any sensitive data that was created in the process.
    pub fn finish_key_init(&mut self) {
        // purge the password cache, if the policy calls for it
        match self.boot_password_policy {
            PasswordRetentionPolicy::AlwaysPurge => {
                self.purge_password(PasswordType::Boot);
            },
            _ => ()
        }
        match self.update_password_policy {
            PasswordRetentionPolicy::AlwaysPurge => {
                self.purge_password(PasswordType::Update);
            },
            _ => ()
        }

        // now purge the keyrom copy and other temporaries
        self.purge_sensitive_data();
        // reset this flag to false in case this was called at the end of a restore op; harmless if it's already false.
        self.restore_running = false;

        // re-allow suspend/resume ops
        self.susres.set_suspendable(true).expect("couldn't re-allow suspend/resume");
    }

    pub fn staged_semver(&self) -> SemVer {
        let staging_meta = self.fetch_gw_metadata(GatewareRegion::Staging);
        if staging_meta.magic == 0x6174656d {
            let tag_str = std::str::from_utf8(&staging_meta.tag_str[..(staging_meta.tag_len as usize).min(64)]).unwrap_or("v0.0.0-1");
            SemVer::from_str(tag_str).unwrap_or(SemVer{maj: 0, min: 0, rev: 0, extra: 2, commit: None})
        } else if staging_meta.magic == 0xFFFF_FFFF {
            log::debug!("metadata blank");
            SemVer{maj: 0xFFFF, min: 0xFFFF, rev: 0xFFFF, extra: 0xFFFF, commit: None}
        } else {
            log::debug!("metadata corrupted: {:?}", staging_meta);
            SemVer{maj: 0, min: 0, rev: 0, extra: 3, commit: None}
        }
    }

    /// Attempt to apply an update with the following assumptions:
    ///    1. No root keys exist.
    ///    2. The staged update is newer than the current update.
    ///
    /// If any of these assumptions fail, return false. Do not crash or panic.
    ///
    /// The main utility of this function is to simplify the out of the box experience
    /// before root keys have been added.
    pub fn try_nokey_soc_update(&mut self, rootkeys_modal: &mut Modal, main_cid: xous::CID) -> bool {
        if self.is_initialized() {
            log::warn!("No-touch update attempted, but keys are initialized. Aborting.");
            return false;
        }
        let staged_sv = self.staged_semver();

        // the semver check *should* be done already, but we check the gateware as written
        // to FLASH and not what's reported from the `llio`. The condition at which these are
        // inconsistent is someone has applied the update but did not do the "cold boot" to
        // force a reload of the SoC. In this case, do not keep applying the same update over
        // and over again. We also do this because we skip all UX interaction in this flow,
        // unlike the UxBlindUpdate call, which has an explicit approval step in it.
        let soc_meta = self.fetch_gw_metadata(GatewareRegion::Boot);
        let tag_str = std::str::from_utf8(&soc_meta.tag_str[..soc_meta.tag_len as usize]).unwrap_or("v0.0.0-1");
        let soc_sv = SemVer::from_str(tag_str).unwrap_or(SemVer{maj: 0, min: 0, rev: 0, extra: 0, commit: None});
        if staged_sv > soc_sv {
            let mut progress_action = Slider::new(main_cid, Opcode::UxGutter.to_u32().unwrap(),
            0, 100, 10, Some("%"), 0, true, true
            );
            progress_action.set_is_password(true);
            rootkeys_modal.modify(
                Some(ActionType::Slider(progress_action)),
                Some(t!("rootkeys.gwup_starting", xous::LANG)), false,
                None, true, None);
            rootkeys_modal.activate();
            xous::yield_slice(); // give some time to the GAM to render
            let mut pb = ProgressBar::new(rootkeys_modal, &mut progress_action);
            pb.set_percentage(1);
            self.ticktimer.sleep_ms(250).expect("couldn't show final message");
            let ret = self.make_gateware_backup(Some(&mut pb), true).is_ok();
            pb.set_percentage(100);
            self.ticktimer.sleep_ms(250).expect("couldn't show final message");
            // the stop emoji, when sent to the slider action bar in progress mode, will cause it to close and relinquish focus
            rootkeys_modal.key_event(['🛑', '\u{0000}', '\u{0000}', '\u{0000}']);
            ret
        } else {
            log::warn!("No-touch update attempted, but the staged version is not newer than the existing version. Aborting.");
            false
        }
    }
    pub fn should_prompt_for_update(&mut self) -> bool {
        let soc_region = self.staging();
        let status = u32::from_le_bytes(soc_region[soc_region.len() - 4..].try_into().unwrap());
        log::info!("prompt for update: {:x?}", status);
        if status == 0xFFFF_FFFF {
            true
        } else {
            false
        }
    }
    /// the update prompt is reset every time you stage a new update
    pub fn set_prompt_for_update(&mut self, state: bool) {
        let patch_data = if state {
            [0xffu8; 4]
        } else {
            [0x0u8; 4]
        };
        self.spinor.patch(
            self.staging(),
            self.staging_base(),
            &patch_data,
            self.staging().len() as u32 - 4
        ).expect("couldn't patch update prompt");
    }
    pub fn is_dont_ask_init_set(&mut self) -> bool {
        let soc_region = self.gateware();
        let status = u32::from_le_bytes(soc_region[soc_region.len() - 4..].try_into().unwrap());
        log::info!("prompt for root key init: {:x?}", status);
        // just check the first byte, although a full word is written for the flag
        status != 0xffff_ffff
    }
    /// this is reset every time the gateware is updated. That's rather intentional, if someone
    /// *is* updating their gateware and they haven't initialized root keys...maybe they should?
    pub fn set_dont_ask_init(&mut self) {
        self.spinor.patch(
            self.gateware(),
            self.gateware_base(),
            &[0u8; 4],
            self.gateware().len() as u32 - 4,
        ).expect("couldn't erase backup region");
    }
    pub fn reset_dont_ask_init(&mut self) {
        self.spinor.patch(
            self.gateware(),
            self.gateware_base(),
            &[0xffu8; 4],
            self.gateware().len() as u32 - 4,
        ).expect("couldn't erase backup region");
    }
    pub fn read_backup_header(&mut self) -> Option<BackupHeader> {
        let kernel = self.kernel();
        let backup = &kernel[KERNEL_BACKUP_OFFSET as usize..KERNEL_BACKUP_OFFSET as usize + size_of::<BackupHeader>()];
        let mut header = BackupHeader::default();
        header.as_mut().copy_from_slice(backup);
        if (header.version & BACKUP_VERSION_MASK) == (BACKUP_VERSION & BACKUP_VERSION_MASK) {
            Some(header)
        } else {
            None
        }
    }
    pub fn get_backup_key(&mut self) -> Option<(backups::BackupKey, backups::KeyRomExport)> {
        // make sure the system is sane
        self.xous_init_interlock();

        // derive signing key
        let pcache: &mut PasswordCache = unsafe{&mut *(self.pass_cache.as_mut_ptr() as *mut PasswordCache)};
        if pcache.hashed_update_pw_valid == 0 {
            self.purge_password(PasswordType::Update);
            log::error!("no password was set going into the update routine");
            #[cfg(feature = "hazardous-debug")]
            log::debug!("key: {:x?}", pcache.hashed_update_pw);
            log::debug!("valid: {}", pcache.hashed_update_pw_valid);

            return None;
        }
        let mut keypair_bytes: [u8; ed25519_dalek::KEYPAIR_LENGTH] = [0; ed25519_dalek::KEYPAIR_LENGTH];
        let enc_signing_key = self.read_key_256(KeyRomLocs::SELFSIGN_PRIVKEY);
        for (key, (&enc_key, &pw)) in
        keypair_bytes[..ed25519_dalek::SECRET_KEY_LENGTH].iter_mut()
        .zip(enc_signing_key.iter().zip(pcache.hashed_update_pw.iter())) {
            *key = enc_key ^ pw;
        }
        self.compute_key_rollback(&mut keypair_bytes[..ed25519_dalek::SECRET_KEY_LENGTH]);
        #[cfg(feature = "hazardous-debug")]
        log::debug!("keypair privkey (after anti-rollback): {:x?}", &keypair_bytes[..ed25519_dalek::SECRET_KEY_LENGTH]);
        for (key, &src) in keypair_bytes[ed25519_dalek::SECRET_KEY_LENGTH..].iter_mut()
        .zip(self.read_key_256(KeyRomLocs::SELFSIGN_PUBKEY).iter()) {
            *key = src;
        }
        // Keypair zeroizes the secret key on drop.
        let keypair = Keypair::from_bytes(&keypair_bytes).ok()?;
        #[cfg(feature = "hazardous-debug")]
        log::debug!("keypair privkey (after anti-rollback + conversion): {:x?}", keypair.secret.to_bytes());

        // check if the keypair is valid by signing and verifying a short message
        let test_data = "whiskey made me do it";
        let test_sig = keypair.sign(test_data.as_bytes());
        match keypair.verify(&test_data.as_bytes(), &test_sig) {
            Ok(_) => {
                for (&src, dst) in self.read_key_256(KeyRomLocs::FPGA_KEY).iter().zip(pcache.fpga_key.iter_mut()) {
                    *dst = src;
                }
                log::debug!("fpga key (encrypted): {:x?}", &pcache.fpga_key);
                for (fkey, &pw) in pcache.fpga_key.iter_mut().zip(pcache.hashed_update_pw.iter()) {
                    *fkey = *fkey ^ pw;
                }
                pcache.fpga_key_valid = 1;
                // copy the plaintext FPGA key into the backup key structure, which implements the zeroize trait.
                let mut bkey = backups::BackupKey::default();
                bkey.0.copy_from_slice(&pcache.fpga_key);
                // copy the key rom into the backup keyrom structure, which implements the zeroize trait.
                let mut backup_rom = backups::KeyRomExport::default();
                for i in 0..256 {
                    self.keyrom.wfo(utra::keyrom::ADDRESS_ADDRESS, i);
                    backup_rom.0[i as usize] = self.keyrom.rf(utra::keyrom::DATA_DATA);
                }
                // we're done with the password now, clear all the temps
                self.purge_password(PasswordType::Update);
                for b in keypair_bytes.iter_mut() {
                    *b = 0;
                }
                self.purge_sensitive_data();
                Some((bkey, backup_rom))
            },
            Err(e) => {
                log::warn!("update password was not connect ({:?})", e);
                self.purge_password(PasswordType::Update);
                for b in keypair_bytes.iter_mut() {
                    *b = 0;
                }
                None
            }
        }
    }
    pub fn write_backup(&mut self,
        mut header: BackupHeader,
        backup_ct: backups::BackupDataCt,
        checksums: Option<Checksums>
    ) -> Result<(), xous::Error> {
        header.op = BackupOp::Backup;  // set the "we're backing up" flag

        // condense the data into a single block, to reduce read/write cycles on the block
        const HASH_LEN: usize = 32;
        let mut block = [0u8; size_of::<BackupHeader>() + size_of::<backups::BackupDataCt>() + HASH_LEN];
        block[..size_of::<BackupHeader>()].copy_from_slice(header.as_ref());
        block[size_of::<BackupHeader>()..size_of::<BackupHeader>() + size_of::<backups::BackupDataCt>()].copy_from_slice(backup_ct.as_ref());

        // compute a hash of the data, for fast verification of the backup header integrity to detect media errors, etc.
        let mut hasher = Sha512Trunc256::new_with_strategy(FallbackStrategy::HardwareThenSoftware);
        hasher.update(&block[..size_of::<BackupHeader>() + size_of::<backups::BackupDataCt>()]);
        let digest = hasher.finalize();
        assert!(digest.len() == HASH_LEN, "Wrong length hash selected! Check your code.");
        block[size_of::<BackupHeader>() + size_of::<backups::BackupDataCt>()..]
            .copy_from_slice(digest.as_slice());

        // stage the artifacts into a full page header
        const PAGE_LEN: usize = 4096;
        let mut page = [0xFFu8; PAGE_LEN];
        // place the metadata at the top of the block
        page[..block.len()].copy_from_slice(&block);
        if let Some(cs) = checksums {
            use std::ops::Deref;
            let cs_slice = cs.deref();
            // stage the checksums, if any, aligned to the end of the block
            assert!(block.len() + cs_slice.len() < PAGE_LEN, "Error: checksum block has outgrown the available space");
            page[PAGE_LEN - cs_slice.len()..].copy_from_slice(cs_slice);
        }
        self.spinor.patch(
            self.kernel(),
            self.kernel_base(),
            &page,
            xous::KERNEL_BACKUP_OFFSET
        ).map_err(|_| xous::Error::InternalError)?;
        Ok(())
    }
    pub fn write_restore_dna(&mut self, mut header: BackupHeader, backup_ct: backups::BackupDataCt) -> Result<(), xous::Error> {
        header.op = BackupOp::RestoreDna; // set the "restore DNA" flag

        // condense the data into a single block, to reduce read/write cycles on the block
        let mut block = [0u8; size_of::<BackupHeader>() + size_of::<backups::BackupDataCt>()];
        block[..size_of::<BackupHeader>()].copy_from_slice(header.as_ref());
        block[size_of::<BackupHeader>()..].copy_from_slice(backup_ct.as_ref());
        self.spinor.patch(
            self.kernel(),
            self.kernel_base(),
            &block,
            xous::KERNEL_BACKUP_OFFSET
        ).map_err(|_| xous::Error::InternalError)?;
        Ok(())
    }
    pub fn read_backup(&mut self) -> Result<(BackupHeader, backups::BackupDataCt), xous::Error> {
        let mut header = BackupHeader::default();
        let mut ct = backups::BackupDataCt::default();
        header.as_mut().copy_from_slice(
            &self.kernel()[
                xous::KERNEL_BACKUP_OFFSET as usize ..
                xous::KERNEL_BACKUP_OFFSET as usize + size_of::<BackupHeader>()
        ]);
        ct.as_mut().copy_from_slice(
        &self.kernel()[
            xous::KERNEL_BACKUP_OFFSET as usize + size_of::<BackupHeader>()..
            xous::KERNEL_BACKUP_OFFSET as usize + size_of::<BackupHeader>() + size_of::<backups::BackupDataCt>()
        ]);
        Ok((header, ct))
    }
    pub fn erase_backup(&mut self) {
        let blank = [0xffu8; size_of::<BackupHeader>() + size_of::<backups::BackupDataCt>()];
        self.spinor.patch(
            self.kernel(),
            self.kernel_base(),
            &blank,
            xous::KERNEL_BACKUP_OFFSET
        ).expect("couldn't erase backup region");
    }
}