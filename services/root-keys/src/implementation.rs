use gam::FONT_TOTAL_LEN;
use ticktimer_server::Ticktimer;
use utralib::generated::*;
use crate::api::*;
use core::num::NonZeroUsize;
use num_traits::*;

use gam::modal::{Modal, Slider};

use crate::bcrypt::*;
use crate::PasswordType;

use core::convert::TryInto;
use ed25519_dalek::{Keypair, Signature, Signer};
use engine_sha512::*;
use digest::Digest;
use graphics_server::BulkRead;
use core::mem::size_of;

// TODO: add hardware acceleration for BCRYPT so we can hit the OWASP target without excessive UX delay
const BCRYPT_COST: u32 = 7;   // 10 is the minimum recommended by OWASP; takes 5696 ms to verify @ 10 rounds; 804 ms to verify 7 rounds

/// Size of the total area allocated for signatures. It is equal to the size of one FLASH sector, which is the smallest
/// increment that can be erased.
const SIGBLOCK_SIZE: u32 = 0x1000;

#[repr(C)]
struct SignatureInFlash {
    pub version: u32,
    pub signed_len: u32,
    pub signature: [u8; 64],
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
}

pub(crate) struct RootKeys {
    keyrom: utralib::CSR<u32>,
    gateware: xous::MemoryRange,
    staging: xous::MemoryRange,
    loader_code: xous::MemoryRange,
    kernel: xous::MemoryRange,
    /// regions of RAM that holds all plaintext passwords, keys, and temp data. stuck in two well-defined page so we can
    /// zero-ize it upon demand, without guessing about stack frames and/or Rust optimizers removing writes
    sensitive_data: xous::MemoryRange, // this gets purged at least on every suspend, but ideally purged sooner than that
    pass_cache: xous::MemoryRange,  // this can be purged based on a policy, as set below
    boot_password_policy: PasswordRetentionPolicy,
    update_password_policy: PasswordRetentionPolicy,
    cur_password_type: Option<PasswordType>, // for tracking which password we're dealing with at the UX layer
    susres: susres::Susres, // for disabling suspend/resume
    trng: trng::Trng,
    gam: gam::Gam, // for raising UX elements directly
    gfx: graphics_server::Gfx, // for reading out font planes for signing verification
}

impl RootKeys {
    pub fn new(xns: &xous_names::XousNames) -> RootKeys {
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

        let sensitive_data = xous::syscall::map_memory(
            None,
            None,
            0x1000,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        ).expect("couldn't map sensitive data page");
        let pass_cache = xous::syscall::map_memory(
            None,
            None,
            0x1000,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        ).expect("couldn't map sensitive data page");

        let keys = RootKeys {
            keyrom: CSR::new(keyrom.as_mut_ptr() as *mut u32),
            gateware,
            staging,
            loader_code,
            kernel,
            sensitive_data,
            pass_cache,
            update_password_policy: PasswordRetentionPolicy::AlwaysPurge,
            boot_password_policy: PasswordRetentionPolicy::AlwaysKeep,
            cur_password_type: None,
            susres: susres::Susres::new_without_hook(&xns).expect("couldn't connect to susres without hook"),
            trng: trng::Trng::new(&xns).expect("couldn't connect to TRNG server"),
            gam: gam::Gam::new(&xns).expect("couldn't connect to GAM"),
            gfx: graphics_server::Gfx::new(&xns).expect("couldn't connect to gfx"),
        };

        keys
    }

    fn purge_password(&mut self, pw_type: PasswordType) {
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
                }
            }
        }
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
    fn purge_sensitive_data(&mut self) {
        let data = self.sensitive_data.as_slice_mut::<u32>();
        for d in data.iter_mut() {
            *d = 0;
        }
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
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
    pub fn hash_and_save_password(&mut self, pw: &str) {
        let pw_type = if let Some(cur_type) = self.cur_password_type {
            cur_type
        } else {
            log::error!("got an unexpected password from the UX");
            return;
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
        let mut hasher = engine_sha512::Sha512Trunc256::new(Some(engine_sha512::FallbackStrategy::SoftwareOnly));
        hasher.update(hashed_password);
        let digest = hasher.finalize();

        let pcache_ptr: *mut PasswordCache = self.pass_cache.as_mut_ptr() as *mut PasswordCache;
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

    /// Returns the `salt` needed for the `bcrypt` routine.
    /// This routine handles the special-case of being unitialized: in that case, we need to get
    /// salt from a staging area, and not our KEYROM. However, `setup_key_init` must be called
    /// first to ensure that the staging area has a valid salt.
    fn get_salt(&mut self) -> [u8; 16] {
        if !self.is_initialized() {
            // we're not initialized, use the salt that should already be in the staging area
            let sensitive_slice = self.sensitive_data.as_slice::<u32>();
            let mut key: [u8; 16] = [0; 16];
            for (word, &keyword) in key.chunks_mut(4).into_iter()
                                                    .zip(sensitive_slice[KeyRomLocs::PEPPER as usize..KeyRomLocs::PEPPER as usize + 128/(size_of::<u32>()*8)].iter()) {
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

    /// Called by the UX layer to set up a key init run. It disables suspend/resume for the duration
    /// of the run, and also sets up some missing fields of KEYROM necessary to encrypt passwords.
    pub fn setup_key_init(&mut self) {
        // block suspend/resume ops during security-sensitive operations
        self.susres.set_suspendable(false).expect("couldn't block suspend/resume");
        // in this block, keyrom data is copied into RAM.
        // make a copy of the KEYROM to hold the new mods, in the sensitive data area
        let sensitive_slice = self.sensitive_data.as_slice_mut::<u32>();
        for addr in 0..256 {
            self.keyrom.wfo(utra::keyrom::ADDRESS_ADDRESS, addr);
            sensitive_slice[addr as usize] = self.keyrom.rf(utra::keyrom::DATA_DATA);
        }

        // provision the pepper
        for keyword in sensitive_slice[KeyRomLocs::PEPPER as usize..KeyRomLocs::PEPPER as usize + 128/(size_of::<u32>()*8)].iter_mut() {
            *keyword = self.trng.get_u32().expect("couldn't get random number");
        }
    }

    /// Core of the key initialization routine. Requires a `progress_modal` dialog box that has been set
    /// up with the appropriate notification messages by the UX layer, and a `Slider` type action which
    /// is used to report the progress of the initialization routine. We assume the `Slider` box is set
    /// up to report progress on a range of 0-100%.
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
    ///   - returns true if we should reboot (everything succeeded)
    ///   - returns false if we had an error condition (don't reboot)
    pub fn do_key_init(&mut self, progress_modal: &mut Modal, progress_action: &mut Slider) -> bool {
        // kick the progress bar to indicate we've entered the routine
        update_progress(1, progress_modal, progress_action);

        let keypair: Keypair = Keypair::generate(&mut self.trng);
        // pub key is easy, no need to encrypt
        let public_key: [u8; ed25519_dalek::PUBLIC_KEY_LENGTH] = keypair.public.to_bytes();
        { // scope sensitive_slice narrowly, as it borrows *self mutably, and can mess up later calls that borrow an immutable self
            // sensitive_slice is our staging area for the new keyrom contents
            let sensitive_slice = self.sensitive_data.as_slice_mut::<u32>();
            for (src, dst) in public_key.chunks(4).into_iter()
                .zip(sensitive_slice[KeyRomLocs::SELFSIGN_PUBKEY as usize..KeyRomLocs::SELFSIGN_PUBKEY as usize + 256/(size_of::<u32>()*8)].iter_mut()) {
                *dst = u32::from_be_bytes(src.try_into().unwrap())
            }
        }

        // extract the update password key from the cache, and apply it to the private key
        let pcache: &PasswordCache = unsafe{&*(self.pass_cache.as_mut_ptr() as *mut PasswordCache)};
        log::info!("cached boot passwords {:x?}", pcache.hashed_boot_pw);
        log::info!("cached update password: {:x?}", pcache.hashed_update_pw);
        // private key must XOR with password before storing
        let mut private_key_enc: [u8; ed25519_dalek::SECRET_KEY_LENGTH] = [0; ed25519_dalek::SECRET_KEY_LENGTH];
        // we do this from to try and avoid making as few copies of the hashed password as possible
        for (dst, (plain, key)) in
            private_key_enc.iter_mut()
            .zip(keypair.secret.to_bytes().iter()
            .zip(pcache.hashed_update_pw.iter())) {
                *dst = plain ^ key;
        }

        // store the private key to the keyrom staging area
        {
            let sensitive_slice = self.sensitive_data.as_slice_mut::<u32>();
            for (src, dst) in private_key_enc.chunks(4).into_iter()
                .zip(sensitive_slice[KeyRomLocs::SELFSIGN_PRIVKEY as usize..KeyRomLocs::SELFSIGN_PRIVKEY as usize + 256/(size_of::<u32>()*8)].iter_mut()) {
                *dst = u32::from_be_bytes(src.try_into().unwrap())
            }
        }

        update_progress(10, progress_modal, progress_action);

        // generate and store a root key (aka boot key), this is what is unlocked by the "boot password"
        // ironically, it's a "lower security" key because it just acts as a gatekeeper to further
        // keys that would have a stronger password applied to them, based upon the importance of the secret
        // think of this more as a user PIN login confirmation, than as a significant cryptographic event
        let mut boot_key_enc: [u8; 32] = [0; 32];
        for (dst, key) in
            boot_key_enc.chunks_mut(4).into_iter()
            .zip(pcache.hashed_boot_pw.chunks(4).into_iter()) {
                let key_word = self.trng.get_u32().unwrap().to_be_bytes();
                // just unroll this loop, it's fast and easy enough
                (*dst)[0] = key[0] ^ key_word[0];
                (*dst)[1] = key[1] ^ key_word[1];
                (*dst)[2] = key[2] ^ key_word[2];
                (*dst)[3] = key[3] ^ key_word[3];
                // also note that interestingly, we don't have to XOR it with the hashed boot password --
                // this key isn't used by this routine, just initialized, so really, it only matters to
                // XOR it with the password when you use it the first time to encrypt something.
        }

        // store the boot key to the keyrom staging area
        {
            let sensitive_slice = self.sensitive_data.as_slice_mut::<u32>();
            for (src, dst) in private_key_enc.chunks(4).into_iter()
                .zip(sensitive_slice[KeyRomLocs::USER_KEY as usize..KeyRomLocs::USER_KEY as usize + 256/(size_of::<u32>()*8)].iter_mut()) {
                *dst = u32::from_be_bytes(src.try_into().unwrap())
            }
        }

        update_progress(20, progress_modal, progress_action);

        // sign the loader
        let (loader_sig, loader_len) = self.sign_loader(&keypair);
        update_progress(30, progress_modal, progress_action);

        // sign the kernel
        let (kernel_sig, kernel_len) = self.sign_kernel(&keypair);
        update_progress(40, progress_modal, progress_action);

        // encrypt the FPGA key using the update password. in an un-init system, it is provided to us in plaintext format
        // e.g. in the case that we're doing a BBRAM boot (eFuse flow would give us a 0's key and we'd later on set it)
        self.debug_print_key(KeyRomLocs::FPGA_KEY as usize, 256, "FPGA key before encryption: ");
        {
            let sensitive_slice = self.sensitive_data.as_slice_mut::<u32>();
            for (word, key_word) in sensitive_slice[KeyRomLocs::FPGA_KEY as usize..KeyRomLocs::FPGA_KEY as usize + 256/(size_of::<u32>()*8)].iter_mut()
                .zip(pcache.hashed_update_pw.chunks(4).into_iter()) {
                *word = *word ^ u32::from_be_bytes(key_word.try_into().unwrap());
            }
        }
        update_progress(50, progress_modal, progress_action);

        // set the "init" bit in the staging area
        {
            let sensitive_slice = self.sensitive_data.as_slice_mut::<u32>();
            self.keyrom.wfo(utra::keyrom::ADDRESS_ADDRESS, KeyRomLocs::CONFIG as u32);
            let config = self.keyrom.rf(utra::keyrom::DATA_DATA);
            log::info!("config on device {:x}", config);
            log::info!("cached copy before mod {:x}", sensitive_slice[KeyRomLocs::CONFIG as usize]);
            log::info!("mask shift computation: {:x}", keyrom_config::INITIALIZED.ms(1));
            sensitive_slice[KeyRomLocs::CONFIG as usize] |= keyrom_config::INITIALIZED.ms(1);
            log::info!("cached copy after mod {:x}", sensitive_slice[KeyRomLocs::CONFIG as usize]);
        }
        update_progress(60, progress_modal, progress_action);

        log::info!("Self private key: {:x?}", keypair.secret.to_bytes());
        log::info!("Self public key: {:x?}", keypair.public.to_bytes());
        self.debug_staging();
        // compute the keyrom patch set for the bitstream
        // at this point the KEYROM as replicated in sensitive_slice should have all its assets in place


        // finalize the progress bar on exit -- always leave at 100%
        update_progress(100, progress_modal, progress_action);
        true
    }

    fn debug_staging(&self) {
        self.debug_print_key(KeyRomLocs::FPGA_KEY as usize, 256, "FPGA key: ");
        self.debug_print_key(KeyRomLocs::SELFSIGN_PRIVKEY as usize, 256, "Self private key: ");
        self.debug_print_key(KeyRomLocs::SELFSIGN_PUBKEY as usize, 256, "Self public key: ");
        self.debug_print_key(KeyRomLocs::DEVELOPER_PUBKEY as usize, 256, "Dev public key: ");
        self.debug_print_key(KeyRomLocs::THIRDPARTY_PUBKEY as usize, 256, "3rd party public key: ");
        self.debug_print_key(KeyRomLocs::USER_KEY as usize, 256, "Boot key: ");
        self.debug_print_key(KeyRomLocs::PEPPER as usize, 128, "Pepper: ");
        self.debug_print_key(KeyRomLocs::CONFIG as usize, 32, "Config (as BE): ");
    }
    fn debug_print_key(&self, offset: usize, num_bits: usize, name: &str) {
        use core::fmt::Write;
        let mut debugstr = xous_ipc::String::<4096>::new();
        let sensitive_slice = self.sensitive_data.as_slice_mut::<u32>();
        write!(debugstr, "{}", name).unwrap();
        for word in sensitive_slice[offset .. offset as usize + num_bits/(size_of::<u32>()*8)].iter() {
            for byte in word.to_be_bytes().iter() {
                write!(debugstr, "{:02x}", byte).unwrap();
            }
        }
        log::info!("{}", debugstr);
    }

    pub fn sign_loader(&mut self, signing_key: &Keypair) -> (Signature, u32) {
        let loader_len =
            xous::LOADER_CODE_LEN
            - SIGBLOCK_SIZE
            + graphics_server::fontmap::FONT_TOTAL_LEN as u32
            + 8; // two u32 words are appended to the end, which repeat the "version" and "length" fields encoded in the signature block

        // this is a huge hash, so, get a hardware hasher, even if it means waiting for it
        let mut hasher = engine_sha512::Sha512::new(Some(engine_sha512::FallbackStrategy::WaitForHardware));
        let loader_region = self.loader_code.as_slice::<u8>();
        // the loader data starts one page in; the first page is reserved for the signature itself
        hasher.update(&loader_region[SIGBLOCK_SIZE as usize..]);

        // now get the font plane data
        self.gfx.bulk_read_restart(); // reset the bulk read pointers on the gfx side
        let mut bulkread = BulkRead::default();
        let mut buf = xous_ipc::Buffer::into_buf(bulkread).expect("couldn't transform bulkread into aligned buffer");
        // this form of loop was chosen to avoid the multiple re-initializations and copies that would be entailed
        // in our usual idiom for pasing buffers around. instead, we create a single buffer, and re-use it for
        // every iteration of the loop.
        loop {
            buf.lend_mut(self.gfx.conn(), self.gfx.bulk_read_fontmap_op()).expect("couldn't do bulkread from gfx");
            let br = buf.as_flat::<BulkRead, _>().unwrap();
            hasher.update(&br.buf[..br.len as usize]);
            if br.len != bulkread.buf.len() as u32 {
                log::info!("non-full block len: {}", br.len);
            }
            if br.len < bulkread.buf.len() as u32 {
                // read until we get a buffer that's not fully filled
                break;
            }
        }
        let digest = hasher.finalize();
        if false {
            log::info!("len: {}", loader_len);
            log::info!("{:x?}", digest);
            // fake hasher for now
            let mut hasher = engine_sha512::Sha512::new(Some(engine_sha512::FallbackStrategy::WaitForHardware));
            hasher.update(&loader_region[SIGBLOCK_SIZE as usize..]);
        }
        (signing_key.sign_prehashed(hasher, None).expect("couldn't sign the loader"), loader_len)
    }

    pub fn sign_kernel(&mut self, signing_key: &Keypair) -> (Signature, u32) {
        let mut hasher = engine_sha512::Sha512::new(Some(engine_sha512::FallbackStrategy::WaitForHardware));
        let kernel_region = self.kernel.as_slice::<u8>();
        // for the kernel length, we can't know/trust the given length in the signature field, so we sign the entire
        // length of the region. This will increase the time it takes to verify; however, at the current trend, we'll probably
        // use most of the available space for the kernel, so by the time we're done maybe only 10-20% of the space is empty.
        let kernel_len = kernel_region.len() - SIGBLOCK_SIZE as usize;
        hasher.update(&kernel_region[SIGBLOCK_SIZE as usize ..]);

        (signing_key.sign_prehashed(hasher, None).expect("couldn't sign the kernel"), kernel_len as u32)
    }

    /// Called by the UX layer at the epilogue of the initialization run. Allows suspend/resume to resume,
    /// and zero-izes any sensitive data that was created in the process.
    pub fn finish_key_init(&mut self) {
        let sensitive_slice = self.sensitive_data.as_slice_mut::<u32>();
        // zeroize the RAM-backed data
        for data in sensitive_slice.iter_mut() {
            *data = 0;
        }
        // re-allow suspend/resume ops
        self.susres.set_suspendable(true).expect("couldn't re-allow suspend/resume");
    }
}

fn update_progress(new_state: u32, progress_modal: &mut Modal, progress_action: &mut Slider) {
    log::info!("progress: {}", new_state);
    progress_action.set_state(new_state);
    progress_modal.modify(
        Some(gam::modal::ActionType::Slider(*progress_action)),
        None, false, None, false, None);
    progress_modal.redraw(); // stage the modal box pixels to the back buffer
    progress_modal.gam.redraw().expect("couldn't cause back buffer to be sent to the screen");
    xous::yield_slice(); // this gives time for the GAM to do the sending
}
