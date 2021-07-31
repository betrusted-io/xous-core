use ticktimer_server::Ticktimer;
use utralib::generated::*;
use crate::api::*;
use core::num::NonZeroUsize;
use num_traits::*;

use crate::bcrypt::*;
use crate::PasswordType;

// TODO: add hardware acceleration for BCRYPT so we can hit the OWASP target without excessive UX delay
const BCRYPT_COST: u32 = 7;   // 10 is the minimum recommended by OWASP; takes 5696 ms to verify @ 10 rounds; 804 ms to verify 7 rounds

struct KeyRomLocations {}
#[allow(dead_code)]
impl KeyRomLocations {
    const FPGA_KEY:            u8 = 0x00;
    const SELFSIGN_PRIVKEY:    u8 = 0x08;
    const SELFSIGN_PUBKEY:     u8 = 0x10;
    const DEVELOPER_PUBKEY:    u8 = 0x18;
    const THIRDPARTY_PUBKEY:   u8 = 0x20;
    const USER_KEY:   u8 = 0x28;
    const PEPPER:     u8 = 0xf8;
    const FPGA_REV:   u8 = 0xfc;
    const LOADER_REV: u8 = 0xfd;
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
        (value & self.mask) << self.offset
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
    pub const INITIALIZED:         KeyField = KeyField::new(4, 27);
}

#[repr(C)]
struct PasswordCache {
    // this structure is mapped into the password cache page and can be zero-ized at any time
    // we avoid using fancy Rust structures because everything has to "make sense" after a forced zero-ization
    hashed_boot_pw: [u8; 24],
    hashed_boot_pw_valid: u32, // non-zero for valid
    hashed_update_pw: [u8; 24],
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

    /// plaintext password is passed as a &str. Any copies internally are destroyed. Caller is responsible for destroying the &str original.
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

        let pcache_ptr: *mut PasswordCache = self.pass_cache.as_mut_ptr() as *mut PasswordCache;
        unsafe {
            match pw_type {
                PasswordType::Boot => {
                    for (&src, dst) in pw.as_bytes().iter().zip((*pcache_ptr).hashed_boot_pw.iter_mut()) {
                        *dst = src;
                    }
                    (*pcache_ptr).hashed_boot_pw_valid = 1;
                }
                PasswordType::Update => {
                    for (&src, dst) in pw.as_bytes().iter().zip((*pcache_ptr).hashed_update_pw.iter_mut()) {
                        *dst = src;
                    }
                    (*pcache_ptr).hashed_update_pw_valid = 1;
                }
            }
        }
    }

    fn update_progress(&mut self, maybe_cid: Option<xous::CID>, current_step: u32, total_steps: u32, finished: bool) {
        if let Some(cid) = maybe_cid {
            xous::send_message(cid, xous::Message::new_scalar(ProgressCallback::Update.to_usize().unwrap(),
                current_step as usize, total_steps as usize, if finished {1} else {0}, 0)
            ).expect("couldn't send a progress report message");
        }
    }

    // reads a 256-bit key at a given index offset
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
    // this routine handles the special-case of being unitialized: in that case, we need to get
    // salt from a staging area, and not our KEYROM
    fn get_salt(&mut self) -> [u8; 16] {
        if !self.is_initialized() {
            // we're not initialized, use the salt that should already be in the staging area
            let sensitive_slice = self.sensitive_data.as_slice::<u32>();
            let mut key: [u8; 16] = [0; 16];
            for (word, &keyword) in key.chunks_mut(4).into_iter()
                                                    .zip(sensitive_slice[KeyRomLocations::PEPPER as usize..(KeyRomLocations::PEPPER + 4) as usize].iter()) {
                for (&byte, dst) in keyword.to_be_bytes().iter().zip(word.iter_mut()) {
                    *dst = byte;
                }
            }
            key
        } else {
            self.read_key_128(KeyRomLocations::PEPPER)
        }
    }

    pub fn set_ux_password_type(&mut self, cur_type: Option<PasswordType>) {
        self.cur_password_type = cur_type;
    }
    pub fn get_ux_password_type(&self) -> Option<PasswordType> {self.cur_password_type}

    pub fn is_initialized(&mut self) -> bool {
        self.keyrom.wfo(utra::keyrom::ADDRESS_ADDRESS, KeyRomLocations::CONFIG as u32);
        let config = self.keyrom.rf(utra::keyrom::DATA_DATA);
        if config & keyrom_config::INITIALIZED.ms(1) != 0 {
            true
        } else {
            false
        }
    }

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
        for keyword in sensitive_slice[KeyRomLocations::PEPPER as usize..(KeyRomLocations::PEPPER + 4) as usize].iter_mut() {
            *keyword = self.trng.get_u32().expect("couldn't get random number");
        }
    }

    pub fn do_key_init(&mut self) {
        // here:
        // - generate signing private key (encrypted with update password)
        // - generate rootkey (encrypted with boot password)
        // - generate signing public key
        // - set the init bit
        // - sign the loader
        // - sign the kernel
        // - compute the patch set for the FPGA bitstream
        // - do the patch (whatever that means - gotta deal with the AES key, HMAC etc.)
        // - verify the FPGA image hmac
        // - sign the FPGA image
        // - get ready for a reboot
    }

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
