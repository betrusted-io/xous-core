use utralib::generated::*;
use crate::api::*;
use log::info;
use core::num::NonZeroUsize;
use num_traits::*;

const KEYROMADR_FPGA_KEY:            u32 = 0x00;
const KEYROMADR_SELFSIGN_PRIVKEY:    u32 = 0x08;
const KEYROMADR_SELFSIGN_PUBKEY:     u32 = 0x10;
const KEYROMADR_DEVELOPER_PUBKEY:    u32 = 0x18;
const KEYROMADR_THIRDPARTY_PUBKEY:   u32 = 0x20;
const KEYROMADR_USER_KEY:   u32 = 0x28;
const KEYROMADR_PEPPER:     u32 = 0xf8;
const KEYROMADR_FPGA_REV:   u32 = 0xfc;
const KEYROMADR_LOADER_REV: u32 = 0xfd;
const KERYOMADR_CONFIG:     u32 = 0xff;

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
    canary: u32, // this is 0 if the page was cleared; it is set immediately upon allocation/initialization/update
}

pub(crate) struct RootKeys {
    keyrom: utralib::CSR<u32>,
    gateware: xous::MemoryRange,
    staging: xous::MemoryRange,
    loader_code: xous::MemoryRange,
    kernel: xous::MemoryRange,
    /// regions of RAM that holds all plaintext passwords, keys, and temp data. stuck in two well-defined page so we can
    /// zero-ize it upon demand, without guessing about stack frames and/or Rust optimizers removing writes
    sensitive_data: xous::MemoryRange,
    pass_cache: xous::MemoryRange,
    password_policy: PasswordRetentionPolicy,
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

        let mut keys = RootKeys {
            keyrom: CSR::new(keyrom.as_mut_ptr() as *mut u32),
            gateware,
            staging,
            loader_code,
            kernel,
            sensitive_data,
            pass_cache,
            password_policy: PasswordRetentionPolicy::AlwaysKeep,
            susres: susres::Susres::new_without_hook(&xns).expect("couldn't connect to susres without hook"),
            trng: trng::Trng::new(&xns).expect("couldn't connect to TRNG server"),
            gam: gam::Gam::new(&xns).expect("couldn't connect to GAM"),
        };

        keys
    }

    pub fn suspend(&mut self) {
        match self.password_policy {
            PasswordRetentionPolicy::AlwaysKeep => {
                ()
            },
            _ => {
                let data = self.sensitive_data.as_slice_mut::<u32>();
                for d in data.iter_mut() {
                    *d = 0;
                }
            }
        }
    }
    pub fn resume(&mut self) {
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

            self.keyrom.wfo(utra::keyrom::ADDRESS_ADDRESS, KERYOMADR_CONFIG);
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
            for keyword in sensitive_slice[KEYROMADR_PEPPER as usize..(KEYROMADR_PEPPER + 8) as usize].iter_mut() {
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

    pub fn test_ux(&mut self, arg: usize) {
        match arg {
            0 => self.gam.raise_modal(ROOTKEY_MODAL_NAME).expect("couldn't raise modal"),
            1 => self.gam.relinquish_focus().expect("couldn't hide modal"),
            _ => log::info!("test_ux got unrecognized arg: {}", arg),
        };
    }
}
