use ticktimer_server::Ticktimer;
use utralib::generated::*;
use crate::api::*;
use core::num::NonZeroUsize;
use num_traits::*;

use crate::bcrypt::*;
use crate::PasswordType;

const BCRYPT_COST: u32 = 10;   // 10 is the minimum recommended by OWASP; takes xxx ms to verify @ 10 rounds

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

    pub fn update_policy(&mut self, policy: Option<PasswordRetentionPolicy>, pw_type: PasswordType) {
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
    }

    /// plaintext password is passed as a &str. Any copies internally are destroyed.
    pub fn hash_and_save_password(&mut self, pw: &str, pw_type: PasswordType) {
        let pw_len = if pw.len() > 72 {
            log::warn!("password of length {} is truncated to 72 bytes [bcrypt]", pw.len());
            72
        } else {
            pw.len()
        };
        let mut plaintext_copy: [u8; 72] = [0; 72];
        for (src, dst) in pw.bytes().zip(plaintext_copy.iter_mut()) {
            *dst = src;
        }
        let mut hashed_password: [u8; 24] = [0; 24];
        let salt = self.read_key_128(KeyRomLocations::PEPPER);

        let timer = ticktimer_server::Ticktimer::new().expect("couldn't connect to ticktimer");
        // this function was pulled in from another crate. A quick look around makes me think it doesn't
        // create any extra copies of the plaintext anywhere -- the &[u8] gets dereferenced and mixed up
        // in some key expansion immediately
        let start_time = timer.elapsed_ms();
        bcrypt(BCRYPT_COST, &salt, &plaintext_copy[..pw_len], &mut hashed_password);
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

        // an unsafe method is used because the compiler will correctly reason that plaintext_copy goes out of scope
        // and these writes are never read, and therefore they may be optimized out.
        let pt_ptr = plaintext_copy.as_mut_ptr();
        for i in 0..plaintext_copy.len() {
            unsafe{pt_ptr.add(i).write_volatile(core::mem::zeroed());}
        }
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
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

    pub fn try_init_keys(&mut self, maybe_progress: Option<xous::SID>) {
        let mut step = 0; // current step
        let total = 10; // total number of steps

        { // this block checks if we're already initialized, and aborts if we are
            let progress_cid = if let Some(progress_sid) = maybe_progress {
                Some(xous::connect(progress_sid).expect("couldn't connect to originator for progress updates"))
            } else {
                None
            };
            // first progress update: we could make a progress update
            step += 1;
            self.update_progress(progress_cid, step, total, false);

            self.keyrom.wfo(utra::keyrom::ADDRESS_ADDRESS, KeyRomLocations::CONFIG as u32);
            let config = self.keyrom.rf(utra::keyrom::DATA_DATA);

            if config & keyrom_config::INITIALIZED.ms(1) != 0 {
                self.update_progress(progress_cid, step, total, true);
                // don't re-initialize an initialized KEYROM
                return;
            }
        }

        // block suspend/resume ops during security-sensitive operations
        self.susres.set_suspendable(false).expect("couldn't block suspend/resume");

        { // in this block, keyrom data is copied into RAM.
            // make a copy of the KEYROM to hold the new mods, in the sensitive data area
            let sensitive_slice = self.sensitive_data.as_slice_mut::<u32>();
            for addr in 0..256 {
                self.keyrom.wfo(utra::keyrom::ADDRESS_ADDRESS, addr);
                sensitive_slice[addr as usize] = self.keyrom.rf(utra::keyrom::DATA_DATA);
            }

            // the following keys should be provisioned:
            // - self-signing private key
            // - self-signing public key
            // - user root key
            // - pepper

            // provision the pepper
            for keyword in sensitive_slice[KeyRomLocations::PEPPER as usize..(KeyRomLocations::PEPPER + 4) as usize].iter_mut() {
                *keyword = self.trng.get_u32().expect("couldn't get random number");
            }

            // prompt the user for a password

            // zeroize the RAM-backed data
            for data in sensitive_slice.iter_mut() {
                *data = 0;
            }
        }

        // re-allow suspend/resume ops
        self.susres.set_suspendable(true).expect("couldn't re-allow suspend/resume");
    }
}
