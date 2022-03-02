use std::convert::TryInto;

use crate::*;
use aes_gcm_siv::{Aes256GcmSiv, Nonce, Key, Tag};
use aes_gcm_siv::aead::{Aead, NewAead, Payload};
use aes::{Aes256, Block, BLOCK_SIZE};
use aes::cipher::{BlockDecrypt, BlockEncrypt, NewBlockCipher, generic_array::GenericArray};
use root_keys::api::AesRootkeyType;
use spinor::SPINOR_BULK_ERASE_SIZE;
use subtle::ConstantTimeEq;
use core::ops::{Deref, DerefMut};
use core::mem::size_of;

use std::collections::HashMap;
#[cfg(not(feature="deterministic"))]
use std::collections::HashSet;
#[cfg(feature="deterministic")]
use std::collections::BTreeSet;
use std::collections::BinaryHeap;
use std::cmp::Reverse;
use std::io::{Result, Error, ErrorKind};

#[cfg(not(feature="deterministic"))]
type FspaceSet = HashSet::<PhysPage>;
#[cfg(feature="deterministic")]
type FspaceSet = BTreeSet::<PhysPage>;

/// Implementation-specific PDDB structures: for Precursor/Xous OS pair
pub(crate) const MBBB_PAGES: usize = 10;
pub(crate) const FSCB_PAGES: usize = 16;

/// size of a physical page
pub const PAGE_SIZE: usize = spinor::SPINOR_ERASE_SIZE as usize;
/// size of a virtual page -- after the AES encryption and journaling overhead is subtracted
pub const VPAGE_SIZE: usize = PAGE_SIZE - size_of::<Nonce>() - size_of::<Tag>() - size_of::<JournalType>();

/// length of the ciphertext in an AES-GCM-SIV page with key commitments
/// equal to the total plaintext to be encrypted, including the journal number
/// does not include the MAC overhead
pub const KCOM_CT_LEN: usize = 4004;

const WRAPPED_AES_KEYSIZE: usize = AES_KEYSIZE + 8;
const SCD_VERSION: u32 = 1;
#[repr(C)] // this can map directly into Flash
pub(crate) struct StaticCryptoData {
    /// a version number for the block
    pub(crate) version: u32,
    /// aes-256 key of the system basis, encrypted with the User0 root key, and wrapped using NIST SP800-38F
    pub(crate) system_key: [u8; WRAPPED_AES_KEYSIZE],
    /// a pool of fixed data used as a salt
    pub(crate) salt_base: [u8; 4096 - WRAPPED_AES_KEYSIZE - size_of::<u32>()],
}
impl StaticCryptoData {
    pub fn default() -> StaticCryptoData {
        StaticCryptoData {
            version: SCD_VERSION,
            system_key: [0u8; WRAPPED_AES_KEYSIZE],
            salt_base: [0u8; 4096 - WRAPPED_AES_KEYSIZE - size_of::<u32>()],
        }
    }
}
impl Deref for StaticCryptoData {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self as *const StaticCryptoData as *const u8, size_of::<StaticCryptoData>())
                as &[u8]
        }
    }
}

// emulated
#[cfg(not(any(target_os = "none", target_os = "xous")))]
type EmuMemoryRange = EmuStorage;
#[cfg(not(any(target_os = "none", target_os = "xous")))]
type EmuSpinor = HostedSpinor;

// native hardware
#[cfg(any(target_os = "none", target_os = "xous"))]
type EmuMemoryRange = xous::MemoryRange;
#[cfg(any(target_os = "none", target_os = "xous"))]
type EmuSpinor = spinor::Spinor;

pub(crate) struct PddbOs {
    spinor: EmuSpinor,
    rootkeys: root_keys::RootKeys,
    tt: ticktimer_server::Ticktimer,
    pddb_mr: EmuMemoryRange,
    /// page table base -- location in FLASH, offset from physical bottom of pddb_mr
    pt_phys_base: PageAlignedPa,
    /// local key store -- one page, to store exactly one key, used for the system basis.
    /// the rest of the keys are generated on the fly entirely from the user password + a salt also stored in this page
    key_phys_base: PageAlignedPa,
    /// make before break buffer base -- location in FLASH, offset from physical bottom of pddb_mr
    mbbb_phys_base: PageAlignedPa,
    /// free space circular buffer base -- location in FLASH, offset from physical bottom of pddb_mr
    fscb_phys_base: PageAlignedPa,
    data_phys_base: PageAlignedPa,
    /// We keep a copy of the raw key around because we have to combine this with the AAD of a block to derive the AES-GCM-SIV cipher.
    system_basis_key: Option<[u8; AES_KEYSIZE]>,
    /// derived cipher for handling fastspace -- cache it, so we can save the time cost of constructing the cipher key schedule
    cipher_ecb: Option<Aes256>,
    /// fast space cache
    fspace_cache: FspaceSet,
    /// memoize the location of the fscb log pages
    fspace_log_addrs: Vec::<PageAlignedPa>,
    /// memoize the current target offset for the next log entry
    fspace_log_next_addr: Option<PhysAddr>,
    /// track roughly how big the log has gotten, so we can pre-emptively garbage collect it before we get too full.
    fspace_log_len: usize,
    /// a cached copy of the FPGA's DNA ID, used in the AAA records.
    dna: u64,
    /// reference to a TrngPool object that's shared among all the hardware functions
    entropy: Rc<RefCell<TrngPool>>,
}

impl PddbOs {
    pub fn new(trngpool: Rc<RefCell<TrngPool>>) -> PddbOs {
        let xns = xous_names::XousNames::new().unwrap();
        #[cfg(any(target_os = "none", target_os = "xous"))]
        let pddb = xous::syscall::map_memory(
            xous::MemoryAddress::new(xous::PDDB_LOC as usize + xous::FLASH_PHYS_BASE as usize),
            None,
            PDDB_A_LEN as usize,
            xous::MemoryFlags::R | xous::MemoryFlags::RESERVE,
        )
        .expect("Couldn't map the PDDB memory range");
        #[cfg(any(target_os = "none", target_os = "xous"))]
        log::info!("pddb slice len: {}, PDDB_A_LEN: {}, raw len: {}", pddb.as_slice::<u8>().len(), PDDB_A_LEN, pddb.len()); // sanity check the PDDB size on init

        // the mbbb is located one page off from the Page Table
        let key_phys_base = PageAlignedPa::from(size_of::<PageTableInFlash>());
        log::debug!("key_phys_base: {:x?}", key_phys_base);
        let mbbb_phys_base = key_phys_base + PageAlignedPa::from(PAGE_SIZE);
        log::debug!("mbbb_phys_base: {:x?}", mbbb_phys_base);
        let fscb_phys_base = PageAlignedPa::from(mbbb_phys_base.as_u32() + MBBB_PAGES as u32 * PAGE_SIZE as u32);
        log::debug!("fscb_phys_base: {:x?}", fscb_phys_base);

        let llio = llio::Llio::new(&xns);
        // native hardware
        #[cfg(any(target_os = "none", target_os = "xous"))]
        let ret = PddbOs {
            spinor: spinor::Spinor::new(&xns).unwrap(),
            rootkeys: root_keys::RootKeys::new(&xns, Some(AesRootkeyType::User0)).expect("FATAL: couldn't access RootKeys!"),
            tt: ticktimer_server::Ticktimer::new().unwrap(),
            pddb_mr: pddb,
            pt_phys_base: PageAlignedPa::from(0 as u32),
            key_phys_base,
            mbbb_phys_base,
            fscb_phys_base,
            data_phys_base: PageAlignedPa::from(fscb_phys_base.as_u32() + FSCB_PAGES as u32 * PAGE_SIZE as u32),
            system_basis_key: None,
            cipher_ecb: None,
            fspace_cache: FspaceSet::new(),
            fspace_log_addrs: Vec::<PageAlignedPa>::new(),
            fspace_log_next_addr: None,
            fspace_log_len: 0,
            dna: llio.soc_dna().unwrap(),
            entropy: trngpool,
        };
        // emulated
        #[cfg(not(any(target_os = "none", target_os = "xous")))]
        let ret = {
            PddbOs {
                spinor: HostedSpinor::new(),
                rootkeys: root_keys::RootKeys::new(&xns, Some(AesRootkeyType::User0)).expect("FATAL: couldn't access RootKeys!"),
                tt: ticktimer_server::Ticktimer::new().unwrap(),
                pddb_mr: EmuStorage::new(),
                pt_phys_base: PageAlignedPa::from(0 as u32),
                key_phys_base,
                mbbb_phys_base,
                fscb_phys_base,
                data_phys_base: PageAlignedPa::from(fscb_phys_base.as_u32() + FSCB_PAGES as u32 * PAGE_SIZE as u32),
                system_basis_key: None,
                cipher_ecb: None,
                fspace_cache: FspaceSet::new(),
                fspace_log_addrs: Vec::<PageAlignedPa>::new(),
                fspace_log_next_addr: None,
                fspace_log_len: 0,
                dna: llio.soc_dna().unwrap(),
                entropy: trngpool,
            }
        };
        ret
    }

    #[cfg(not(any(target_os = "none", target_os = "xous")))]
    pub fn dbg_dump(&self, name: Option<String>, extra_keys: Option<&Vec::<KeyExport>>) {
        self.pddb_mr.dump_fs(&name);
        let mut export = Vec::<KeyExport>::new();
        if let Some(key) = self.system_basis_key {
            log::info!("(hosted mode debug) written key: {:x?}", key);
            let mut name = [0 as u8; 64];
            for (&src, dst) in PDDB_DEFAULT_SYSTEM_BASIS.as_bytes().iter().zip(name.iter_mut()) {
                *dst = src;
            }
            export.push(
                KeyExport {
                    basis_name: name,
                    key,
                }
            );
        }
        if let Some(extra) = extra_keys {
            for key in extra {
                export.push(*key);
            }
        }
        self.pddb_mr.dump_keys(&export, &name);
    }
    #[allow(dead_code)]
    #[cfg(any(target_os = "none", target_os = "xous"))]
    pub fn dbg_dump(&self, _name: Option<String>) {
        // placeholder
    }
    #[allow(dead_code)]
    #[cfg(not(any(target_os = "none", target_os = "xous")))]
    /// used to reset the hardware structure for repeated runs of testing within a single invocation
    pub fn test_reset(&mut self) {
        self.fspace_cache = FspaceSet::new();
        self.fspace_log_addrs = Vec::<PageAlignedPa>::new();
        self.system_basis_key = None;
        self.cipher_ecb = None;
        self.fspace_log_next_addr = None;
        self.pddb_mr.reset();
    }

    pub(crate) fn nonce_gen(&mut self) -> Nonce {
        let nonce_array = self.entropy.borrow_mut().get_nonce();
        *Nonce::from_slice(&nonce_array)
    }
    #[allow(dead_code)]
    pub(crate) fn dna(&self) -> u64 {self.dna}
    pub(crate) fn trng_slice(&mut self, slice: &mut [u8]) {
        self.entropy.borrow_mut().get_slice(slice);
    }
    pub(crate) fn trng_u32(&mut self) -> u32 {
        self.entropy.borrow_mut().get_u32()
    }
    pub(crate) fn trng_u8(&mut self) -> u8 {
        self.entropy.borrow_mut().get_u8()
    }
    pub(crate) fn timestamp_now(&self) -> u64 {self.tt.elapsed_ms()}
    /// checks if the root keys are initialized, which is a prerequisite to formatting and mounting
    pub(crate) fn rootkeys_initialized(&self) -> bool {
        self.rootkeys.is_initialized().expect("couldn't query initialization state of the rootkeys server")
    }
    /// patches data at an offset starting from the data physical base address, which corresponds
    /// exactly to the first entry in the page table
    pub(crate) fn patch_data(&self, data: &[u8], offset: u32) {
        log::trace!("patch offset: {:x} len: {:x}", offset, data.len());
        assert!(data.len() + offset as usize <= PDDB_A_LEN - self.data_phys_base.as_usize(), "attempt to store past disk boundary");
        self.spinor.patch(
            self.pddb_mr.as_slice(),
            xous::PDDB_LOC,
            &data,
            offset + self.data_phys_base.as_u32(),
        ).expect("couldn't write to data region in the PDDB");
    }
    fn patch_pagetable(&self, data: &[u8], offset: u32) {
        if cfg!(feature = "mbbb") {
            assert!(data.len() + offset as usize <= size_of::<PageTableInFlash>(), "attempt to patch past page table end");
            // 1. Check if there is an MBBB structure existing. If so, copy it into the empty slot in the page table, then erase the mbbb.
            if let Some(mbbb) = self.mbbb_retrieve() {
                if let Some(erased_offset) = self.pt_find_erased_slot() {
                    self.spinor.patch(
                        self.pddb_mr.as_slice(),
                        xous::PDDB_LOC,
                        &mbbb,
                        self.pt_phys_base.as_u32() + erased_offset,
                    ).expect("couldn't write to page table");
                }
                // if there *isn't* an erased slot, we still want to get rid of the MBBB structure. A lack of an
                // erased slot would indicate we lost power after we copied the previous MBBB structure but before we could erase
                // it from storage, so this picks up where we left off.
                self.mbbb_erase();
            }
            // 2. find the page we're patching
            let base_page = offset as usize & !(PAGE_SIZE - 1);
            // 3. copy the data to a local buffer
            let mut mbbb_page = [0u8; PAGE_SIZE];
            for (&src, dst) in
            self.pddb_mr.as_slice()[self.pt_phys_base.as_usize() + base_page..self.pt_phys_base.as_usize() + base_page + PAGE_SIZE].iter()
            .zip(mbbb_page.iter_mut()) {
                *dst = src;
            }
            // 4. patch the local buffer copy
            let base_offset = offset as usize & (PAGE_SIZE - 1);
            for (&src, dst) in data.iter()
            .zip(mbbb_page[base_offset..base_offset + data.len()].iter_mut()) {
                *dst = src;
            }
            // 5. pick a random offset in the MBBB for the target patch, and write the data into the target
            let offset = (self.entropy.borrow_mut().get_u8() % MBBB_PAGES as u8) as u32 * PAGE_SIZE as u32;
            self.patch_mbbb(&mbbb_page, offset);
            // 6. erase the original page area, thus making the MBBB the authorative location
            let blank = [0xffu8; PAGE_SIZE];
            self.spinor.patch(
                self.pddb_mr.as_slice(),
                xous::PDDB_LOC,
                &blank,
                self.pt_phys_base.as_u32() + base_page as u32,
            ).expect("couldn't write to page table");
        } else {
            self.patch_pagetable_raw(data, offset);
        }
    }
    /// Direct write to the page table, without MBBB buffering.
    fn patch_pagetable_raw(&self, data: &[u8], offset: u32) {
        assert!(data.len() + offset as usize <= size_of::<PageTableInFlash>(), "attempt to patch past page table end");
        self.spinor.patch(
            self.pddb_mr.as_slice(),
            xous::PDDB_LOC,
            &data,
            self.pt_phys_base.as_u32() + offset,
        ).expect("couldn't write to page table");
    }
    fn patch_keys(&self, data: &[u8], offset: u32) {
        assert!(data.len() + offset as usize <= PAGE_SIZE, "attempt to burn key data that is outside the key region");
        log::info!("patching keys area with {} bytes", data.len());
        self.spinor.patch(
            self.pddb_mr.as_slice(),
            xous::PDDB_LOC,
            data,
            self.key_phys_base.as_u32() + offset
        ).expect("couldn't burn keys");
    }
    fn patch_mbbb(&self, data: &[u8], offset: u32) {
        assert!(data.len() + offset as usize <= PAGE_SIZE * MBBB_PAGES, "mbbb patch would go out of bounds");
        self.spinor.patch(
            self.pddb_mr.as_slice(),
            xous::PDDB_LOC,
            data,
            self.mbbb_phys_base.as_u32() + offset
        ).expect("couldn't burn mbbb");
    }
    /// raw patch is provided for 128-bit incremental updates to the FLASH. For FastSpace master record writes,
    /// see fast_space_write()
    fn patch_fscb(&self, data: &[u8], offset: u32) {
        assert!(data.len() + offset as usize <= PAGE_SIZE * FSCB_PAGES, "fscb patch would go out of bounds");
        self.spinor.patch(
            self.pddb_mr.as_slice(),
            xous::PDDB_LOC,
            data,
            self.fscb_phys_base.as_u32() + offset
        ).expect("couldn't burn fscb");
    }

    /// Public function that "does the right thing" for patching in a page table entry based on a virtual to
    /// physical mapping. Note that the `va` is an *address* (in units of bytes), and the `phy_page_num` is
    /// a *page number* (so a physical address divided by the page size). It's a slightly awkward units, but
    /// it saves a bit of math going back and forth between the native storage formats of the records.
    pub(crate) fn pt_patch_mapping(&mut self, va: VirtAddr, phys_page_num: u32, cipher: &Aes256) {
        let mut pte = Pte::new(va, PtFlags::CLEAN, Rc::clone(&self.entropy));
        let mut block = Block::from_mut_slice(pte.deref_mut());
        //log::info!("pte pt: {:x?}", block);
        cipher.encrypt_block(&mut block);
        //log::info!("pte ct: {:x?}", block);
        self.patch_pagetable(&block, phys_page_num * aes::BLOCK_SIZE as u32);
    }

    /// erases a page table entry by overwriting it with garbage
    pub(crate) fn pt_erase(&mut self, phys_page_num: u32) {
        let mut eraseblock = [0u8; aes::BLOCK_SIZE];
        self.trng_slice(&mut eraseblock);
        self.patch_pagetable(&eraseblock, phys_page_num * aes::BLOCK_SIZE as u32);
    }
    /// Searches the page table for an MBBB slot. This is currently an O(N) search but
    /// in practice for Precursor there are only 8 pages, so it's quite fast on average.
    /// This would want to be optimized or cached for a much larger filesystem.
    /// Returns the physical address of the erased slot as an offset from the pt_phys_base()
    fn pt_find_erased_slot(&self) -> Option<PhysAddr> {
        let pt: &[u8] = &self.pddb_mr.as_slice()[self.pt_phys_base.as_usize()..self.pt_phys_base.as_usize() + size_of::<PageTableInFlash>()];
        let blank = [0xffu8; aes::BLOCK_SIZE];
        for (index, page) in pt.chunks(PAGE_SIZE).enumerate() {
            if page[..aes::BLOCK_SIZE] == blank {
                return Some( (index * PAGE_SIZE) as PhysAddr )
            }
        }
        None
    }
    fn pt_as_slice(&self) -> &[u8] {
        &self.pddb_mr.as_slice()[self.pt_phys_base.as_usize()..self.pt_phys_base.as_usize() + size_of::<PageTableInFlash>()]
    }

    /// scans the page tables and returns all entries for a given basis
    /// basis_name is needed to decrypt pages in case of a journal conflict
    pub(crate) fn pt_scan_key(&self, key: &[u8; AES_KEYSIZE], basis_name: &str) -> Option<HashMap::<VirtAddr, PhysPage>> {
        let cipher = Aes256::new(&GenericArray::from_slice(key));
        let pt = self.pt_as_slice();
        let mut map = HashMap::<VirtAddr, PhysPage>::new();
        let blank = [0xffu8; aes::BLOCK_SIZE];
        for (page_index, pt_page) in pt.chunks(PAGE_SIZE).enumerate() {
            let clean_page = if pt_page[..aes::BLOCK_SIZE] == blank {
                if let Some(page) = self.mbbb_retrieve() {
                    page
                } else {
                    log::debug!("Blank page in PT found, but no MBBB entry exists. PT is either corrupted or not initialized!");
                    pt_page
                }
            } else {
                pt_page
            };
            for (index, candidate) in clean_page.chunks(aes::BLOCK_SIZE).enumerate() {
                // encryption is in-place, but the candidates are read-only, so we have to copy them to a new location
                let mut block = Block::clone_from_slice(candidate);
                cipher.decrypt_block(&mut block);
                if let Some(pte) = Pte::try_from_slice(block.as_slice()) {
                    let mut pp = PhysPage(0);
                    pp.set_page_number(((page_index * PAGE_SIZE / aes::BLOCK_SIZE) + index) as PhysAddr);
                    // the state is clean because this entry is, by definition, synchronized with the disk
                    pp.set_clean(true);
                    pp.set_valid(true);
                    // handle conflicting journal versions here
                    if let Some(prev_page) = map.get(&pte.vaddr()) {
                        let cipher = Aes256GcmSiv::new(Key::from_slice(key));
                        let aad = self.data_aad(basis_name);
                        let prev_data = self.data_decrypt_page(&cipher, &aad, prev_page);
                        let new_data = self.data_decrypt_page(&cipher, &aad, &pp);
                        if let Some(new_d) = new_data {
                            if let Some(prev_d) = prev_data {
                                let prev_j = JournalType::from_le_bytes(prev_d[..size_of::<JournalType>()].try_into().unwrap());
                                let new_j = JournalType::from_le_bytes(new_d[..size_of::<JournalType>()].try_into().unwrap());
                                if new_j > prev_j {
                                    map.insert(pte.vaddr(), pp);
                                } else if new_j == prev_j {
                                    log::error!("Found duplicate blocks with same journal age, picking arbitrary block and moving on...");
                                }
                            } else {
                                // prev data was bogus anyways, replace with the new entry
                                map.insert(pte.vaddr(), pp);
                            }
                        } else {
                            // new data is bogus, ignore it
                        }
                    } else {
                        map.insert(pte.vaddr(), pp);
                    }
                }
            }
        }
        if map.len() > 0 {
            Some(map)
        } else {
            None
        }
    }

    /// maps a StaticCryptoData structure into the key area of the PDDB.
    fn static_crypto_data_get(&self) -> &StaticCryptoData {
        let scd_ptr = self.pddb_mr.as_slice::<u8>()[self.key_phys_base.as_usize()..self.key_phys_base.as_usize() + PAGE_SIZE].as_ptr() as *const StaticCryptoData;
        let scd: &StaticCryptoData = unsafe{scd_ptr.as_ref().unwrap()};
        scd
    }
    /// takes the key and writes it with zero, using hard pointer math and a compiler fence to ensure
    /// the wipe isn't optimized out.
    #[allow(dead_code)]
    fn syskey_erase(&mut self) {
        if let Some(mut key) = self.system_basis_key.take() {
            let b = key.as_mut_ptr();
            for i in 0..key.len() {
                unsafe {
                    b.add(i).write_volatile(core::mem::zeroed());
                }
            }
            core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        }
        self.cipher_ecb = None;
    }
    pub(crate) fn clear_password(&self) {
        self.rootkeys.clear_password(AesRootkeyType::User0);
    }
    pub (crate) fn try_login(&mut self) -> PasswordState {
        if self.system_basis_key.is_none() || self.cipher_ecb.is_none() {
            let scd = self.static_crypto_data_get();
            if scd.version == 0xFFFF_FFFF { // system is in the blank state
                return PasswordState::Uninit
            }
            if scd.version != SCD_VERSION {
                log::error!("Version mismatch for keystore, declaring database as uninitialized");
                return PasswordState::Uninit;
            }
            match self.rootkeys.unwrap_key(&scd.system_key, AES_KEYSIZE) {
                Ok(mut syskey) => {
                    let cipher = Aes256::new(GenericArray::from_slice(&syskey));
                    self.cipher_ecb = Some(cipher);
                    let mut system_key: [u8; AES_KEYSIZE] = [0; AES_KEYSIZE];
                    for (&src, dst) in syskey.iter().zip(system_key.iter_mut()) {
                        *dst = src;
                    }
                    self.system_basis_key = Some(system_key);
                    // erase the old vector completely
                    let nuke = syskey.as_mut_ptr();
                    for i in 0..syskey.len() {
                        unsafe{nuke.add(i).write_volatile(0)};
                    }
                    PasswordState::Correct
                }
                Err(e) => {
                    log::error!("Couldn't unwrap our system key: {:?}", e);
                    PasswordState::Incorrect
                }
            }
        } else {
            PasswordState::Correct
        }
    }
    fn syskey_ensure(&mut self) {
        while self.try_login() != PasswordState::Correct {
            self.clear_password(); // clear the bad password entry
            let xns = xous_names::XousNames::new().unwrap();
            let modals = modals::Modals::new(&xns).expect("can't connect to Modals server");
            modals.show_notification(t!("pddb.badpass_infallible", xous::LANG)).expect("notification failed");
        }
    }

    fn mbbb_as_slice(&self) -> &[u8] {
        &self.pddb_mr.as_slice()[self.mbbb_phys_base.as_usize()..self.mbbb_phys_base.as_usize() + MBBB_PAGES * PAGE_SIZE]
    }
    fn mbbb_retrieve(&self) -> Option<&[u8]> {
        // Invariant: MBBB pages should be blank, unless a page is stashed there. So, just check the first
        // AES key size region for "blankness"
        let blank = [0xffu8; aes::BLOCK_SIZE];
        for page in self.mbbb_as_slice().chunks(PAGE_SIZE) {
            if page[..aes::BLOCK_SIZE] == blank {
                continue;
            } else {
                return Some(page)
            }
        }
        None
    }
    fn mbbb_erase(&self) {
        let blank = [0xffu8; aes::BLOCK_SIZE];
        for (index, page) in self.mbbb_as_slice().chunks(PAGE_SIZE).enumerate() {
            if page[..aes::BLOCK_SIZE] == blank {
                continue;
            } else {
                let blank_page = [0xffu8; PAGE_SIZE];
                self.patch_mbbb(&blank_page, (index * PAGE_SIZE) as u32);
            }
        }
    }

    /// Create fast_space AAD: name, version number, and FPGA ID.
    /// This data is "well known", and fixed for every device, but changes
    /// from device to device. It prevents records from one device from being copied
    /// and used on another, and it also makes it annoying to swap out the FPGA.
    /// This makes it a bit harder to repair, but also makes it harder for an adversary
    /// to change out the FPGA on your board without also having to patch the OS.
    /// If you are doing a repair, patch out the LLIO function that returns the DNA with
    /// the desired target DNA to effectively bypass the check (you need to, of course,
    /// know what that DNA is in the first place, so hopefully you were able to extract
    /// it before you destroyed the FPGA).
    fn fast_space_aad(&self, aad: &mut Vec::<u8>) {
        aad.extend_from_slice(PDDB_FAST_SPACE_SYSTEM_BASIS.as_bytes());
        aad.extend_from_slice(&PDDB_VERSION.to_le_bytes());
        aad.extend_from_slice(&self.dna.to_le_bytes());
    }
    /// Assumes you are writing a "most recent" version of FastSpace. Thus
    /// Anytime the fscb is updated, all the partial records are nuked, as well as any existing record.
    /// Then, a _random_ location is picked to place the structure to help with wear levelling.
    fn fast_space_write(&mut self, fs: &FastSpace) {
        self.syskey_ensure();
        if let Some(system_basis_key) = self.system_basis_key {
            let key = Key::from_slice(&system_basis_key);
            let cipher = Aes256GcmSiv::new(key);
            let nonce_array = self.entropy.borrow_mut().get_nonce();
            let nonce = Nonce::from_slice(&nonce_array);
            let fs_ser: &[u8] = fs.deref();
            assert!( ((fs_ser.len() + size_of::<Nonce>() + size_of::<Tag>()) & (PAGE_SIZE - 1)) == 0,
                "FastSpace record is not page-aligned in size!");
            // AAD + data => Payload
            let mut aad = Vec::<u8>::new();
            self.fast_space_aad(&mut aad);
            let payload = Payload {
                msg: fs_ser,
                aad: &aad,
            };
            // these are useful for debug, but do a hard-comment on them because they leak secret info
            //log::info!("key: {:x?}", key);
            //log::info!("nonce: {:x?}", nonce);
            //log::info!("aad: {:x?}", aad);
            //log::info!("payload: {:x?}", fs_ser);
            let ciphertext = cipher.encrypt(nonce, payload).expect("failed to encrypt FastSpace record");
            //log::info!("ct_len: {}", ciphertext.len());
            //log::info!("mac: {:x?}", &ciphertext[ciphertext.len()-16..]);
            let ct_to_flash = ciphertext.deref();
            // determine which page we're going to write the ciphertext into
            let page_search_limit = FSCB_PAGES - ((PageAlignedPa::from(ciphertext.len()).as_usize() / PAGE_SIZE) - 1);
            log::info!("picking a random page out of {} pages for fscb", page_search_limit);
            let dest_page = self.entropy.borrow_mut().get_u32() % page_search_limit as u32;
            // atomicity of the FreeSpace structure is a bit of a tough topic. It's a fairly hefty structure,
            // that runs a risk of corruption as it's being written, if power is lost or the system crashes.
            // However, the guiding principle of this ordering is that it's better to have no FastSpace structure
            // (and force a re-computation of it by scanning all the open Basis), than it is to have a broken
            // FastSpace structure + stale SpaceUpdates. In particular a stale SpaceUpdate would lead the system
            // to conclude that some pages are free when they aren't. Thus, we prefer to completely erase the
            // FSCB region before committing the updated version.
            { // this is where we begin the "it would be bad if we lost power about now" code region
                // erase the entire fscb area
                let blank_sector: [u8; PAGE_SIZE] = [0xff; PAGE_SIZE];
                for offset in 0..FSCB_PAGES {
                    self.patch_fscb(&blank_sector, (offset * PAGE_SIZE) as u32);
                }
                // commit the fscb data
                self.patch_fscb(&[&nonce_array, ct_to_flash].concat(), dest_page * PAGE_SIZE as u32);
            } // end "it would be bad if we lost power now" region

            // note: this function should be followed up by a fast_space_read() to regenerate the temporary
            // bookkeeping variables that are not reset by this function.
        } else {
            panic!("invalid state!");
        }
    }

    /// Sweeps through the entire set of known data (as indicated in `page_heap`) and
    /// returns a subset of the total free space in a PhysPage vector that is a list of physical pages,
    /// in random order, that can be used by PDDB operations in the future without worry about
    /// accidentally overwriting Basis data that are locked.
    ///
    /// The function is coded to prioritize small peak memory footprint over speed, as it
    /// needs to run in a fairly memory-constrained environment, keeping in mind that if the PDDB
    /// structures were to be extended to run on say, an external USB drive with gigabytes of space,
    /// we cannot afford to naively allocate vectors that count every single page.
    fn fast_space_generate(&mut self, mut page_heap: BinaryHeap<Reverse<u32>>) -> Vec::<PhysPage> {
        let mut free_pool = Vec::<usize>::new();
        let max_entries = FASTSPACE_PAGES * PAGE_SIZE / size_of::<PhysPage>();
        free_pool.reserve_exact(max_entries);

        // 1. check that the page_heap has enough entries
        let total_used_pages = page_heap.len();
        let total_free_pages = (PDDB_A_LEN - self.data_phys_base.as_usize()) / PAGE_SIZE;
        log::info!("page alloc: {} used; {} free", total_used_pages, total_free_pages);
        if total_free_pages == 0 {
            log::warn!("Disk is out of space, no free pages available!");
            // return an empty free_pool vector.
            return Vec::<PhysPage>::new();
        }
        // 2. fill the free_pool, avoiding entries in the page_heap. This algorithm
        // uses a fixed amount of storage regardless of the size of the disk, but
        // it produces a free_pool that has entries biased toward the high
        // addresses.
        let mut min_used_page = page_heap.pop();
        // consider every page
        for page_candidate in 0..(PDDB_A_LEN - self.data_phys_base.as_usize()) / PAGE_SIZE {
            // if the page is used, skip it.
            if let Some(Reverse(mp)) = min_used_page {
                if page_candidate == mp as usize {
                    log::info!("removing used page from free_pool: {}", mp);
                    min_used_page = page_heap.pop();
                    continue;
                }
            }
            // page is free. if we've space in the pool, just deposit it there
            if free_pool.len() < max_entries {
                free_pool.push(page_candidate);
            } else {
                // page is free, but we have no space in the pool.
                // pick a random page from the pool, and replace it with the current page
                let index = self.entropy.borrow_mut().get_u32() as usize % free_pool.len();
                free_pool[index] = page_candidate;
            }
        }
        // 3. shuffle the contents of free_pool. This is important in the case that the
        // amount of free space starts to approach the size of the free pool, as it will
        // essentially come out of step 2 as a sorted list.
        // this shuffle is stolen out of the rand crate directly -- it's a small function,
        // and pulling in the ENTIRE rand crate for this code seemed very unnecessary.
        // https://github.com/rust-random/rand/blob/0f4fc6b4c303696bd5f8765a375162ac7142b1df/src/seq/mod.rs#L586-L592
        for i in (1..free_pool.len()).rev() {
            // invariant: elements with index > i have been locked in place.
            free_pool.swap(i, self.entropy.borrow_mut().get_u32() as usize % (i+1));
        }
        log::info!("free_pool initial count: {}", free_pool.len());

        // 4. ensure that the free pool stays within the defined deniability ratio + noise
        let mut noise = (self.entropy.borrow_mut().get_u32() as f32 / u32::MAX as f32) * FSCB_FILL_UNCERTAINTY;
        if self.entropy.borrow_mut().get_u8() > 127 {
            noise = -noise;
        }
        let deniable_free_pages = (total_free_pages as f32 * (FSCB_FILL_COEFFICIENT + noise)) as usize;
        // we're guaranteed to have at least one free page, because we errored out if the pages was 0 above.
        let deniable_free_pages = if deniable_free_pages == 0 { 1 } else { deniable_free_pages };
        free_pool.truncate(deniable_free_pages);
        log::warn!("total_free: {}; free_pool after PD trim: {}; max pages allowed: {}",
            total_free_pages, free_pool.len(), deniable_free_pages);

        // 5. Take the free_pool and annotate it for writing to disk
        let mut page_pool = Vec::<PhysPage>::new();
        for page in free_pool {
            let mut pp = PhysPage(0);
            pp.set_journal(self.trng_u8() % FSCB_JOURNAL_RAND_RANGE);
            pp.set_page_number(page as PhysAddr);
            pp.set_space_state(SpaceState::Free);
            pp.set_valid(true);
            page_pool.push(pp);
        }
        page_pool
    }

    fn fscb_deref(&self) -> &[u8] {
        &self.pddb_mr.as_slice()[self.fscb_phys_base.as_usize()..self.fscb_phys_base.as_usize() + FSCB_PAGES * PAGE_SIZE]
    }
    /// Reads the data structures in the FSCB, if any, and stores the results in the fspace_cache HashSet.
    /// Note the following convention on the fscb: if the first 128 bits of a page are all 1's, then that sector
    /// cannot contain the master FastSpace record. Also, if a sector is to contain *any* data, the first piece
    /// of data must start at exactly 16 bytes into the page (at the 129th bit). Examples:
    ///
    /// FF = must be all 1's
    /// xx/yy/zz = AES encrypted data. Techincally AES includes the all 1's ciphertext in its set, but it's extremely unlikely.
    ///
    /// Byte #
    /// | 0  | 1  | 2  | 3  | 4  | 5  | 6  | 7  | 8  | 9  |  A |  B | C  | D  | E  | F  | 10 | 11 | 12 | 13 | 14 | ...  # byte offset
    /// ---------------------------------------------------------------------------------------------------------------
    /// | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | ...  # page must be blank
    /// | xx | xx | xx | xx | xx | xx | xx | xx | xx | xx | xx | xx | xx | xx | xx | xx | yy | yy | yy | yy | yy | ...  # page must contain FastSpace record
    /// | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | zz | zz | zz | zz | zz | ...  # page contains an arbitrary number of SpaceUpdate records
    ///
    fn fast_space_read(&mut self) {
        self.syskey_ensure();
        if let Some(system_key) = self.system_basis_key {
            // remove the old contents, since we're about to re-read an authorative copy from disk.
            self.fspace_cache.clear();
            self.fspace_log_addrs.clear();
            self.fspace_log_next_addr = None;
            self.fspace_log_len = 0;

            // let fscb_slice = self.fscb_deref(); // can't use this line because it causse self to be immutably borrowed, so we write out the equivalent below.
            let fscb_slice = &self.pddb_mr.as_slice()[self.fscb_phys_base.as_usize()..self.fscb_phys_base.as_usize() + FSCB_PAGES * PAGE_SIZE];

            // 1. scan through the entire space, and look for the FastSpace record. It can be identified by the
            // first 16 (aes::BLOCK_SIZE) bytes not being all 1's.
            let blank: [u8; aes::BLOCK_SIZE] = [0xff; aes::BLOCK_SIZE];
            let mut fscb_pages = 0;
            let mut blank_pages = Vec::new();
            for page_start in (0..fscb_slice.len()).step_by(PAGE_SIZE) {
                if (fscb_slice[page_start..page_start + aes::BLOCK_SIZE] == blank)
                && (fscb_slice[page_start + aes::BLOCK_SIZE..page_start + aes::BLOCK_SIZE * 2] == blank) {
                //if fscb_slice[page_start..page_start + aes::BLOCK_SIZE].iter().zip(blank.iter()).all(|(&a,&b)| a==b)
                //&& fscb_slice[page_start + aes::BLOCK_SIZE..page_start + aes::BLOCK_SIZE * 2].iter().zip(blank.iter()).all(|(&a,&b)| a==b) {
                    // page has met the criteria for being blank, skip to the next page
                    blank_pages.push(page_start);
                    continue
                } else if fscb_slice[page_start..page_start+aes::BLOCK_SIZE] == blank {
                //} else if fscb_slice[page_start..page_start+aes::BLOCK_SIZE].iter().zip(blank.iter()).all(|(&a,&b)| a==b) {
                    // this page contains update records; stash it for scanning after we've read in the master record
                    self.fspace_log_addrs.push(PageAlignedPa::from(page_start));
                    continue
                } else {
                    // this page (and the ones immediately afterward) "should" contain the FastSpace encrypted record
                    if fscb_pages == 0 {
                        let mut fscb_buf = [0; FASTSPACE_PAGES * PAGE_SIZE - size_of::<Nonce>()];
                        // copy the encrypted data to the decryption buffer
                        for (&src, dst) in
                        fscb_slice[page_start + size_of::<Nonce>() .. page_start + FASTSPACE_PAGES * PAGE_SIZE]
                        .iter().zip(fscb_buf.iter_mut()) {
                            *dst = src;
                        }
                        let mut aad = Vec::<u8>::new();
                        self.fast_space_aad(&mut aad);
                        let payload = Payload {
                            msg: &fscb_buf,
                            aad: &aad,
                        };
                        let key = Key::from_slice(&system_key);
                        let cipher = Aes256GcmSiv::new(key);
                        match cipher.decrypt(Nonce::from_slice(&fscb_slice[page_start..page_start + size_of::<Nonce>()]), payload) {
                            Ok(msg) => {
                                log::info!("decrypted: {}, FastSpace size: {}", msg.len(), size_of::<FastSpace>());
                                assert!(msg.len() == size_of::<FastSpace>());
                                // payload now contains the decrypted data, and the "msg" return field is truncated to the right length
                                // we now map the FastSpace structure onto the payload using an unsafe operation.
                                let fs_ptr = (&msg).as_ptr() as *const FastSpace;
                                let fs_ref: &FastSpace = unsafe{fs_ptr.as_ref().unwrap()};
                                // iterate through the FastSpace disk image and extract the valid and free pages, and note them in the cache
                                for pp in fs_ref.free_pool.iter() {
                                    if pp.valid() && pp.space_state() == SpaceState::Free {
                                        self.fspace_cache.insert(*pp);
                                    }
                                }
                            },
                            Err(e) => {
                                log::warn!("FSCB data was found, but it did not decrypt correctly. Ignoring FSCB record. Error: {:?}", e)
                            }
                        }
                    }
                    fscb_pages += 1;
                }
            }

            // 2. visit the update_page_addrs and modify the fspace_cache accordingly.
            let cipher = Aes256::new(GenericArray::from_slice(&system_key));
            let mut block = Block::default();
            log::info!("space_log_addrs len: {}", self.fspace_log_addrs.len());
            for page in &self.fspace_log_addrs {
                for (index, ct_block) in
                fscb_slice[page.as_usize() + aes::BLOCK_SIZE .. page.as_usize() + PAGE_SIZE]
                .chunks_exact(aes::BLOCK_SIZE).enumerate() {
                    let mut is_blank = true;
                    for &ct in ct_block {
                        if ct != 0xFF {
                            is_blank = false;
                        }
                    }
                    if is_blank {
                        // end the scan at the first blank block. Note the location.
                        self.fspace_log_next_addr = Some((page.as_usize() + ((1 + index) * aes::BLOCK_SIZE)) as PhysAddr);
                        break;
                    }
                    // now try to decrypt the ciphertext block
                    for (&src, dst) in ct_block.iter().zip(block.iter_mut()) {
                        *dst = src;
                    }
                    cipher.decrypt_block(&mut block);
                    if let Some(pp) = SpaceUpdate::try_into_phys_page(block.as_slice()) {
                        log::debug!("maybe replacing fspace block: {:x?}", pp);
                        // note: pp.valid() isn't the cryptographic check, the cryptographic check of record validity is in try_into_phys_page()
                        if pp.valid() { // PS: it should always be valid!
                            if let Some(prev_pp) = self.fspace_cache.get(&pp) {
                                if pp.journal() > prev_pp.journal() {
                                    self.fspace_cache.replace(pp);
                                } else if pp.journal() == prev_pp.journal() {
                                    log::error!("got two identical journal revisions -- this shouldn't happen, prev: {:?}, candidate: {:?}", prev_pp, pp);
                                    panic!("Inconsistent FSCB state");
                                }
                            } else {
                                log::warn!("Strange...we have a journal entry for a free space page that isn't already in our cache. Guru meditation: {:?}", pp);
                                self.fspace_cache.insert(pp);
                            }
                        }
                    } else {
                        log::info!("possibly corrupted FSCB update record: {:x?}", block);
                    }
                }
            }
            // at this point, fspace_cache should contain a collection of either Free or Dirty pages. Both are
            // fair game for being recycled.

            // 3. Check to see if we have a target for our next cache write, if not, make one up.
            if self.fspace_log_next_addr.is_none() {
                if blank_pages.len() != 0 {
                    // pick a random page out of the blank pool (random for wear levelling)
                    let random_index = self.entropy.borrow_mut().get_u32() as usize % blank_pages.len();
                    // set the next log address at an offset of one AES block in from the top.
                    self.fspace_log_next_addr = Some((blank_pages[random_index] + aes::BLOCK_SIZE) as PhysAddr);
                } else {
                    log::warn!("FSCB has no blank space for new update records. This will cause fast_space_alloc() to fail, which can be remedied with a call to fast_space_generate().");
                }
            }
        } else {
            panic!("invalid state!");
        }
    }
    /// returns a count of the number of pages in the fspace cache
    pub fn fast_space_len(&self) -> usize {
        self.fspace_cache.len()
    }
    /// Normally, the fspace_log_next_addr is just incremented, but when it hits the end of the
    /// page, it's set to None. This function will do a modestly expensive scan of the FSCB area
    /// to try and either find another partially filled page, or a completely empty page.
    pub fn fast_space_ensure_next_log(&mut self) -> bool {
        if self.fspace_log_next_addr.is_some() {
            true
        } else {
            let fscb_slice = self.fscb_deref();
            let blank: [u8; aes::BLOCK_SIZE] = [0xff; aes::BLOCK_SIZE];
            let mut blank_pages = Vec::new();
            for page_start in (0..fscb_slice.len()).step_by(PAGE_SIZE) {
                if (fscb_slice[page_start..page_start + aes::BLOCK_SIZE] == blank)
                && (fscb_slice[page_start + aes::BLOCK_SIZE..page_start + aes::BLOCK_SIZE * 2] == blank) {
                    // page has met the criteria for being blank, skip to the next page
                    blank_pages.push(page_start);
                    continue
                } else if fscb_slice[page_start..page_start + aes::BLOCK_SIZE] == blank {
                    // this page contains update records; scan it for an empty slot
                    for (index, block) in
                    fscb_slice[page_start + aes::BLOCK_SIZE..page_start + PAGE_SIZE]
                    .chunks_exact(aes::BLOCK_SIZE).enumerate() {
                        // start with a size check; a failure mode of just iterating is the iterator will terminate early if the block sizes aren't the same.
                        let mut is_blank = block.len() == blank.len();
                        // now confirm that every item is the same
                        for (&a, &b) in block.iter().zip(blank.iter()) {
                            if a != b {is_blank = false;}
                        }
                        if is_blank {
                            self.fspace_log_next_addr = Some( (page_start + (1 + index) * aes::BLOCK_SIZE) as PhysAddr );
                            log::info!("Next FSCB entry: {:x?}", self.fspace_log_next_addr);
                            return true
                        }
                    }
                } else {
                    // this is probably an encrypted FastSpace page, just skip it
                    continue
                }
            }
            // if we got to this point, we couldn't find a partially full page. Pull a random page from the blank page pool.
            if blank_pages.len() != 0 {
                // pick a random page out of the blank pool (random for wear levelling)
                let random_index = self.entropy.borrow_mut().get_u32() as usize % blank_pages.len();
                // set the next log address at an offset of one AES block in from the top.
                self.fspace_log_next_addr = Some((blank_pages[random_index] + aes::BLOCK_SIZE) as PhysAddr);
                log::warn!("Moving log to fresh FSCB page: {:x?}", self.fspace_log_next_addr);
                true
            } else {
                false
            }
        }
    }
    pub fn fast_space_has_pages(&self, count: usize) -> bool {
        let mut free_count = 0;
        for pp in self.fspace_cache.iter() {
            if (pp.space_state() == SpaceState::Free || pp.space_state() == SpaceState::Dirty) && (pp.journal() < PHYS_PAGE_JOURNAL_MAX) {
                free_count += 1;
            } else {
                log::trace!("fastpace other entry: {:?}", pp.space_state());
            }
            if free_count >= count {
                return true
            }
        }
        false
    }
    /// Attempts to allocate a page out of the fspace cache (in RAM). This is the "normal" route for allocating pages.
    /// This call should be prefixed by a call to ensure_fast_space_alloc() to make sure it doesn't fail.
    /// We do a two-stage "look before you leap" because trying to dynamically redo the fast space allocation tables
    /// in the middle of trying to allocate pages causes a borrow checker problem -- because you're already using the
    /// page maps to figure out you ran out of space, but then you're trying to mutate them to allocate more space.
    /// This can lead to concurrency issues, so we do a "look before you leap" method instead, where we just check that
    /// the correct amount of free space is available before doing an allocation, and if not, we mutate the map to populate
    /// new free space; and if so, we mutate the map to remove the allocated page.
    pub fn try_fast_space_alloc(&mut self) -> Option<PhysPage> {
        // 1. Confirm that the fspace_log_next_addr is valid. If not, regenerate it, or fail.
        if !self.fast_space_ensure_next_log() {
            log::warn!("Couldn't ensure fast space log entry: {}", self.fspace_log_len);
            None
        } else {
            // 2. find the first page that is Free or Dirty. The order is already randomized, so we can do a stupid linear search.
            let mut maybe_alloc = None;
            for pp in self.fspace_cache.iter() {
                if (pp.space_state() == SpaceState::Free || pp.space_state() == SpaceState::Dirty) && (pp.journal() < PHYS_PAGE_JOURNAL_MAX) {
                    let mut ppc = pp.clone();
                    // take the state directly to Used, skipping MaybeUsed. If the system crashes between now and
                    // when the page is actually used, the consequence is a "lost" entry in the FastSpace cache. However,
                    // the entry will be reclaimed on the next full-space scan. This is a less-bad outcome than filling up
                    // the log with 2x the number of operations to record MaybeUsed and then Used.
                    ppc.set_space_state(SpaceState::Used);
                    ppc.set_clean(false); // the allocated page is not clean, because it hasn't been written to disk
                    ppc.set_valid(true); // the allocated page is now valid, so it should be flushed to disk
                    ppc.set_journal(pp.journal() + 1); // this is guaranteed not to overflow because of a check in the "if" clause above

                    // commit the usage to the journal
                    self.syskey_ensure();
                    let cipher = self.cipher_ecb.as_ref().expect("Inconsistent internal state - syskey_ensure() failed");
                    let mut update = SpaceUpdate::new(self.entropy.borrow_mut().get_u64(), ppc);
                    let mut block = Block::from_mut_slice(update.deref_mut());
                    log::trace!("block: {:x?}", block);
                    cipher.encrypt_block(&mut block);
                    let log_addr = self.fspace_log_next_addr.take().unwrap() as PhysAddr;
                    log::trace!("patch: {:x?}", block);
                    self.patch_fscb(&block, log_addr);
                    self.fspace_log_len += 1;
                    let next_addr = log_addr + aes::BLOCK_SIZE as PhysAddr;
                    if (next_addr & (PAGE_SIZE as PhysAddr - 1)) != 0 {
                        self.fspace_log_next_addr = Some(next_addr as PhysAddr);
                    } else {
                        // fspace_log_next_addr is already None because we used "take()". We'll find a free spot for the
                        // next journal entry the next time around.
                    }
                    maybe_alloc = Some(ppc);
                    break;
                }
            }
            if maybe_alloc.is_none() {
                log::warn!("Ran out of free space. fspace cache has {} entries", self.fspace_cache.len());
                //for entry in self.fspace_cache.iter() {
                //    log::info!("{:?}", entry);
                //}
            }
            if let Some(alloc) = maybe_alloc {
                assert!(self.fspace_cache.remove(&alloc), "inconsistent state: we found a free page, but later when we tried to update it, it wasn't there!");
            }
            maybe_alloc
        }
    }
    pub fn fast_space_free(&mut self, pp: &mut PhysPage) {
        self.fast_space_ensure_next_log();
        // update the fspace cache
        pp.set_space_state(SpaceState::Dirty);
        pp.set_journal(pp.journal() + 1);

        // re-cycle the space into the fspace_cache
        self.fspace_cache.insert(pp.clone());

        // commit the free'd block to the journal
        self.syskey_ensure();
        let cipher = self.cipher_ecb.as_ref().expect("Inconsistent internal state - syskey_ensure() failed");
        let mut update = SpaceUpdate::new(self.entropy.borrow_mut().get_u64(), pp.clone());
        let mut block = Block::from_mut_slice(update.deref_mut());
        log::trace!("block: {:x?}", block);
        cipher.encrypt_block(&mut block);
        let log_addr = self.fspace_log_next_addr.take().unwrap() as PhysAddr;
        log::trace!("patch: {:x?}", block);
        self.patch_fscb(&block, log_addr);
        self.fspace_log_len += 1;
        let next_addr = log_addr + aes::BLOCK_SIZE as PhysAddr;
        if (next_addr & (PAGE_SIZE as PhysAddr - 1)) != 0 {
            self.fspace_log_next_addr = Some(next_addr as PhysAddr);
        } else {
            // fspace_log_next_addr is already None because we used "take()". We'll find a free spot for the
            // next journal entry the next time around.
        }
        // mark the page as invalid, so that it will be deleted on the next PT sync
        pp.set_valid(false);
    }
    /// This is a "look before you leap" function that will potentially pause all system operations
    /// and do a deep scan for space if the required amount is not available.
    pub fn ensure_fast_space_alloc(&mut self, pages: usize, cache: &Vec::<BasisCacheEntry>) -> bool {
        const BUFFER: usize = 1; // a bit of slop in the trigger point
        let has_pages = self.fast_space_has_pages(pages + BUFFER);
        log::trace!("alloc fast_space_len: {}, log_len {}, has {} pages: {}", self.fast_space_len(), self.fspace_log_len, pages + BUFFER, has_pages);
        // make sure we have fast space pages...
        if has_pages
        // ..and make sure we have space for fast space log entries
        && (self.fspace_log_len < (FSCB_PAGES - FASTSPACE_PAGES - 1) * PAGE_SIZE / aes::BLOCK_SIZE) {
            true
        } else {
            if !has_pages {
                log::warn!("FastSpace alloc forced by lack of free space");
                // if we're really out of space, do an expensive full-space sweep
                if let Some(used_pages) = self.pddb_generate_used_map(cache) {
                    let free_pool = self.fast_space_generate(used_pages);
                    if free_pool.len() == 0 {
                        // we're out of free space
                        false
                    } else {
                        let mut fast_space = FastSpace {
                            free_pool: [PhysPage(0); FASTSPACE_FREE_POOL_LEN],
                        };
                        for pp in fast_space.free_pool.iter_mut() {
                            pp.set_journal(self.trng_u8() % FSCB_JOURNAL_RAND_RANGE)
                        }
                        for (&src, dst) in free_pool.iter().zip(fast_space.free_pool.iter_mut()) {
                            *dst = src;
                        }
                        // write just commits a new record to disk, but doesn't update our internal data cache
                        // this also clears the fast space log.
                        self.fast_space_write(&fast_space);
                        // this will ensure the data cache is fully in sync
                        self.fast_space_read();

                        // check that we have enough space now -- if not, we're just out of disk space!
                        if self.fast_space_len() > pages {
                            true
                        } else {
                            false
                        }
                    }
                } else {
                    false
                }
            } else {
                // log regenration is faster & less intrusive than fastspace regeneration, and we would have
                // to do this more often. So we have a separate path for this outcome.
                log::warn!("FastSpace alloc forced by lack of log space");
                let mut fast_space = FastSpace {
                    free_pool: [PhysPage(0); FASTSPACE_FREE_POOL_LEN],
                };
                for pp in fast_space.free_pool.iter_mut() {
                    pp.set_journal(self.trng_u8() % FSCB_JOURNAL_RAND_RANGE)
                }
                // regenerate from the existing fast space cache
                for (&src, dst) in self.fspace_cache.iter().zip(fast_space.free_pool.iter_mut()) {
                    *dst = src;
                }
                // write just commits a new record to disk, but doesn't update our internal data cache
                // this also clears the fast space log.
                self.fast_space_write(&fast_space);
                // this will re-read back in the data, shuffle the alloc order a bit, and ensure the data cache is fully in sync
                self.fast_space_read();
                // this will locate the next fast space log point.
                self.fast_space_ensure_next_log();
                true
            }
        }
    }

    pub(crate) fn data_aad(&self, name: &str) -> Vec::<u8> {
        let mut aad = Vec::<u8>::new();
        aad.extend_from_slice(&name.as_bytes());
        aad.extend_from_slice(&PDDB_VERSION.to_le_bytes());
        aad.extend_from_slice(&self.dna.to_le_bytes());
        aad
    }

    /// returns a decrypted page that still includes the journal number at the very beginning
    /// We don't clip it off because it would require re-allocating a vector, and it's cheaper (although less elegant) to later
    /// just index past it.
    pub(crate) fn data_decrypt_page(&self, cipher: &Aes256GcmSiv, aad: &[u8], page: &PhysPage) -> Option<Vec::<u8>> {
        let ct_slice = &self.pddb_mr.as_slice()[
            self.data_phys_base.as_usize() + page.page_number() as usize * PAGE_SIZE ..
            self.data_phys_base.as_usize() + (page.page_number() as usize + 1) * PAGE_SIZE];
        let nonce = &ct_slice[..size_of::<Nonce>()];
        let ct = &ct_slice[size_of::<Nonce>()..];
        match cipher.decrypt(
            Nonce::from_slice(nonce),
            Payload {
                aad,
                msg: ct,
            }
        ) {
            Ok(data) => {
                assert!(data.len() == VPAGE_SIZE + size_of::<JournalType>(), "authentication successful, but wrong amount of data was recovered");
                Some(data)
            },
            Err(e) => {
                log::trace!("Error decrypting page: {:?}", e); // sometimes this is totally "normal", like when we're testing for valid data.
                None
            }
        }
    }

    /// returns a decrypted page that also encodes a key commitments. In this case, a raw key is passed,
    /// instead of the generic AES-GCM-SIV cipher, because we need to derive the key commitment.
    /// Key commitments are a patch to work-around the salamander problem in AES-GCM-SIV see https://eprint.iacr.org/2020/1456.pdf
    ///
    /// The structure of a page with commit key storage is as follows:
    /// - Nonce - 12 bytes
    /// - ciphertext - 4004 bytes (includes the journal number)
    ///   - kcomm_nonce - 32 bytes
    ///   - kcomm - 32 bytes
    /// - MAC - 16 bytes
    /// We stripe the MAC at the end just in case the MAC has some arithmetic property that can betray the existence
    /// of a basis root record with key commitment. The committed key and the nonce both should be indistinguishable
    /// from ciphertext.
    pub(crate) fn data_decrypt_page_with_commit(&mut self, key: &[u8], aad: &[u8], page: &PhysPage) -> Option<Vec::<u8>> {
        const KCOM_NONCE_LEN: usize = 32;
        const KCOM_LEN: usize = 32;
        const MAC_LEN: usize = 16;
        let ct_slice = &self.pddb_mr.as_slice()[
            self.data_phys_base.as_usize() + page.page_number() as usize * PAGE_SIZE ..
            self.data_phys_base.as_usize() + (page.page_number() as usize + 1) * PAGE_SIZE];
        log::debug!("commit data at 0x{:x}", self.data_phys_base.as_usize() + page.page_number() as usize * PAGE_SIZE);
        let nonce = &ct_slice[..size_of::<Nonce>()];
        let ct_total = &ct_slice[size_of::<Nonce>()..];

        // extract the regions of the stored data and place them into their respective buffers
        let mut ct_plus_mac = [0u8; KCOM_CT_LEN + MAC_LEN];
        let mut nonce_comm = [0u8; KCOM_NONCE_LEN];
        let mut key_comm_stored = [0u8; KCOM_LEN];
        let mut ct_pos = 0;

        for (&src, dst) in ct_total[ct_pos..].iter().zip(ct_plus_mac[..KCOM_CT_LEN].iter_mut()) {
            *dst = src;
            ct_pos += 1;
        }
        for (&src, dst) in ct_total[ct_pos..].iter().zip(nonce_comm.iter_mut()) {
            *dst = src;
            ct_pos += 1;
        }
        for (&src, dst) in ct_total[ct_pos..].iter().zip(key_comm_stored.iter_mut()) {
            *dst = src;
            ct_pos += 1;
        }
        for (&src, dst) in ct_total[ct_pos..].iter().zip(ct_plus_mac[KCOM_CT_LEN..].iter_mut()) {
            *dst = src;
            ct_pos += 1;
        }
        assert!(ct_pos == PAGE_SIZE - size_of::<Nonce>(), "struct sizing error in unpacking page with key commit");
        log::debug!("found nonce of {:x?}", nonce);
        log::debug!("found kcom_nonce of {:x?}", nonce_comm);

        let (kenc, kcom) = self.kcom_func(key.try_into().unwrap(), &nonce_comm);
        let cipher = Aes256GcmSiv::new(Key::from_slice(&kenc));

        // Attempt decryption. This is None on failure
        let plaintext = cipher.decrypt(
            Nonce::from_slice(nonce),
            Payload {
                aad,
                msg: &ct_plus_mac,
            }
        ).ok();

        // Only return the plaintext if the stored key commitment agrees with the computed one
        if kcom.ct_eq(&key_comm_stored).into() {
            plaintext
        } else {
            None
        }
    }

    /// `data` includes the journal entry on top. The data passed in must be exactly one vpage plus the journal entry
    pub(crate) fn data_encrypt_and_patch_page(&mut self, cipher: &Aes256GcmSiv, aad: &[u8], data: &mut [u8], pp: &PhysPage) {
        assert!(data.len() == VPAGE_SIZE + size_of::<JournalType>(), "did not get a page-sized region to patch");
        let j = JournalType::from_le_bytes(data[..size_of::<JournalType>()].try_into().unwrap()).saturating_add(1);
        for (&src, dst) in j.to_le_bytes().iter().zip(data[..size_of::<JournalType>()].iter_mut()) { *dst = src; }
        let nonce = self.nonce_gen();
        let ciphertext = cipher.encrypt(
            &nonce,
            Payload {
                aad,
                msg: &data,
            }
        ).expect("couldn't encrypt data");
        self.patch_data(&[nonce.as_slice(), &ciphertext].concat(), pp.page_number() * PAGE_SIZE as u32);
    }

    /// `data` includes the journal entry on top.
    /// The data passed in must be exactly one vpage plus the journal entry minus the length of the commit structure (64 bytes),
    /// which is 4004 bytes total
    pub(crate) fn data_encrypt_and_patch_page_with_commit(&mut self, key: &[u8], aad: &[u8], data: &mut [u8], pp: &PhysPage) {
        assert!(data.len() == KCOM_CT_LEN, "did not get a key-commit sized region to patch");
        // updates the journal type
        let j = JournalType::from_le_bytes(data[..size_of::<JournalType>()].try_into().unwrap()).saturating_add(1);
        for (&src, dst) in j.to_le_bytes().iter().zip(data[..size_of::<JournalType>()].iter_mut()) { *dst = src; }
        // gets the AES-GCM-SIV nonce
        let nonce = self.nonce_gen();
        // makes a nonce for the key commit
        let mut kcom_nonce = [0u8; 32];
        self.trng_slice(&mut kcom_nonce);
        // generates the encryption and commit keys
        let (kenc, kcom) = self.kcom_func(key.try_into().unwrap(), &kcom_nonce);
        let cipher = Aes256GcmSiv::new(Key::from_slice(&kenc));
        let ciphertext = cipher.encrypt(
            &nonce,
            Payload {
                aad,
                msg: &data,
            }
        ).expect("couldn't encrypt data");
        let mut dest_page = [0u8; PAGE_SIZE];

        let mut written = 0; // used as a sanity check on the insane iterator chain constructed below
        for (&src, dst) in
        nonce.as_slice().iter()
        .chain(ciphertext[..KCOM_CT_LEN].iter())
        .chain(kcom_nonce.iter())
        .chain(kcom.iter())
        .chain(ciphertext[KCOM_CT_LEN..].iter())
        .zip(dest_page.iter_mut()) {
            *dst = src;
            written += 1;
        }
        assert!(written == 4096, "data sizing error in encryption with key commit");
        log::trace!("nonce: {:x?}", &nonce);
        log::debug!("dest_page[kcom_nonce]: {:x?}", &dest_page[12+4004..12+4004+32]);
        self.patch_data(&dest_page, pp.page_number() * PAGE_SIZE as u32);
    }

    /// Derive a key commitment. This takes in a base `key`, which is 256 bits;
    /// a `nonce` which is the 96-bit nonce used in the AES-GCM-SIV for a given block;
    /// and `nonce_com` which is the commitment nonce, set at 256 bits.
    /// The result is two tuples, (kenc, kcom).
    fn kcom_func(&self,
        key: &[u8; 32],
        nonce_com: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
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
        (k_enc.into(), k_com.into())
    }

    /// Meant to be called on boot. This will read the FastSpace record, and then attempt to load
    /// in the system basis.
    pub(crate) fn pddb_mount(&mut self) -> Option<BasisCacheEntry> {
        self.fast_space_read();
        self.syskey_ensure();
        if let Some(syskey) = self.system_basis_key {
            if let Some(sysbasis_map) = self.pt_scan_key(&syskey, PDDB_DEFAULT_SYSTEM_BASIS) {
                let aad = self.data_aad(PDDB_DEFAULT_SYSTEM_BASIS);
                // get the first page, where the basis root is guaranteed to be
                if let Some(root_page) = sysbasis_map.get(&VirtAddr::new(VPAGE_SIZE as u64).unwrap()) {
                    let vpage = match self.data_decrypt_page_with_commit(&syskey, &aad, root_page) {
                        Some(data) => data,
                        None => {log::error!("System basis decryption did not authenticate. Unrecoverable error."); return None;},
                    };
                    // if the below assertion fails, you will need to re-code this to decrypt more than one VPAGE and stripe into a basis root struct
                    assert!(size_of::<BasisRoot>() <= VPAGE_SIZE, "BasisRoot has grown past a single VPAGE, this routine needs to be re-coded to accommodate the extra bulk");
                    let mut basis_root = BasisRoot::default();
                    for (&src, dst) in vpage[size_of::<JournalType>()..].iter().zip(basis_root.deref_mut().iter_mut()) {
                        *dst = src;
                    }
                    if basis_root.magic != PDDB_MAGIC {
                        log::error!("Basis root did not deserialize correctly, unrecoverable error.");
                        return None;
                    }
                    if basis_root.version != PDDB_VERSION {
                        log::error!("PDDB version mismatch in system basis root. Unrecoverable error.");
                        return None;
                    }
                    let basis_name = std::str::from_utf8(&basis_root.name.data[..basis_root.name.len as usize]).expect("basis is not valid utf-8");
                    if basis_name != PDDB_DEFAULT_SYSTEM_BASIS {
                        log::error!("PDDB system basis name is incorrect: {}; aborting mount operation.", basis_name);
                        return None;
                    }
                    log::info!("System BasisRoot record found, generating cache entry");
                    BasisCacheEntry::mount(self, &basis_name, &syskey, false, BasisRetentionPolicy::Persist)
                } else {
                    // i guess technically we could try a brute-force search for the page, but meh.
                    log::error!("System basis did not contain a root page -- unrecoverable error.");
                    None
                }
            } else { None }
        } else {
            None
        }
    }

    /// this function is dangerous in that calling it will completely erase all of the previous data
    /// in the PDDB an replace it with a brand-spanking new, blank PDDB.
    /// The number of servers that can connect to the Spinor crate is strictly tracked, so we borrow a reference
    /// to the Spinor object allocated to the PDDB implementation for this operation.
    pub(crate) fn pddb_format(&mut self, fast: bool, progress: Option<&modals::Modals>) -> Result<()> {
        if !self.rootkeys.is_initialized().unwrap() {
            return Err(Error::new(ErrorKind::Unsupported, "Root keys are not initialized; cannot format a PDDB without root keys!"));
        }
        // step 0. If we have a modal, confirm that the password entered was correct with a double-entry.
        if let Some(modals) = progress {
            let mut success = false;
            while !success {
                // the "same password check" is accomplished by just encrypting the all-zeros block twice
                // with the cipher after clearing the password and re-entering it, and then comparing that
                // the results are identical. The test blocks are never committed or stored anywhere.
                // The actual creation of the "real" key material is done in step 3.
                let mut checkblock_a = [0u8; BLOCK_SIZE];
                self.rootkeys.decrypt_block(GenericArray::from_mut_slice(&mut checkblock_a));

                #[cfg(any(target_os = "none", target_os = "xous"))] // skip this dialog in hosted mode
                modals.show_notification(t!("pddb.checkpass", xous::LANG)).expect("notification failed");

                self.clear_password();
                let mut checkblock_b = [0u8; BLOCK_SIZE];
                self.rootkeys.decrypt_block(GenericArray::from_mut_slice(&mut checkblock_b));

                if checkblock_a == checkblock_b {
                    success = true;
                } else {
                    modals.show_notification(t!("pddb.checkpass_fail", xous::LANG)).expect("notification failed");
                    self.clear_password();
                }
            }
        }

        // step 1. Erase the entire PDDB region - leaves the state in all 1's
        if !fast {
            log::info!("Erasing the PDDB region");
            if let Some(modals) = progress {
                modals.start_progress(
                    t!("pddb.erase", xous::LANG),
                    xous::PDDB_LOC, xous::PDDB_LOC + PDDB_A_LEN as u32, xous::PDDB_LOC)
                    .expect("couldn't raise progress bar");
                self.tt.sleep_ms(100).unwrap();
            }
            for offset in (xous::PDDB_LOC..(xous::PDDB_LOC + PDDB_A_LEN as u32)).step_by(SPINOR_BULK_ERASE_SIZE as usize) {
                if (offset / SPINOR_BULK_ERASE_SIZE) % 4 == 0 {
                    log::info!("Initial erase: {}/{}", offset - xous::PDDB_LOC, PDDB_A_LEN as u32);
                    if let Some(modals) = progress {
                        modals.update_progress(offset as u32).expect("couldn't update progress bar");
                    }
                }
                // do a blank check first to see if the sector really needs erasing
                let mut blank = true;
                let slice_start = (offset - xous::PDDB_LOC) as usize / size_of::<u32>();
                for word in self.pddb_mr.as_slice::<u32>()[slice_start..slice_start + SPINOR_BULK_ERASE_SIZE as usize / size_of::<u32>()].iter() {
                    if *word != 0xFFFF_FFFF {
                        blank = false;
                        break;
                    }
                }
                if !blank {
                    self.spinor.bulk_erase(offset, SPINOR_BULK_ERASE_SIZE).expect("couldn't erase memory");
                }
            }
            if let Some(modals) = progress {
                modals.update_progress(xous::PDDB_LOC + PDDB_A_LEN as u32).expect("couldn't update progress bar");
                modals.finish_progress().expect("couldn't dismiss progress bar");
                self.tt.sleep_ms(100).unwrap();
            }
        }

        // step 2. fill in the page table with junk, which marks it as cryptographically empty
        if let Some(modals) = progress {
            modals.start_progress(t!("pddb.initpt", xous::LANG), 0, size_of::<PageTableInFlash>() as u32, 0).expect("couldn't raise progress bar");
        }
        let mut temp: [u8; PAGE_SIZE] = [0; PAGE_SIZE];
        for page in (0..(size_of::<PageTableInFlash>() & !(PAGE_SIZE - 1))).step_by(PAGE_SIZE) {
            self.entropy.borrow_mut().get_slice(&mut temp);
            self.patch_pagetable_raw(&temp, page as u32);
            if let Some(modals) = progress {
                if (page / PAGE_SIZE) % 16 == 0 {
                    modals.update_progress(page as u32).expect("couldn't update progress bar");
                }
            }
        }
        if let Some(modals) = progress {
            modals.update_progress(size_of::<PageTableInFlash>() as u32).expect("couldn't update progress bar");
        }
        if size_of::<PageTableInFlash>() & (PAGE_SIZE - 1) != 0 {
            let remainder_start = size_of::<PageTableInFlash>() & !(PAGE_SIZE - 1);
            log::info!("Page table does not end on a page boundary. Handling trailing page case of {} bytes", size_of::<PageTableInFlash>() - remainder_start);
            let mut temp = Vec::<u8>::new();
            for _ in remainder_start..size_of::<PageTableInFlash>() {
                temp.push(self.entropy.borrow_mut().get_u8());
            }
            self.patch_pagetable_raw(&temp, remainder_start as u32);
        }
        if let Some(modals) = progress {
            modals.finish_progress().expect("couldn't dismiss progress bar");
            self.tt.sleep_ms(100).unwrap();
        }

        // step 3. create our key material
        // consider: making ensure_aes_password() a pub-scoped function? let's see how this works in practice.
        //if !self.rootkeys.ensure_aes_password() {
        //    return Err(Error::new(ErrorKind::PermissionDenied, "unlock password was incorrect"));
        //}
        if let Some(modals) = progress {
            modals.start_progress(t!("pddb.key", xous::LANG), 0, 100, 0).expect("couldn't raise progress bar");
            self.tt.sleep_ms(100).unwrap();
        }
        assert!(size_of::<StaticCryptoData>() == PAGE_SIZE, "StaticCryptoData structure is not correctly sized");
        let mut system_basis_key: [u8; AES_KEYSIZE] = [0; AES_KEYSIZE];
        self.entropy.borrow_mut().get_slice(&mut system_basis_key);
        // build the ECB cipher for page table entries
        self.cipher_ecb = Some(Aes256::new(GenericArray::from_slice(&system_basis_key)));
        let cipher_ecb = Aes256::new(GenericArray::from_slice(&system_basis_key)); // a second copy for patching the page table later in this routine interior mutability blah blah work around oops
        // now wrap the key for storage
        let wrapped_key = self.rootkeys.wrap_key(&system_basis_key).expect("Internal error wrapping our encryption key");
        self.system_basis_key = Some(system_basis_key); // this causes system_basis_key to be owned by self and go out of scope
        let mut crypto_keys = StaticCryptoData::default();
        crypto_keys.version = SCD_VERSION; // should already be set by `default()` but let's be sure.
        if let Some(modals) = progress {
            modals.update_progress(50).expect("couldn't update progress bar");
            self.tt.sleep_ms(100).unwrap();
        }
        // copy the encrypted key into the data structure for commit to Flash
        // the wrapped key should have a well-defined length of 40 bytes
        assert!(wrapped_key.len() == 40, "wrapped key did not have the expected length");
        for (&src, dst) in wrapped_key.iter().zip(crypto_keys.system_key.iter_mut()) {
            *dst = src;
        }
        // initialize the salt
        self.entropy.borrow_mut().get_slice(&mut crypto_keys.salt_base);
        // commit keys
        self.patch_keys(crypto_keys.deref(), 0);
        if let Some(modals) = progress {
            modals.update_progress(100).expect("couldn't update progress bar");
            self.tt.sleep_ms(100).unwrap();
            modals.finish_progress().expect("couldn't dismiss progress bar");
            self.tt.sleep_ms(100).unwrap();
        }
        // now we have a copy of the AES key necessary to encrypt the default System basis that we created in step 2.

        #[cfg(not(any(target_os = "none", target_os = "xous")))]
        self.tt.sleep_ms(500).unwrap(); // delay for UX to catch up in emulation

        // step 4. mbbb handling
        // mbbb should just be blank at this point, and the flash was erased in step 1, so there's nothing to do.

        // step 5. fscb handling
        // pick a set of random pages from the free pool and assign it to the fscb
        // pass the generator an empty cache - this causes it to treat the entire disk as free space
        if let Some(modals) = progress {
            modals.start_progress(t!("pddb.fastspace", xous::LANG), 0, 100, 0).expect("couldn't raise progress bar");
            self.tt.sleep_ms(100).unwrap();
        }
        let free_pool = self.fast_space_generate(BinaryHeap::<Reverse<u32>>::new());
        let mut fast_space = FastSpace {
            free_pool: [PhysPage(0); FASTSPACE_FREE_POOL_LEN],
        };
        for (&src, dst) in free_pool.iter().zip(fast_space.free_pool.iter_mut()) {
            *dst = src;
        }
        if let Some(modals) = progress {
            modals.update_progress(50).expect("couldn't update progress bar");
            self.tt.sleep_ms(100).unwrap();
        }
        self.fast_space_write(&fast_space);
        if let Some(modals) = progress {
            modals.update_progress(100).expect("couldn't update progress bar");
            self.tt.sleep_ms(100).unwrap();
            modals.finish_progress().expect("couldn't dismiss progress bar");
            self.tt.sleep_ms(100).unwrap();
        }

        #[cfg(not(any(target_os = "none", target_os = "xous")))]
        self.tt.sleep_ms(500).unwrap();

        // step 5. salt the free space with random numbers. this can take a while, we might need a "progress report" of some kind...
        // this is coded using "direct disk" offsets...under the assumption that we only ever really want to do this here, and
        // not re-use this routine elsewhere.
        if let Some(modals) = progress {
            modals.start_progress(t!("pddb.randomize", xous::LANG),
            self.data_phys_base.as_u32(), PDDB_A_LEN as u32, self.data_phys_base.as_u32()).expect("couldn't raise progress bar");
            self.tt.sleep_ms(100).unwrap();
        }
        let blank = [0xffu8; aes::BLOCK_SIZE];
        for offset in (self.data_phys_base.as_usize()..PDDB_A_LEN).step_by(PAGE_SIZE) {
            if fast {
                // we could "skip" pages already encrypted to an old key as a short cut -- because we nuked our
                // session key, previous data should be undecipherable. You shouldn't do this for a production erase
                // but this is good for speeding up testing.
                let mut is_blank = true;
                let block: &[u8] = &self.pddb_mr.as_slice()[offset + aes::BLOCK_SIZE * 3..offset + aes::BLOCK_SIZE * 4];
                for (&a, &b) in block.iter().zip(blank.iter()) {
                    if a != b {
                        is_blank = false;
                        break;
                    }
                }
                if !is_blank {
                    if (offset / PAGE_SIZE) % 16 == 0 {
                        log::info!("Page at {} is likely to already have cryptographic data, skipping...", offset);
                    }
                    continue;
                }
            }
            self.entropy.borrow_mut().get_slice(&mut temp);
            if (offset / PAGE_SIZE) % 256 == 0 { // ~one update per megabyte
                log::info!("Cryptographic 'erase': {}/{}", offset, PDDB_A_LEN);
                if let Some(modals) = progress {
                    modals.update_progress(offset as u32).expect("couldn't update progress bar");
                }
            }
            self.spinor.patch(
                self.pddb_mr.as_slice(),
                xous::PDDB_LOC,
                &temp,
                offset as u32
            ).expect("couldn't fill in disk with random datax");
        }
        if let Some(modals) = progress {
            modals.update_progress(PDDB_A_LEN as u32).expect("couldn't update progress bar");
            self.tt.sleep_ms(100).unwrap();
            modals.finish_progress().expect("couldn't dismiss progress bar");
            self.tt.sleep_ms(100).unwrap();
        }

        // step 6. create the system basis root structure
        if let Some(modals) = progress {
            modals.start_progress(t!("pddb.structure", xous::LANG), 0, 100, 0).expect("couldn't raise progress bar");
            self.tt.sleep_ms(100).unwrap();
        }
        let basis_root = BasisRoot {
            magic: api::PDDB_MAGIC,
            version: api::PDDB_VERSION,
            name: BasisRootName::try_from_str(PDDB_DEFAULT_SYSTEM_BASIS).unwrap(),
            age: 0,
            num_dictionaries: 0,
        };

        // step 7. Create a hashmap for our reverse PTE, allocate sectors, and add it to the Pddb's cache
        self.fast_space_read(); // we reconstitute our fspace map even though it was just generated, partially as a sanity check that everything is ok
        if let Some(modals) = progress {
            modals.update_progress(33).expect("couldn't update progress bar");
            self.tt.sleep_ms(100).unwrap();
        }

        let mut basis_v2p_map = HashMap::<VirtAddr, PhysPage>::new();
        // allocate one page for the basis root
        if let Some(alloc) = self.try_fast_space_alloc() {
            let mut rpte = alloc.clone();
            rpte.set_clean(true); // it's not clean _right now_ but it will be by the time this routine is done...
            rpte.set_valid(true);
            let va = VirtAddr::new((1 * VPAGE_SIZE) as u64).unwrap(); // page 1 is where the root goes, by definition
            log::info!("adding basis va {:x?} with pte {:?}", va, rpte);
            basis_v2p_map.insert(va, rpte);
        }

        // step 8. write the System basis to Flash, at the physical locations noted above. This is an extract
        // from the basis_sync() method on a BasisCache entry, but because we haven't created a cache entry,
        // we're copypasta'ing the code here
        let aad = basis_root.aad(self.dna);
        let pp = basis_v2p_map.get(&VirtAddr::new(1 * VPAGE_SIZE as u64).unwrap())
            .expect("Internal consistency error: Basis exists, but its root map was not allocated!");
        assert!(pp.valid(), "v2p returned an invalid page");
        let journal_bytes = (self.trng_u32() % JOURNAL_RAND_RANGE).to_le_bytes();
        let slice_iter =
            journal_bytes.iter() // journal rev
            .chain(basis_root.as_ref().iter());
        let mut block = [0 as u8; KCOM_CT_LEN];
        for (&src, dst) in slice_iter.zip(block.iter_mut()) {
            *dst = src;
        }
        let syskey = self.system_basis_key.unwrap(); // take the key out
        self.data_encrypt_and_patch_page_with_commit(&syskey, &aad, &mut block, &pp);
        self.system_basis_key = Some(syskey); // put the key back
        if let Some(modals) = progress {
            modals.update_progress(66).expect("couldn't update progress bar");
            self.tt.sleep_ms(100).unwrap();
        }

        // step 9. generate & write initial page table entries
        for (&virt, phys) in basis_v2p_map.iter_mut() {
            self.pt_patch_mapping(virt, phys.page_number(), &cipher_ecb);
            // mark the entry as clean, as it has been sync'd to disk
            phys.set_clean(true);
        }
        if let Some(modals) = progress {
            modals.update_progress(100).expect("couldn't update progress bar");
            self.tt.sleep_ms(100).unwrap();
            modals.finish_progress().expect("couldn't dismiss progress bar");
            self.tt.sleep_ms(100).unwrap();
        }
        Ok(())
    }

    /// This function will prompt the user to unlock all the Basis. If the user asserts all
    /// Basis have been unlocked, the function returns `true`. The other option is the user
    /// can decline to unlock all the Basis right now, cancelling out of the process, which will
    /// cause the requesting free space sweep to fail.
    pub(crate) fn pddb_generate_used_map(&self, cache: &Vec::<BasisCacheEntry>) -> Option<BinaryHeap<Reverse<u32>>> {
        if let Some(extra_keys) = self.pddb_get_additional_keys(&cache) {
            let mut page_heap = BinaryHeap::new();
            // 1. check the extra keys, if any
            for (key, name) in extra_keys {
                // scan the extra closed basis
                if let Some(map) = self.pt_scan_key(&key, &name) {
                    for pp in map.values() {
                        page_heap.push(Reverse(pp.page_number()))
                    }
                }
            }
            // 2. scan through all of the physical pages in our caches, and add them to a binary heap.
            //    WARNING: this could get really big for a very large filesystem. It's capped at ~100k for
            //    Precursor's ~100MiB storage increment.
            for entry in cache {
                for pp in entry.v2p_map.values() {
                    page_heap.push(Reverse(pp.page_number()));
                }
            }
            Some(page_heap)
        } else {
            None
        }
    }

    /// UX function that informs the user of the currently open Basis, and prompts the user to enter passwords
    /// for other basis that may not currently be open. This function is also responsible for validating
    /// that the password is correct by doing a quick scan for "any" PTEs that happen to decrypt to something
    /// valid (it'll scan up and stop as soon as one Pte checks out). Note that it then only returns
    /// a Vec of keys & names, not a BasisCacheEntry -- so it means that the Basis still are "closed"
    /// at the conclusion of the sweep, but their page use can be accounted for.
    pub(crate) fn pddb_get_additional_keys(&self, cache: &Vec::<BasisCacheEntry>) -> Option<Vec<([u8; AES_KEYSIZE], String)>> {
        log::info!("{} basis are open, with the following names:", cache.len());
        for entry in cache {
            log::info!(" - {}", entry.name);
        }

        // 0. allow user to cancel out of the operation -- this will abort everything and cause the current
        //    alloc operation to fail

        // 1. prompt user to enter any name/password combos for other basis we want to keep
        // 2. validate the name/password combo
        // 3. add to the Aes256 return vec
        // 4. repeat summary print-out

        // for now, do nothing -- just indicate success with a returned empty set
        Some(Vec::<([u8; AES_KEYSIZE], String)>::new())
    }

    /// Derives a 256-bit AES encryption key for a basis given a basis name and its password.
    /// You will also need to derive the AAD for the basis using the basis_name.
    pub(crate) fn basis_derive_key(&self, basis_name: &str, password: &str) -> [u8; AES_KEYSIZE] {
        use sha2::{FallbackStrategy, Sha512Trunc256};
        use digest::Digest;
        use backend::bcrypt::*;

        // 1. derive the salt from the "key" region. First step is to create the salt lookup
        // table, which is done by hashing the name and password together with SHA-512
        // manage the allocation of the data for the basis & password explicitly so that we may wipe them later
        let mut bname_copy = [0u8; BASIS_NAME_LEN];
        for (src, dst) in basis_name.bytes().zip(bname_copy.iter_mut()) {
            *dst = src;
        }
        let mut plaintext_pw: [u8; 73] = [0; 73];
        for (src, dst) in password.bytes().zip(plaintext_pw.iter_mut()) {
            *dst = src;
        }
        plaintext_pw[72] = 0; // always null terminate

        log::info!("creating salt");
        // uses Sha512Trunc256 on the salt array to generate a compressed version of
        // the basis name and plaintext password, which forms the Salt that is fed into bcrypt
        // our salt is probably way too big but what else are we going to use all that page's data for?
        let scd = self.static_crypto_data_get();
        let mut salt = [0u8; 16];
        let mut hasher = Sha512Trunc256::new_with_strategy(FallbackStrategy::SoftwareOnly);
        hasher.update(&scd.salt_base);
        hasher.update(&bname_copy);
        hasher.update(&plaintext_pw);
        let result = hasher.finalize();
        for (&src, dst) in result.iter().zip(salt.iter_mut()) {
            *dst = src;
        }
        log::info!("derived salt: {:x?}", salt);

        // 3. use the salt + password and run bcrypt on it to derive a key.
        let mut hashed_password: [u8; 24] = [0; 24];
        let start_time = self.timestamp_now();
        bcrypt(BCRYPT_COST, &salt, password, &mut hashed_password); // note: this internally makes a copy of the password, and destroys it
        let elapsed = self.timestamp_now() - start_time;
        log::info!("derived bcrypt password in {}ms", elapsed);

        // 4. take the resulting 24-byte password and expand it to 32 bytes using sha512trunc256
        let mut expander = Sha512Trunc256::new_with_strategy(FallbackStrategy::SoftwareOnly);
        expander.update(hashed_password);
        let final_key = expander.finalize();
        let mut key = [0u8; AES_KEYSIZE];
        for (&src, dst) in final_key.iter().zip(key.iter_mut()) {
            *dst = src;
        }

        // 5. erase extra plaintext copies made of the basis name and password using a routine that
        // shouldn't be optimized out or re-ordered
        let bn_ptr = bname_copy.as_mut_ptr();
        for i in 0..bname_copy.len() {
            unsafe{bn_ptr.add(i).write_volatile(core::mem::zeroed());}
        }
        let pt_ptr = plaintext_pw.as_mut_ptr();
        for i in 0..plaintext_pw.len() {
            unsafe{pt_ptr.add(i).write_volatile(core::mem::zeroed());}
        }
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

        //6. return the key
        key
    }
}
