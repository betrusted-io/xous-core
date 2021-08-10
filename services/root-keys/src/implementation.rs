use utralib::generated::*;
use crate::api::*;
use core::num::NonZeroUsize;
use num_traits::*;

use gam::modal::{Modal, Slider, ProgressBar, ActionType};
use locales::t;

use crate::bcrypt::*;
use crate::api::PasswordType;

use core::convert::TryInto;
use ed25519_dalek::{Keypair, Signature, PublicKey, Signer, ExpandedSecretKey};
use curve25519_dalek::scalar::Scalar;
use curve25519_dalek::constants;
use sha2::{FallbackStrategy, Sha256, Sha512, Sha512Trunc256};
use digest::Digest;
use graphics_server::BulkRead;
use core::mem::size_of;

use aes_xous::{Aes256, NewBlockCipher, BlockDecrypt, BlockEncrypt};
use cipher::generic_array::GenericArray;

use root_keys::key2bits::*;

// TODO: add hardware acceleration for BCRYPT so we can hit the OWASP target without excessive UX delay
const BCRYPT_COST: u32 = 7;   // 10 is the minimum recommended by OWASP; takes 5696 ms to verify @ 10 rounds; 804 ms to verify 7 rounds

/// Size of the total area allocated for signatures. It is equal to the size of one FLASH sector, which is the smallest
/// increment that can be erased.
const SIGBLOCK_SIZE: u32 = 0x1000;
/// location of the csr.csv that's appended on gateware images, used for USB updates.
const CSR_CSV_OFFSET: usize = 0x27_8000;

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
    fpga_key: [u8; 32],
    fpga_key_valid: u32,
}

/// helper routine that will reverse the order of bits. uses a divide-and-conquer approach.
fn bitflip(input: &[u8], output: &mut [u8]) {
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

const AES_BLOCKSIZE: usize = 16;
/// This structure encapsulates the tools necessary to create an Oracle that can go from
/// the encrypted bitstream to plaintext and back again, based on the position in the bitstream.
/// It is a partial re-implementation of the Cbc crate from block-ciphers, and the reason we're
/// not just using the stock Cbc crate is that it doesn't seem to support restarting the Cbc
/// stream from an arbitrary position.
struct BitstreamOracle<'a> {
    bitstream: &'a [u8],
    base: u32,
    dec_cipher: Aes256,
    enc_cipher: Aes256,
    iv: [u8; AES_BLOCKSIZE],
    /// subslice of the bitstream that contains just the encrypted area of the bitstream
    ciphertext: &'a [u8],
    ct_absolute_offset: usize,
    type2_count: u32,
    /// start of type2 data as an absolute offset in the overall bitstream
    type2_absolute_offset: usize,
    /// start of type2 data as an offset relative to the start of ciphertext
    type2_ciphertext_offset: usize,
    /// length of the undifferentiated plaintext header -- this is right up to the IV specifier
    pt_header_len: usize,
}
impl<'a> BitstreamOracle<'a> {
    /// oracle supports separate encryption and decryption keys, so that it may
    /// be used for re-encryption of bitstreams to a new key. If using it for patching,
    /// set the keys to the same value.
    pub fn new(dec_key: &'a[u8], enc_key: &'a[u8], bitstream: &'a[u8], base: u32) -> Result<BitstreamOracle<'a>, RootkeyResult> {
        let mut position: usize = 0;

        // Search through the bitstream for the key words that define the IV and ciphertext length.
        // This is done so that if extra headers are added or modified, the code doesn't break (versus coding in static offsets).
        let mut iv_pos = 0;
        let mut cwd = 0;
        while position < bitstream.len() {
            cwd = u32::from_be_bytes(bitstream[position..position+4].try_into().unwrap());
            if cwd == 0x3001_6004 {
                iv_pos = position + 4
            }
            if cwd == 0x3003_4001 {
                break;
            }
            position += 1;
        }

        let position = position + 4;
        let ciphertext_len = 4 * u32::from_be_bytes(bitstream[position..position+4].try_into().unwrap());
        let ciphertext_start = position + 4;
        if ciphertext_start & (AES_BLOCKSIZE - 1) != 0 {
            log::error!("Padding is incorrect on the bitstream. Check append_csr.py for padding and make sure you are burning gateware with --raw-binary, and not --bitstream as the latter strips padding from the top of the file.");
            return Err(RootkeyResult::AlignmentError)
        }
        log::debug!("ciphertext len: {} bytes, start: 0x{:08x}", ciphertext_len, ciphertext_start);
        let ciphertext = &bitstream[ciphertext_start..ciphertext_start + ciphertext_len as usize];

        let mut iv_bytes: [u8; AES_BLOCKSIZE] = [0; AES_BLOCKSIZE];
        bitflip(&bitstream[iv_pos..iv_pos + AES_BLOCKSIZE], &mut iv_bytes);
        log::debug!("recovered iv (pre-flip): {:x?}", &bitstream[iv_pos..iv_pos + AES_BLOCKSIZE]);
        log::debug!("recovered iv           : {:x?}", &iv_bytes);

        let dec_cipher = Aes256::new(dec_key.try_into().unwrap());
        let enc_cipher = Aes256::new(enc_key.try_into().unwrap());

        let mut oracle = BitstreamOracle {
            bitstream,
            base,
            dec_cipher,
            enc_cipher,
            iv: iv_bytes,
            ciphertext,
            ct_absolute_offset: ciphertext_start,
            type2_count: 0,
            type2_absolute_offset: 0,
            type2_ciphertext_offset: 0,
            pt_header_len: iv_pos, // plaintext header goes all the way up to the IV
        };

        // search forward for the "type 2" region in the first kilobyte of plaintext
        // type2 is where regular dataframes start, and we can compute offsets for patching
        // based on this.
        let mut first_block: [u8; 1024] = [0; 1024];
        oracle.decrypt(0, &mut first_block);

        // check that the plaintext header has a well-known sequence in it -- sanity check the AES key
        let mut pass = true;
        for &b in first_block[32..64].iter() {
            if b != 0x6C {
                pass = false;
            }
        }
        if !pass {
            log::error!("hmac_header did not decrypt correctly: {:x?}", &first_block[..64]);
            return Err(RootkeyResult::KeyError)
        }

        // start searching for the commands *after* the IV and well-known sequence
        let mut pt_pos = 64;
        let mut flipword: [u8; 4] = [0; 4];
        for word in first_block[64..].chunks_exact(4).into_iter() {
            bitflip(&word[0..4], &mut flipword);
            cwd = u32::from_be_bytes(flipword);
            pt_pos += 4;
            if (cwd & 0xE000_0000) == 0x4000_0000 {
                break;
            }
        }
        oracle.type2_count = cwd & 0x3FF_FFFF;
        if pt_pos > 1000 { // pt_pos is usually found within the first 200 bytes of a bitstream, should definitely be in the first 1k or else shenanigans
            log::error!("type 2 region not found in the expected region, is the FPGA key correct?");
            return Err(RootkeyResult::KeyError)
        }
        oracle.type2_absolute_offset = pt_pos + ciphertext_start;
        oracle.type2_ciphertext_offset = pt_pos;
        log::debug!("type2 absolute: {}, relative to ct start: {}", oracle.type2_absolute_offset, oracle.type2_ciphertext_offset);

        Ok(oracle)
    }
    pub fn pt_header_len(&self) -> usize {self.pt_header_len as usize}
    pub fn base(&self) -> u32 {self.base}
    pub fn ciphertext_offset(&self) -> usize {
        self.ct_absolute_offset as usize
    }
    pub fn ciphertext(&self) -> &[u8] {
        self.ciphertext
    }
    pub fn ciphertext_offset_to_frame(&self, offset: usize) -> (usize, usize) {
        let type2_offset = offset - self.type2_ciphertext_offset;

        let frame = type2_offset / (101 * 4);
        let frame_offset = type2_offset - (frame * 101 * 4);
        (frame, frame_offset / 4)
    }
    pub fn clear(&mut self) {
        self.enc_cipher.clear();
        self.dec_cipher.clear();

        for b in self.iv.iter_mut() {
            *b = 0;
        }
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
    /// Decrypts a portion of the bitstream starting at "from", of length output.
    /// Returns the actual number of bytes processed.
    pub fn decrypt(&self, from: usize, output: &mut [u8]) -> usize {
        assert!(from & (AES_BLOCKSIZE - 1) == 0); // all requests must be an even multiple of an AES block size

        let mut index = from;
        let mut temp_block = [0; AES_BLOCKSIZE];
        let mut chain: [u8; AES_BLOCKSIZE] = [0; AES_BLOCKSIZE];
        let mut bytes_processed = 0;
        for block in output.chunks_mut(AES_BLOCKSIZE).into_iter() {
            if index > self.ciphertext_len() - AES_BLOCKSIZE {
                return bytes_processed;
            }
            if index ==  0 {
                chain = self.iv;
            } else {
                bitflip(&self.ciphertext[index - AES_BLOCKSIZE..index], &mut chain);
            };
            // copy the ciphertext into the temp_block, with a bitflip
            bitflip(&self.ciphertext[index..index + AES_BLOCKSIZE], &mut temp_block);

            // replaces the ciphertext with "plaintext"
            let mut d = GenericArray::clone_from_slice(&mut temp_block);
            self.dec_cipher.decrypt_block(&mut d);
            for (&src, dst) in d.iter().zip(temp_block.iter_mut()) {
                *dst = src;
            }

            // now XOR against the IV into the final output block. We use a "temp" block so we
            // guarantee an even block size for AES, but in fact, the output block does not
            // have to be an even multiple of our block size!
            for (dst, (&src, &iv)) in block.iter_mut().zip(temp_block.iter().zip(chain.iter())) {
                *dst = src ^ iv;
            }
            index += AES_BLOCKSIZE;
            bytes_processed += AES_BLOCKSIZE;
        }
        bytes_processed
    }
    /// Encrypts a portion of a bitstream starting at "from"; manages the IV lookup for the encryption process
    ///
    /// "from" is relative to ciphertext start, and chosen to match the "from" of a decrypt operation.
    /// When using this function to encrypt the very first block, the "from" offset should be negative.
    ///
    /// ASSUME: the chain is unbroken, e.g. this is being called successively on correctly encrypted, chained
    /// blocks, that have been written to FLASH such that we can refer to the older blocks for the current
    /// chaining value. It is up to the caller to manage the linear order of the calls.
    /// ASSUME: `from` + `self.ct_absolute_offset` is a multiple of an erase block
    /// returns the actual number of bytes processed
    pub fn encrypt_sector(&self, from: i32, input_plaintext: &[u8], output_sector: &mut [u8]) -> usize {
        assert!(output_sector.len() & (AES_BLOCKSIZE - 1) == 0, "output length must be a multiple of AES block size");
        assert!(input_plaintext.len() & (AES_BLOCKSIZE - 1) == 0, "input length must be a multiple of AES block size");
        assert!((from + self.ct_absolute_offset as i32) & (spinor::SPINOR_ERASE_SIZE as i32 - 1) == 0, "request address must line up with an erase block boundary");

        if from > 0 {
            assert!(input_plaintext.len() == output_sector.len(), "input and output length must match");
        } else {
            assert!(input_plaintext.len() == output_sector.len() - self.ct_absolute_offset, "output length must have space for the plaintext header");
        }

        let mut out_start_offset = 0;
        if from < 0 {
            // copy the plaintext header as-is: we don't modify it, once it's in the device (e.g. we won't switch
            // from eFuse to BBRAM or back again). Later on if such a switching is desired, I believe it's just a couple
            // bytes patched in the plaintext header to change this, but, for now, let's assume once you're on a given
            // encryption train, you're staying on it. BBRAM requires external provisioning anyways, and BBRAM users
            // would be using this feature because they don't trust eFuse. eFuse users can't switch to BBRAM without an
            // external programmer that would burn the key in the first place.
            // the plaintext header already has:
            //   - all the fuse and config settings
            //   - padding
            //   - IV for cipher
            //   - length of ciphertext region
            for (&src, dst) in self.bitstream[..self.ct_absolute_offset].iter()
            .zip(output_sector[..self.ct_absolute_offset].iter_mut()) {
                *dst = src;
            }
            out_start_offset = self.ct_absolute_offset;
        }
        let mut chain: [u8; AES_BLOCKSIZE] = self.iv;
        if from > 0 {
            bitflip(&self.ciphertext[from as usize - AES_BLOCKSIZE..from as usize], &mut chain);
        }

        let mut bytes_processed = 0;
        let mut temp_block = [0; AES_BLOCKSIZE];
        for (iblock, oblock) in input_plaintext.chunks(AES_BLOCKSIZE).into_iter()
        .zip(output_sector[out_start_offset..].chunks_mut(AES_BLOCKSIZE).into_iter()) {
            // XOR against IV/chain value
            for (dst, (&src, &iv)) in temp_block.iter_mut().zip(iblock.iter().zip(chain.iter())) {
                *dst = src ^ iv;
            }
            let mut e = GenericArray::clone_from_slice(&mut temp_block);
            self.enc_cipher.encrypt_block(&mut e);
            for (&src, dst) in e.iter().zip(temp_block.iter_mut()) {
                *dst = src;
            }
            // redo the IV based on the previous output. no bitflipping of the IV.
            for (&src, dst) in temp_block.iter().zip(chain.iter_mut()) {
                *dst = src;
            }

            // copy the ciphertext into the output block, with a bitflip
            bitflip(&temp_block, oblock);
            bytes_processed += AES_BLOCKSIZE;
        }
        bytes_processed
    }

    pub fn ciphertext_len(&self) -> usize {
        self.ciphertext.len()
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
    sensitive_data: xous::MemoryRange, // this gets purged at least on every suspend, but ideally purged sooner than that
    pass_cache: xous::MemoryRange,  // this can be purged based on a policy, as set below
    boot_password_policy: PasswordRetentionPolicy,
    update_password_policy: PasswordRetentionPolicy,
    cur_password_type: Option<PasswordType>, // for tracking which password we're dealing with at the UX layer
    susres: susres::Susres, // for disabling suspend/resume
    trng: trng::Trng,
    gfx: graphics_server::Gfx, // for reading out font planes for signing verification
    spinor: spinor::Spinor,
    ticktimer: ticktimer_server::Ticktimer,
    xns: xous_names::XousNames,
    //jtag: jtag::Jtag,
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

        let spinor = spinor::Spinor::new(&xns).expect("couldn't connect to spinor server");
        spinor.register_soc_token().expect("couldn't register rootkeys as the one authorized writer to the gateware update area!");
        //let jtag = jtag::Jtag::new(&xns).expect("couldn't connect to JTAG server");

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
            sensitive_data,
            pass_cache,
            update_password_policy: PasswordRetentionPolicy::AlwaysPurge,
            boot_password_policy: PasswordRetentionPolicy::AlwaysKeep,
            cur_password_type: None,
            susres: susres::Susres::new_without_hook(&xns).expect("couldn't connect to susres without hook"),
            trng: trng::Trng::new(&xns).expect("couldn't connect to TRNG server"),
            gfx: graphics_server::Gfx::new(&xns).expect("couldn't connect to gfx"),
            spinor,
            ticktimer: ticktimer_server::Ticktimer::new().expect("couldn't connect to ticktimer"),
            xns,
            //jtag,
        };

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

    fn xns_interlock(&self) {
        loop {
            if self.xns.trusted_init_done().expect("couldn't query init done status") {
                break;
            } else {
                log::warn!("trusted init not finished, rootkeys is holding off on sensitive operations");
                self.ticktimer.sleep_ms(650).expect("couldn't sleep");
            }
        }
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

                    for p in (*pcache_ptr).fpga_key.iter_mut() {
                        *p = 0;
                    }
                    (*pcache_ptr).fpga_key_valid = 0;
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
        let mut hasher = Sha512Trunc256::new_with_strategy(FallbackStrategy::SoftwareOnly);
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
        self.xns_interlock();
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
    pub fn do_key_init(&mut self, rootkeys_modal: &mut Modal, main_cid: xous::CID) -> Result<(), RootkeyResult> {
        self.xns_interlock();
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
        // capture the progress bar elements in a convenience structure
        let mut pb = ProgressBar::new(rootkeys_modal, &mut progress_action);

        // kick the progress bar to indicate we've entered the routine
        pb.set_percentage(1);

        // get access to the pcache and generate a keypair
        let pcache: &mut PasswordCache = unsafe{&mut *(self.pass_cache.as_mut_ptr() as *mut PasswordCache)};
        let keypair = if true { // true for production, false for debug (uses dev keys, so we can compare results)
            Keypair::generate(&mut self.trng)
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
        log::debug!("keypair privkey: {:x?}", keypair.secret.to_bytes());

        // encrypt the FPGA key using the update password. in an un-init system, it is provided to us in plaintext format
        // e.g. in the case that we're doing a BBRAM boot (eFuse flow would give us a 0's key and we'd later on set it)
        #[cfg(feature = "hazardous-debug")]
        self.debug_print_key(KeyRomLocs::FPGA_KEY as usize, 256, "FPGA key before encryption: ");
        {
            let sensitive_slice = self.sensitive_data.as_slice_mut::<u32>();
            // before we encrypt it, stash a copy in our password cache, as we'll need it later on to encrypt the bitstream
            for (word, key) in sensitive_slice[KeyRomLocs::FPGA_KEY as usize..KeyRomLocs::FPGA_KEY as usize + 256/(size_of::<u32>()*8)].iter()
            .zip(pcache.fpga_key.chunks_mut(4).into_iter()) {
                for (&s, d) in word.to_be_bytes().iter().zip(key.iter_mut()) {
                    *d = s;
                }
            }
            pcache.fpga_key_valid = 1;

            // now encrypt it in the staging area
            for (word, key_word) in sensitive_slice[KeyRomLocs::FPGA_KEY as usize..KeyRomLocs::FPGA_KEY as usize + 256/(size_of::<u32>()*8)].iter_mut()
            .zip(pcache.hashed_update_pw.chunks(4).into_iter()) {
                *word = *word ^ u32::from_be_bytes(key_word.try_into().unwrap());
            }
        }

        // allocate a decryption oracle for the FPGA bitstream. This will fail early if the FPGA key is wrong.
        assert!(pcache.fpga_key_valid == 1);
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
        { // scope sensitive_slice narrowly, as it borrows *self mutably, and can mess up later calls that borrow an immutable self
            // sensitive_slice is our staging area for the new keyrom contents
            let sensitive_slice = self.sensitive_data.as_slice_mut::<u32>();
            for (src, dst) in public_key.chunks(4).into_iter()
            .zip(sensitive_slice[KeyRomLocs::SELFSIGN_PUBKEY as usize..KeyRomLocs::SELFSIGN_PUBKEY as usize + 256/(size_of::<u32>()*8)].iter_mut()) {
                *dst = u32::from_be_bytes(src.try_into().unwrap())
            }
        }
        log::info!("public key as computed: {:x?}", public_key);

        // extract the update password key from the cache, and apply it to the private key
        #[cfg(feature = "hazardous-debug")]
        {
            log::debug!("cached boot passwords {:x?}", pcache.hashed_boot_pw);
            log::debug!("cached update password: {:x?}", pcache.hashed_update_pw);
        }
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

        pb.set_percentage(10);

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
        {
            let sensitive_slice = self.sensitive_data.as_slice_mut::<u32>();
            sensitive_slice[KeyRomLocs::CONFIG as usize] |= keyrom_config::INITIALIZED.ms(1);
        }

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
        self.make_gateware_backup(Some(&mut pb))?;

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

        if !self.verify_selfsign_kernel() {
            log::error!("kernel signature failed to verify, probably should not try to reboot!");
            src_oracle.clear();
            dst_oracle.clear();
            return Err(RootkeyResult::IntegrityError);
        }

        pb.set_percentage(98);
        let (gateware_sig, gateware_len) = self.sign_gateware(&keypair);
        log::debug!("gateware signature ({}): {:x?}", gateware_len, gateware_sig.to_bytes());
        self.commit_signature(gateware_sig, gateware_len, SignatureType::Gateware)?;

        // clean up the oracles
        src_oracle.clear();
        dst_oracle.clear();
        self.spinor.set_staging_write_protect(false).expect("couldn't un-protect the staging area");

        // finalize the progress bar on exit -- always leave at 100%
        pb.update_text(t!("rootkeys.init.finished", xous::LANG));
        pb.set_percentage(100);

        self.ticktimer.sleep_ms(2000).expect("couldn't show final message");

        Ok(())
    }

    #[cfg(feature = "hazardous-debug")]
    pub fn printkeys(&mut self) {
        // dump the keystore -- used to confirm that patching worked right. does not get compiled in when hazardous-debug is not enable.
        let sensitive_slice = self.sensitive_data.as_slice_mut::<u32>();
        for addr in 0..256 {
            self.keyrom.wfo(utra::keyrom::ADDRESS_ADDRESS, addr);
            self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[addr as usize] = self.keyrom.rf(utra::keyrom::DATA_DATA);
            log::info!("{:02x}: 0x{:08x}", addr, self.sensitive_data.borrow_mut().as_slice::<u32>()[addr as usize]);
        }
    }

    #[cfg(feature = "hazardous-debug")]
    pub fn test(&mut self) -> bool {
        let pcache: &mut PasswordCache = unsafe{&mut *(self.pass_cache.as_mut_ptr() as *mut PasswordCache)};

        // setup the local cache
        let sensitive_slice = self.sensitive_data.as_slice_mut::<u32>();
        for addr in 0..256 {
            self.keyrom.wfo(utra::keyrom::ADDRESS_ADDRESS, addr);
            self.sensitive_data.borrow_mut().as_slice_mut::<u32>()[addr as usize] = self.keyrom.rf(utra::keyrom::DATA_DATA);
            log::info!("{:02x}: 0x{:08x}", addr, self.sensitive_data.borrow_mut().as_slice::<u32>()[addr as usize]);
        }

        for (word, key) in sensitive_slice[KeyRomLocs::FPGA_KEY as usize..KeyRomLocs::FPGA_KEY as usize + 256/(size_of::<u32>()*8)].iter()
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
        match self.make_gateware_backup(30, 50, progress_modal, progress_action) {
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
            return false;
        }

        if self.verify_gateware(&dst_oracle, None).is_err() {
            log::error!("error occurred in gateware verification");
            return false;
        }

        true
    }

    /// copy data from a source region to a destination region, re-encrypting and patching as we go along.
    /// the region/region_base should be specified for the destination oracle
    ///
    /// KEYROM patching data must previously have been staged in the sensitive area.
    /// failure to do so would result in the erasure of all secret data.
    /// ASSUME: CSR appendix does not change during the copy (it is not copied/updated)
    fn gateware_copy_and_patch(&self, src_oracle: &BitstreamOracle, dst_oracle: &BitstreamOracle,
    mut maybe_pb: Option<&mut ProgressBar>) -> Result<(), RootkeyResult> {
        let sensitive_slice = self.sensitive_data.as_slice_mut::<u32>();
        if sensitive_slice[KeyRomLocs::CONFIG as usize] & keyrom_config::INITIALIZED.ms(1) == 0 {
            // the keys weren't initialized, or are wrong. abort!
            return Err(RootkeyResult::KeyError);
        }
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
        // hash the decrypt data; we assume we're not patching anything in the first sector.
        let mut bytes_hashed = spinor::SPINOR_ERASE_SIZE as usize - src_oracle.ciphertext_offset();
        bitflip(&pt_sector[..bytes_hashed], &mut flipper[..bytes_hashed]);
        hasher.update(&flipper[..bytes_hashed]);

        // now encrypt and patch the data to disk
        dst_oracle.encrypt_sector(
            -(dst_oracle.ciphertext_offset() as i32),
            &pt_sector[..bytes_hashed],
            &mut ct_sector // full array, for space for plaintext header
        );

        // restore the plaintext header from the *destination* -- in this case, the destination is the
        // gateware boot location, and this contains the fuse configurations we need to boot from (e.g. eFuse
        // or BBRAM). We also prefer our original header because we nominally trust in more than whatever header
        // is being presented to us for the update.
        assert!(src_oracle.pt_header_len() == dst_oracle.pt_header_len(), "source and destination gatewares have different header lengths. Check your vivado bitgen settings...");
        for (&src, dst) in dst_oracle.bitstream[..dst_oracle.pt_header_len()].iter().zip(ct_sector.iter_mut()) {
            *dst = src;
        }

        log::debug!("sector 0 patch len: {}", bytes_hashed);
        self.spinor.patch(dst_oracle.bitstream, dst_oracle.base(), &ct_sector, 0)
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
                dummy_consume ^= self.patch_sector(src_oracle, from, &mut pt_sector, &sensitive_slice[0..256]);
            }

            let hash_len = if bytes_hashed + decrypt_len < hash_stop {
                decrypt_len
            } else {
                hash_stop - bytes_hashed
            };
            if hash_len > 0 {
                bitflip(&pt_sector[..hash_len], &mut flipper[..hash_len]);
                hasher.update(&flipper[..hash_len]);
                bytes_hashed += hash_len;
                if hash_len != decrypt_len {
                    log::debug!("final short block len: {}", hash_len);
                }
            }

            dst_oracle.encrypt_sector(
                from as i32,
                &pt_sector[..decrypt_len],
                &mut ct_sector[..decrypt_len],
            );
            self.spinor.patch(dst_oracle.bitstream, dst_oracle.base(),
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

        dst_oracle.encrypt_sector(ct_last_block_loc as i32, &pt_sector[..pt_sector_len], &mut ct_sector[..pt_sector_len]);
        log::trace!("hash patching from 0x{:x} len {}", ct_last_block_loc, pt_sector_len);
        self.spinor.patch(dst_oracle.bitstream, dst_oracle.base(),
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
            log::info!("gateware verified");
            Ok(())
        } else {
            log::error!("gateware failed to verify");
            Err(RootkeyResult::IntegrityError)
        }
    }


    fn make_gateware_backup(&self, mut maybe_pb: Option<&mut ProgressBar>) -> Result<(), RootkeyResult> {
        let gateware_dest = self.staging();
        let mut gateware_dest_base = self.staging_base();
        let gateware_src = self.gateware();

        log::trace!("src: {:x?}", &gateware_src[0..32]);
        log::trace!("dst: {:x?}", &gateware_dest[0..32]);

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

        log::trace!("src: {:x?}", &gateware_src[0..32]);
        log::trace!("dst: {:x?}", &gateware_dest[0..32]);
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
    }

    #[cfg(feature = "hazardous-debug")]
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
        // for the kernel length, we can't know/trust the given length in the signature field, so we sign the entire
        // length of the region. This will increase the time it takes to verify; however, at the current trend, we'll probably
        // use most of the available space for the kernel, so by the time we're done maybe only 10-20% of the space is empty.
        let kernel_len = kernel_region.len() - SIGBLOCK_SIZE as usize;

        (signing_key.sign(&kernel_region[SIGBLOCK_SIZE as usize..]), kernel_len as u32)
    }

    /// the public key must already be in the cache
    pub fn verify_selfsign_kernel(&self) -> bool {
        let sensitive_slice = self.sensitive_data.as_slice::<u32>();
        if sensitive_slice[KeyRomLocs::CONFIG as usize] & keyrom_config::INITIALIZED.ms(1) == 0 {
            log::warn!("key cache was not initialized, can't verify the kernel with our self-signing key");
            return false;
        }

        // read the public key directly out of the keyrom
        let mut key: [u8; 32] = [0; 32];
        log::debug!("reading public key from cached area");
        for (word, &keyword) in key.chunks_mut(4).into_iter()
        .zip(sensitive_slice[KeyRomLocs::SELFSIGN_PUBKEY as usize..KeyRomLocs::SELFSIGN_PUBKEY as usize + 256/(size_of::<u32>()*8)].iter()) {
            for (&byte, dst) in keyword.to_be_bytes().iter().zip(word.iter_mut()) {
                *dst = byte;
            }
        }
        log::debug!("pubkey as reconstituted: {:x?}", key);
        let pubkey = PublicKey::from_bytes(&key).expect("public key was not valid");

        let kernel_region = self.kernel();
        let sig_region = &kernel_region[..core::mem::size_of::<SignatureInFlash>()];
        let sig_rec: &SignatureInFlash = unsafe{(sig_region.as_ptr() as *const SignatureInFlash).as_ref().unwrap()}; // this pointer better not be null, we just created it!
        let sig = Signature::new(sig_rec.signature);

        let kern_len = sig_rec.signed_len as usize;
        log::debug!("recorded kernel len: {} bytes", kern_len);
        log::debug!("verifying with signature {:x?}", sig_rec.signature);
        log::debug!("verifying with pubkey {:x?}", pubkey.to_bytes());

        match pubkey.verify_strict(&kernel_region[SIGBLOCK_SIZE as usize..], &sig) {
            Ok(()) => true,
            Err(e) => {
                log::error!("error verifying signature: {:?}", e);
                false
            }
        }
    }

    pub fn sign_gateware(&self, signing_key: &Keypair) -> (Signature, u32) {
        let mut hasher = Sha512::new_with_strategy(FallbackStrategy::WaitForHardware);
        let gateware_region = self.gateware();

        // sign everything except a hole just big enough to fit the digital signature record
        let mut bytes_hashed = CSR_CSV_OFFSET - core::mem::size_of::<SignatureInFlash>();
        hasher.update(&gateware_region[..bytes_hashed]);
        bytes_hashed += gateware_region.len() - CSR_CSV_OFFSET;
        hasher.update(&gateware_region[CSR_CSV_OFFSET..]);

        // note that we use the *prehash* version here, this produces a different signature than a straightforward ed25519 sign
        (signing_key.sign_prehashed(hasher, None).expect("couldn't sign the gateware"), bytes_hashed as u32)
    }

    pub fn verify_gateware_self_signature(&mut self) -> bool {
        log::info!("verifying gateware self signature");
        // read the public key directly out of the keyrom
        let pubkey = PublicKey::from_bytes(&self.read_key_256(KeyRomLocs::SELFSIGN_PUBKEY)).expect("public key was not valid");

        let mut hasher = Sha512::new_with_strategy(FallbackStrategy::WaitForHardware);
        let gateware_region = self.gateware();

        // verify everything except a hole just big enough to fit the digital signature record
        log::debug!("hashing gateware");
        hasher.update(&gateware_region[..CSR_CSV_OFFSET - core::mem::size_of::<SignatureInFlash>()]);
        hasher.update(&gateware_region[CSR_CSV_OFFSET..]);
        log::debug!("hash done");

        let mut sig_region: [u8; core::mem::size_of::<SignatureInFlash>()] = [0; core::mem::size_of::<SignatureInFlash>()];
        for (&src, dst) in gateware_region[CSR_CSV_OFFSET - core::mem::size_of::<SignatureInFlash>()..CSR_CSV_OFFSET].iter()
        .zip(sig_region.iter_mut()) {
            *dst = src;
        }
        let sig_rec: &SignatureInFlash = unsafe{(sig_region.as_ptr() as *const SignatureInFlash).as_ref().unwrap()}; // this pointer better not be null, we just created it!
        let sig = Signature::new(sig_rec.signature);
        log::debug!("sig_rec ({}): {:x?}", sig_rec.signed_len, sig_rec.signature);
        log::debug!("sig: {:x?}", sig.to_bytes());
        log::debug!("pubkey: {:x?}", pubkey.to_bytes());

        // note that we use the *prehash* version here, this has a different signature than a straightforward ed25519
        match pubkey.verify_prehashed(hasher, None, &sig) {
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

    pub fn commit_signature(&self, sig: Signature, len: u32, sig_type: SignatureType) -> Result<(), RootkeyResult> {
        let mut sig_region: [u8; core::mem::size_of::<SignatureInFlash>()] = [0; core::mem::size_of::<SignatureInFlash>()];
        // map a structure onto the signature region, so we can do something sane when writing stuff to it
        let mut signature: &mut SignatureInFlash = unsafe{(sig_region.as_mut_ptr() as *mut SignatureInFlash).as_mut().unwrap()}; // this pointer better not be null, we just created it!

        signature.version = 1;
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
                log::info!("gateware sig area before: {:x?}", &(self.gateware()[CSR_CSV_OFFSET - core::mem::size_of::<SignatureInFlash>()..CSR_CSV_OFFSET]));
                self.spinor.patch(self.gateware(), self.gateware_base(), &sig_region, (CSR_CSV_OFFSET - core::mem::size_of::<SignatureInFlash>()) as u32)
                    .map_err(|_| RootkeyResult::FlashError)?;
                log::info!("gateware sig area after: {:x?}", &(self.gateware()[CSR_CSV_OFFSET - core::mem::size_of::<SignatureInFlash>()..CSR_CSV_OFFSET]));
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

        // re-allow suspend/resume ops
        self.susres.set_suspendable(true).expect("couldn't re-allow suspend/resume");
    }
}
