use std::convert::TryInto;

use crate::*;
use aes_gcm_siv::{Aes256GcmSiv, Nonce, Key, Tag};
use aes_gcm_siv::aead::{Aead, NewAead, Payload};
use aes::Aes256;
use aes::cipher::{BlockCipher, BlockDecrypt, BlockEncrypt, NewBlockCipher, generic_array::GenericArray};
use root_keys::api::{AesRootkeyType, Block};
use core::ops::{Deref, DerefMut};
use core::mem::size_of;
use core::convert::TryFrom;

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::BinaryHeap;
use std::cmp::Reverse;
use std::io::{Result, Error, ErrorKind};

/// Implementation-specific PDDB structures: for Precursor/Xous OS pair

pub(crate) const MBBB_PAGES: usize = 10;
pub(crate) const FSCB_PAGES: usize = 16;
pub(crate) const INITIAL_BASIS_ALLOC: usize = 16;

/// size of a physical page
pub const PAGE_SIZE: usize = spinor::SPINOR_ERASE_SIZE as usize;
/// size of a virtual page -- after the AES encryption and journaling overhead is subtracted
pub const VPAGE_SIZE: usize = PAGE_SIZE - size_of::<Nonce>() - size_of::<Tag>() - size_of::<JournalType>();

#[repr(C, packed)] // this can map directly into Flash
pub(crate) struct StaticCryptoData {
    /// aes-256 key of the system basis, encrypted with the User0 root key
    pub(crate) system_key: [u8; AES_KEYSIZE],
    /// a pool of fixed data used to pick salts, based on a hash of the basis name
    pub(crate) salt_base: [u8; 2048],
    /// also random data, but no specific purpose
    pub(crate) reserved: [u8; 2016],
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

pub(crate) struct PddbOs {
    spinor: spinor::Spinor,
    rootkeys: root_keys::RootKeys,
    pddb_mr: xous::MemoryRange,
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
    system_basis_key: Option<[u8; AES_KEYSIZE]>,
    /// virtual to physical page map. It's the reverse mapping of the physical page table on disk.
    v2p_map: HashMap<BasisRootName, HashMap<VirtAddr, PhysPage>>,
    /// fast space cache
    fspace_cache: HashSet<PhysPage>,
    /// memoize the location of the fscb log pages
    fspace_log_addrs: Vec::<PageAlignedPa>,
    /// memoize the current target offset for the next log entry
    fspace_log_next_addr: Option<PhysAddr>,
    /// a cached copy of the FPGA's DNA ID, used in the AAA records.
    dna: u64,
    /// reference to a TrngPool object that's shared among all the hardware functions
    entropy: Rc<RefCell<TrngPool>>,
}

impl PddbOs {
    pub fn new(trngpool: Rc<RefCell<TrngPool>>) -> PddbOs {
        let xns = xous_names::XousNames::new().unwrap();
        let pddb = xous::syscall::map_memory(
            xous::MemoryAddress::new(xous::PDDB_LOC as usize),
            None,
            PDDB_A_LEN as usize,
            xous::MemoryFlags::R,
        )
        .expect("Couldn't map the PDDB memory range");

        // the mbbb is located one page off from the Page Table
        let key_phys_base = PageAlignedPa::from(size_of::<PageTableInFlash>());
        let mbbb_phys_base = key_phys_base + PageAlignedPa::from(PAGE_SIZE);
        let fscb_phys_base = PageAlignedPa::from(mbbb_phys_base.as_u32() + MBBB_PAGES as u32 * PAGE_SIZE as u32);

        let llio = llio::Llio::new(&xns).unwrap();
        PddbOs {
            spinor: spinor::Spinor::new(&xns).unwrap(),
            rootkeys: root_keys::RootKeys::new(&xns, Some(AesRootkeyType::User0)).expect("FATAL: couldn't access RootKeys!"),
            pddb_mr: pddb,
            pt_phys_base: PageAlignedPa::from(0 as u32),
            key_phys_base,
            mbbb_phys_base,
            fscb_phys_base,
            data_phys_base: PageAlignedPa::from(fscb_phys_base.as_u32() + FSCB_PAGES as u32 * PAGE_SIZE as u32),
            system_basis_key: None,
            v2p_map: HashMap::<BasisRootName, HashMap<VirtAddr, PhysPage>>::new(),
            fspace_cache: HashSet::<PhysPage>::new(),
            fspace_log_addrs: Vec::<PageAlignedPa>::new(),
            fspace_log_next_addr: None,
            dna: llio.soc_dna().unwrap(),
            entropy: trngpool,
        }
    }

    /// patches data at an offset starting from the data physical base address, which corresponds
    /// exactly to the first entry in the page table
    fn patch_data(&self, data: &[u8], offset: u32) {
        assert!(data.len() + offset as usize <= PDDB_A_LEN - self.data_phys_base.as_usize(), "attempt to store past disk boundary");
        self.spinor.patch(
            self.pddb_mr.as_slice(),
            xous::PDDB_LOC,
            &data,
            offset + self.data_phys_base.as_u32(),
        ).expect("couldn't write to data region in the PDDB");
    }
    fn patch_pagetable(&self, data: &[u8], offset: u32) {
        assert!(data.len() + offset as usize <= size_of::<PageTableInFlash>(), "attempt to patch past page table end");
        self.spinor.patch(
            self.pddb_mr.as_slice(),
            xous::PDDB_LOC,
            &data,
            offset,
        ).expect("couldn't write to page table");
    }
    fn patch_keys(&self, data: &[u8], offset: u32) {
        assert!(data.len() + offset as usize <= PAGE_SIZE, "attempt to burn key data that is outside the key region");
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

    /// maps a StaticCryptoData structure into the key area of the PDDB.
    fn static_crypto_data_get(&self) -> &StaticCryptoData {
        let scd_ptr = self.key_phys_base.as_usize() as *const StaticCryptoData;
        let scd: &StaticCryptoData = unsafe{scd_ptr.as_ref().unwrap()};
        scd
    }
    /// takes the key and writes it with zero, using hard pointer math and a compiler fence to ensure
    /// the wipe isn't optimized out.
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
    }
    fn syskey_ensure(&mut self) {
        if self.system_basis_key.is_none() {
            let scd = self.static_crypto_data_get();
            let mut system_key: [u8; AES_KEYSIZE] = [0; AES_KEYSIZE];
            for (&src, dst) in scd.system_key.iter().zip(system_key.iter_mut()) {
                *dst = src;
            }
            self.rootkeys.decrypt_block(Block::from_mut_slice(&mut system_key));
            self.system_basis_key = Some(system_key);
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
            let ciphertext = cipher.encrypt(nonce, payload).expect("failed to encrypt FastSpace record");
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
        } else {
            panic!("invalid state!");
        }
    }

    /// WARNING: only call this function when all knows Basis have been unlocked, otherwise locked
    /// Basis will be marked in the freespace sweep for deletion.
    ///
    /// Sweeps through the entire set of known data (as loaded in v2p_map) and
    /// returns a subset of the total free space in a PhysPage vector that is a list of physical pages,
    /// in random order, that can be used by PDDB operations in the future without worry about
    /// accidentally overwriting Basis data that are locked.
    ///
    /// The function is coded to prioritize small peak memory footprint over speed, as it
    /// needs to run in a fairly memory-constrained environment, keeping in mind that if the PDDB
    /// structures were to be extended to run on say, an external USB drive with gigabytes of space,
    /// we cannot afford to naively allocate vectors that count every single page.
    fn fast_space_generate(&mut self) -> Vec::<PhysPage> {
        let mut free_pool = Vec::<usize>::new();
        let max_entries = FASTSPACE_PAGES * PAGE_SIZE / size_of::<PhysPage>();
        free_pool.reserve_exact(max_entries);
        // 1. scan through all of the known physical pages, and add them to a binary heap.
        //    WARNING: this could get really big for a very large filesystem. It's capped at ~100k for
        //    Precursor's ~100MiB storage increment.
        let mut page_heap = BinaryHeap::new();
        for (_, basismap) in &self.v2p_map {
            for (_, pp) in basismap {
                page_heap.push(Reverse(pp.page_number()));
            }
        }
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
        for page_candidate in 0..PDDB_A_LEN / PAGE_SIZE {
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

        // 4. ensure that the free pool stays within the defined deniability ratio
        let deniable_free_pages = (total_free_pages as f32 * FSCB_FILL_COEFFICIENT) as usize;
        // we're guarantede to have at least one free page, because we errored out if the pages was 0 above.
        let deniable_free_pages = if deniable_free_pages == 0 { 1 } else { deniable_free_pages };
        free_pool.truncate(deniable_free_pages);
        log::info!("free_pool after PD trim: {}; max pages allowed: {}", free_pool.len(), deniable_free_pages);

        // 5. Take the free_pool and annotate it for writing to disk
        let mut page_pool = Vec::<PhysPage>::new();
        for page in free_pool {
            let mut pp = PhysPage(0);
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

            // let fscb_slice = self.fscb_deref(); // can't use this line because it causse self to be immutably borrowed, so we write out the equivalent below.
            let fscb_slice = &self.pddb_mr.as_slice()[self.fscb_phys_base.as_usize()..self.fscb_phys_base.as_usize() + FSCB_PAGES * PAGE_SIZE];

            // 1. scan through the entire space, and look for the FastSpace record. It can be identified by the
            // first 16 (aes::BLOCK_SIZE) bytes not being all 1's.
            let not_aes: [u8; aes::BLOCK_SIZE] = [0xff; aes::BLOCK_SIZE];
            let mut fscb_pages = 0;
            let mut blank_pages = Vec::new();
            for page_start in (0..fscb_slice.len()).step_by(PAGE_SIZE) {
                if (fscb_slice[page_start..page_start + aes::BLOCK_SIZE] == not_aes)
                && (fscb_slice[page_start + aes::BLOCK_SIZE..page_start + aes::BLOCK_SIZE * 2] == not_aes) {
                    // page has met the criteria for being blank, skip to the next page
                    blank_pages.push(page_start);
                    continue
                } else if fscb_slice[page_start..page_start+aes::BLOCK_SIZE] == not_aes {
                    // this page contains update records; stash it for scanning after we've read in the master record
                    self.fspace_log_addrs.push(PageAlignedPa::from(page_start));
                    continue
                } else {
                    // this page (and the ones immediately afterward) "should" contain the FastSpace encrypted record
                    if fscb_pages == 0 {
                        let mut fscb_buf = [0; FSCB_PAGES * PAGE_SIZE - size_of::<Nonce>()];
                        // copy the encrypted data to the decryption buffer
                        for (&src, dst) in
                        fscb_slice[page_start + size_of::<Nonce>() .. page_start + size_of::<Nonce>() + FSCB_PAGES * PAGE_SIZE]
                        .iter().zip(fscb_buf.iter_mut()) {
                            *dst = src;
                        }
                        let mut aad = Vec::<u8>::new();
                        self.fast_space_aad(&mut aad);
                        let mut payload = Payload {
                            msg: &fscb_buf,
                            aad: &aad,
                        };
                        let key = Key::from_slice(&system_key);
                        let cipher = Aes256GcmSiv::new(key);
                        match cipher.decrypt(Nonce::from_slice(&fscb_slice[page_start..page_start + size_of::<Nonce>()]), payload) {
                            Ok(msg) => {
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
            assert!(fscb_pages == size_of::<FastSpace>());

            // 2. visit the update_page_addrs and modify the fspace_cache accordingly.
            let cipher = Aes256::new(GenericArray::from_slice(&system_key));
            let mut block = Block::default();
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
                                log::info!("Strange...we have a journal entry for a free space page that isn't already in our cache. Guru meditation: {:?}", pp);
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
                true
            } else {
                false
            }
        }
    }
    /// attempts to allocate a page out of the fspace cache (in RAM)
    /// If None is returned, try calling fast_space_generate() to create a new pool of FastSpace pages.
    pub fn try_fast_space_alloc(&mut self) -> Option<PhysPage> {
        // 1. Confirm that the fspace_log_next_addr is valid. If not, regenerate it, or fail.
        if !self.fast_space_ensure_next_log() {
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
                    ppc.set_journal(pp.journal() + 1); // this is guaranteed not to overflow because of a check in the "if" clause above

                    // commit the usage to the journal
                    self.syskey_ensure();
                    if let Some(system_key) = self.system_basis_key {
                        let cipher = Aes256::new(GenericArray::from_slice(&system_key));
                        let mut update = SpaceUpdate::new(self.entropy.borrow_mut().get_u64(), ppc);
                        let mut block = Block::from_mut_slice(update.deref_mut());
                        cipher.encrypt_block(block);
                        let log_addr = self.fspace_log_next_addr.take().unwrap() as PhysAddr;
                        self.patch_fscb(&block, log_addr);
                        let next_addr = log_addr + aes::BLOCK_SIZE as PhysAddr;
                        if (next_addr & (PAGE_SIZE as PhysAddr - 1)) != 0 {
                            self.fspace_log_next_addr = Some(next_addr as PhysAddr);
                        } else {
                            // fspace_log_next_addr is already None because we used "take()". We'll find a free spot for the
                            // next journal entry the next time around.
                        }

                    } else {
                        panic!("Inconsistent internal state");
                    }
                    maybe_alloc = Some(ppc);
                    break;
                }
            }
            if let Some(alloc) = maybe_alloc {
                assert!(self.fspace_cache.replace(alloc).is_some(), "inconsistent state: we found a free page, but later when we tried to update it, it wasn't there!");
            }
            maybe_alloc
        }
    }
    /// This is the "try really hard" version of fast_space alloc. If the easy path doesn't work,
    /// It will try to rescan the entire system and allocate a new FastSpace cache.
    ///
    /// If this returns None, we really are out of free space, *or* the user has cancelled out of the
    /// "unlock all Basis" request and the system will act like it's out of free space.
    pub fn fast_space_alloc(&mut self) -> Option<PhysPage> {
        match self.try_fast_space_alloc() {
            Some(alloc) => Some(alloc),
            None => {
                if self.basis_request_unlock_all() {
                    let free_pool = self.fast_space_generate();
                    if free_pool.len() == 0 {
                        // we're out of free space
                        None
                    } else {
                        let mut fast_space = FastSpace {
                            free_pool: [PhysPage(0); FASTSPACE_FREE_POOL_LEN],
                        };
                        for (&src, dst) in free_pool.iter().zip(fast_space.free_pool.iter_mut()) {
                            *dst = src;
                        }
                        // write just commits a new record to disk, but doesn't update our internal data cache
                        self.fast_space_write(&fast_space);
                        // this will ensure the data cache is fully in sync
                        self.fast_space_read();

                        // retry the alloc -- this /should/ return a Some, but if it doesn't...we're out of space!
                        self.try_fast_space_alloc()
                    }
                } else {
                    None
                }
            }
        }
    }
    /// This function will prompt the user to unlock all the Basis. If the user asserts all
    /// Basis have been unlocked, the function returns `true`. The other option is the user
    /// can decline to unlock all the Basis right now, cancelling out of the process, which will
    /// cause the requesting free space sweep to fail.
    pub(crate) fn basis_request_unlock_all(&self) -> bool {
        // this function is a morass of UX code that has to be written. Let's save it until later,
        // once we've got some core functionality in the PDDB; or for testing, we could just "return true"
        // and YOLO it.
        unimplemented!();
    }

    /// this function is dangerous in that calling it will completely erase all of the previous data
    /// in the PDDB an replace it with a brand-spanking new, blank PDDB.
    /// The number of servers that can connect to the Spinor crate is strictly tracked, so we borrow a reference
    /// to the Spinor object allocated to the PDDB implementation for this operation.
    pub(crate) fn pddb_format(&mut self) -> Result<()> {
        if !self.rootkeys.is_initialized().unwrap() {
            return Err(Error::new(ErrorKind::Unsupported, "Root keys are not initialized; cannot format a PDDB without root keys!"));
        }
        // step 1. Erase the entire PDDB region - leaves the state in all 1's
        {
            log::info!("Erasing the PDDB region");
            let blank_sector: [u8; PAGE_SIZE] = [0xff; PAGE_SIZE];

            // there is no convenience routine for erasing the entire disk. Maybe that's a good thing?
            for offset in (0..PDDB_A_LEN).step_by(PAGE_SIZE) {
                if (offset / PAGE_SIZE) % 64 == 0 {
                    log::info!("Initial erase: {}/{}", offset, PDDB_A_LEN);
                }
                self.spinor.patch(
                    self.pddb_mr.as_slice(),
                    xous::PDDB_LOC,
                    &blank_sector,
                    offset as u32
                ).expect("couldn't erase memory");
            }
        }

        // step 2. fill in the page table with junk, which marks it as cryptographically empty
        let mut temp: [u8; PAGE_SIZE] = [0; PAGE_SIZE];
        for page in (0..size_of::<PageTableInFlash>()).step_by(PAGE_SIZE) {
            self.entropy.borrow_mut().get_slice(&mut temp);
            self.patch_pagetable(&temp, page as u32);
        }
        if size_of::<PageTableInFlash>() & (PAGE_SIZE - 1) != 0 {
            let remainder_start = size_of::<PageTableInFlash>() & !(PAGE_SIZE - 1);
            log::info!("Page table does not end on a page boundary. Handling trailing page case of {} bytes", remainder_start);
            let mut temp = Vec::<u8>::new();
            for _ in remainder_start..size_of::<PageTableInFlash>() {
                temp.push(self.entropy.borrow_mut().get_u8());
            }
            self.patch_pagetable(&temp, remainder_start as u32);
        }

        // step 3. create our key material
        // consider: making ensure_aes_password() a pub-scoped function? let's see how this works in practice.
        //if !self.rootkeys.ensure_aes_password() {
        //    return Err(Error::new(ErrorKind::PermissionDenied, "unlock password was incorrect"));
        //}
        assert!(size_of::<StaticCryptoData>() == PAGE_SIZE, "StaticCryptoData structure is not correctly sized");
        let mut system_basis_key: [u8; AES_KEYSIZE] = [0; AES_KEYSIZE];
        self.entropy.borrow_mut().get_slice(&mut system_basis_key);
        let mut basis_key_enc: [u8; AES_KEYSIZE] = system_basis_key.clone();
        self.system_basis_key = Some(system_basis_key); // causes system_basis_key to be owned by self
        log::info!("sanity check: plaintext system basis key: {:x?}", basis_key_enc);
        self.rootkeys.encrypt_block(Block::from_mut_slice(&mut basis_key_enc));
        log::info!("sanity check: encrypted system basis key: {:x?}", basis_key_enc);
        let mut crypto_keys = StaticCryptoData {
            system_key: [0; AES_KEYSIZE],
            salt_base: [0; 2048],
            reserved: [0; 2016],
        };
        // copy the encrypted key into the data structure for commit to Flash
        for (&src, dst) in basis_key_enc.iter().zip(crypto_keys.system_key.iter_mut()) {
            *dst = src;
        }
        self.entropy.borrow_mut().get_slice(&mut crypto_keys.salt_base);
        self.entropy.borrow_mut().get_slice(&mut crypto_keys.reserved);
        self.patch_keys(crypto_keys.deref(), 0);
        // now we have a copy of the AES key necessary to encrypt the default System basis that we created in step 2.

        // step 4. mbbb handling
        // mbbb should just be blank at this point, and the flash was erased in step 1, so there's nothing to do.

        // step 5. fscb handling
        // pick a set of random pages from the free pool and assign it to the fscb
        let free_pool = self.fast_space_generate();
        let mut fast_space = FastSpace {
            free_pool: [PhysPage(0); FASTSPACE_FREE_POOL_LEN],
        };
        for (&src, dst) in free_pool.iter().zip(fast_space.free_pool.iter_mut()) {
            *dst = src;
        }
        self.fast_space_write(&fast_space);

        // step 5. salt the free space with random numbers. this can take a while, we might need a "progress report" of some kind...
        // this is coded using "direct disk" offsets...under the assumption that we only ever really want to do this here, and
        // not re-use this routine elsewhere.
        for offset in (self.data_phys_base.as_usize()..PDDB_A_LEN).step_by(PAGE_SIZE) {
            self.entropy.borrow_mut().get_slice(&mut temp);
            if (offset / PAGE_SIZE) % 64 == 0 {
                log::info!("Crytpographic 'erase': {}/{}", offset, PDDB_A_LEN);
            }
            self.spinor.patch(
                self.pddb_mr.as_slice(),
                xous::PDDB_LOC,
                &temp,
                offset as u32
            ).expect("couldn't fill in disk with random datax");
        }

        // step 6. create the system basis root structure
        let mut name: [u8; PDDB_MAX_BASIS_NAME_LEN] = [0; PDDB_MAX_BASIS_NAME_LEN];
        for (&src, dst) in PDDB_DEFAULT_SYSTEM_BASIS.as_bytes().iter().zip(name.iter_mut()) {
            *dst = src;
        }
        let mut basis_root = BasisRoot {
            magic: api::PDDB_MAGIC,
            version: api::PDDB_VERSION,
            name,
            age: 0,
            num_dictionaries: 0,
            prealloc_open_end: PageAlignedVa::from(INITIAL_BASIS_ALLOC * VPAGE_SIZE),
            dict_ptr: None,
        };

        // step 7. Create a hashmap for our reverse PTE, allocate sectors, and add it to the Pddb's cache
        self.fast_space_read(); // we reconstitute our fspace map even though it was just generated, partially as a sanity check that everything is ok

        { // this bit of code could be the start of an "allocate space for basis" routine, but we need to think about how overwritten pages are handled...
            let mut basis_v2p_map = HashMap::<VirtAddr, PhysPage>::new();
            for virt_page in 1..=basis_root.prealloc_open_end.as_vpage_num() {
                if let Some(alloc) = self.fast_space_alloc() {
                    let mut rpte = alloc.clone();
                    rpte.set_clean(true); // it's not clean _right now_ but it will be by the time this routine is done...
                    rpte.set_valid(true);
                    basis_v2p_map.insert(VirtAddr::new((virt_page * VPAGE_SIZE) as u64).unwrap(), rpte);
                }
            }
            self.v2p_map.insert(basis_root.name, basis_v2p_map);
        }

        // step 8. write the System basis to Flash, at the physical locations noted above
        if let Some(syskey) = self.system_basis_key {
            let key = Key::from_slice(&system_basis_key);
            let cipher = Aes256GcmSiv::new(key);
            let basis_encryptor = BasisEncryptor::new(
                &basis_root,
                &[],
                self.dna,
                cipher,
                0,
                Rc::clone(&self.entropy),
            );
            if let Some(basis_v2p_map) = self.v2p_map.get(&basis_root.name) {
                for (vpage_no, ppage_data) in basis_encryptor.into_iter().enumerate() {
                    let vaddr = VirtAddr::new( ((vpage_no + 1) * VPAGE_SIZE) as u64 ).unwrap();
                    match basis_v2p_map.get(&vaddr) {
                        Some(pp) => {
                            self.patch_data(&ppage_data, pp.page_number() * PAGE_SIZE as u32);
                        }
                        None => {
                            log::error!("Previously allocated page was not found in our map!");
                            panic!("Inconsistent internal state");
                        }
                    }
                }
            } else {
                log::error!("Couldn't find the system basis!");
                panic!("Inconsistent internal state");
            }
        } else {
            log::error!("System key was not found, but it should be present!");
            panic!("Inconsistent internal state");
        }

        // step 9. generate & write initial page table entries
        // page table organization:
        //
        //   offset from |
        //   pt_phys_base|  contents  (example for total PDDB len of 0x6f8_0000 or 111 MiB)
        //   ------------|---------------------------------------------------
        //   0x0000_0000 |  virtual map for page at (0x0000 + data_phys_base)
        //   0x0000_0010 |  virtual map for page at (0x1000 + data_phys_base)
        //   0x0000_0020 |  virtual map for page at (0x2000 + data_phys_base)
        //    ...
        //   0x0006_F7F0 |  virtual map for page at (0x06F7_F000 + data_phys_base)
        //   0x0006_F800 |  unused
        //    ...
        //   0x0007_0000 |  key page
        //    ...
        //   0x0007_1000 |  mbbb start (example of 10 pages)
        //    ...
        //   0x0007_B000 |  fscb start (example of 10 pages)
        //    ...
        //   0x0008_5000 |  data_phys_base - start of basis + dictionary + key data region


        Ok(())
    }
}