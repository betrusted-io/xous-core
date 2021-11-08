use std::convert::TryInto;

use crate::*;
use aes_gcm_siv::{Aes256GcmSiv, Nonce, Key, Tag};
use aes_gcm_siv::aead::{Aead, NewAead, Payload};
use rand_core::{CryptoRng, RngCore};
use cipher::{BlockCipher, BlockDecrypt, BlockEncrypt};
use root_keys::api::{AesRootkeyType, Block};
use core::ops::Deref;
use core::convert::TryFrom;

use std::collections::HashMap;
use std::collections::BinaryHeap;
use std::cmp::Reverse;
use std::io::{Result, Error, ErrorKind};

/// Implementation-specific PDDB structures: for Precursor/Xous OS pair

pub(crate) const MBBB_PAGES: usize = 10;
pub(crate) const FSCB_PAGES: usize = 16;
pub(crate) const INITIAL_BASIS_ALLOC: usize = 16;

pub const PAGE_SIZE: usize = spinor::SPINOR_ERASE_SIZE as usize;

#[repr(C, packed)] // this can map directly into Flash
pub(crate) struct StaticCryptoData {
    /// aes-256 key of the system basis, encrypted with the User0 root key
    pub(crate) system_key: [u8; 32],
    /// a pool of fixed data used to pick salts, based on a hash of the basis name
    pub(crate) salt_base: [u8; 2048],
    /// also random data, but no specific purpose
    pub(crate) reserved: [u8; 2016],
}
impl Deref for StaticCryptoData {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self as *const StaticCryptoData as *const u8, core::mem::size_of::<StaticCryptoData>())
                as &[u8]
        }
    }
}

pub(crate) struct PddbOs {
    spinor: spinor::Spinor,
    rootkeys: root_keys::RootKeys,
    pddb_mr: xous::MemoryRange,
    trng: trng::Trng,
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
    system_basis_key: Option<[u8; 32]>,
    v2p_map: HashMap<BasisRootName, HashMap<VirtAddr, PhysPage>>,
    /// The PDDB eats a lot of entropy. Keep a local pool of entropy, so we're not wasting a lot of
    /// overhead passing messages to the TRNG.
    e_cache: Vec::<u8>,
    /// a cached copy of the FPGA's DNA ID, used in the AAA records.
    dna: u64,
}

impl PddbOs {
    pub fn new() -> PddbOs {
        let xns = xous_names::XousNames::new().unwrap();
        let pddb = xous::syscall::map_memory(
            xous::MemoryAddress::new(xous::PDDB_LOC as usize),
            None,
            PDDB_A_LEN as usize,
            xous::MemoryFlags::R,
        )
        .expect("Couldn't map the PDDB memory range");

        // the mbbb is located one page off from the Page Table
        let key_phys_base = PageAlignedPa::from(core::mem::size_of::<PageTableInFlash>());
        let mbbb_phys_base = key_phys_base + PageAlignedPa::from(PAGE_SIZE);
        let fscb_phys_base = PageAlignedPa::from(mbbb_phys_base.as_u32() + MBBB_PAGES as u32 * PAGE_SIZE as u32);

        let mut trng = trng::Trng::new(&xns).unwrap();
        let mut cache: [u8; 8192] = [0; 8192];
        trng.fill_bytes(&mut cache);

        let llio = llio::Llio::new(&xns).unwrap();
        PddbOs {
            spinor: spinor::Spinor::new(&xns).unwrap(),
            rootkeys: root_keys::RootKeys::new(&xns, Some(AesRootkeyType::User0)).expect("FATAL: couldn't access RootKeys!"),
            pddb_mr: pddb,
            trng,
            pt_phys_base: PageAlignedPa::from(0 as u32),
            key_phys_base,
            mbbb_phys_base,
            fscb_phys_base,
            data_phys_base: PageAlignedPa::from(fscb_phys_base.as_u32() + FSCB_PAGES as u32 * PAGE_SIZE as u32),
            system_basis_key: None,
            v2p_map: HashMap::<BasisRootName, HashMap<VirtAddr, PhysPage>>::new(),
            e_cache: cache.to_vec(),
            dna: llio.soc_dna().unwrap(),
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
    /// see write_fast_space()
    fn patch_fscb(&self, data: &[u8], offset: u32) {
        assert!(data.len() + offset as usize <= PAGE_SIZE * FSCB_PAGES, "fscb patch would go out of bounds");
        self.spinor.patch(
            self.pddb_mr.as_slice(),
            xous::PDDB_LOC,
            data,
            self.fscb_phys_base.as_u32() + offset
        ).expect("couldn't burn fscb");
    }
    /// anytime the fscb is updated, all the partial records are nuked, as well as any existing record.
    /// then, a _random_ location is picked to place the structure to help with wear levelling.
    fn write_fast_space(&mut self, fs: &FastSpace) {
        self.ensure_system_key();
        if let Some(system_basis_key) = self.system_basis_key {
            let key = Key::from_slice(&system_basis_key);
            let cipher = Aes256GcmSiv::new(key);
            let nonce_array = self.gen_nonce();
            let nonce = Nonce::from_slice(&nonce_array);
            let fs_ser: &[u8] = fs.deref();
            assert!( ((fs_ser.len() + core::mem::size_of::<Nonce>() + core::mem::size_of::<Tag>()) & (PAGE_SIZE - 1)) == 0,
                "FastSpace record is not page-aligned in size!");
            // create AAD: name, version number, and FPGA ID.
            let mut aad = Vec::<u8>::new();
            aad.extend_from_slice(PDDB_FAST_SPACE_SYSTEM_BASIS.as_bytes());
            aad.extend_from_slice(&PDDB_VERSION.to_le_bytes());
            aad.extend_from_slice(&self.dna.to_le_bytes());
            // AAD + data => Payload
            let payload = Payload {
                msg: fs_ser,
                aad: &aad,
            };
            let ciphertext = cipher.encrypt(nonce, payload).expect("failed to encrypt FastSpace record");
            let ct_to_flash = ciphertext.deref();
            // determine which page we're going to write the ciphertext into
            let page_search_limit = FSCB_PAGES - ((PageAlignedPa::from(ciphertext.len()).as_usize() / PAGE_SIZE) - 1);
            log::info!("picking a random page out of {} pages for fscb", page_search_limit);
            let dest_page = self.trng_cache_u32() % page_search_limit as u32;
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

    /// maps a StaticCryptoData structure into the key area of the PDDB.
    fn get_static_crypto_data(&self) -> &StaticCryptoData {
        let scd_ptr = self.key_phys_base.as_usize() as *const StaticCryptoData;
        let scd: &StaticCryptoData = unsafe{scd_ptr.as_ref().unwrap()};
        scd
    }
    /// takes the key and writes it with zero, using hard pointer math and a compiler fence to ensure
    /// the wipe isn't optimized out.
    fn erase_system_key(&mut self) {
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
    fn ensure_system_key(&mut self) {
        if self.system_basis_key.is_none() {
            let scd = self.get_static_crypto_data();
            let mut system_key: [u8; 32] = [0; 32];
            for (&src, dst) in scd.system_key.iter().zip(system_key.iter_mut()) {
                *dst = src;
            }
            self.rootkeys.decrypt_block(Block::from_mut_slice(&mut system_key));
            self.system_basis_key = Some(system_key);
        }
    }
    fn ensure_entropy(&mut self, amount: usize) {
        if self.e_cache.len() < amount {
            let mut cache: [u8; 8192] = [0; 8192];
            self.trng.fill_bytes(&mut cache);
            self.e_cache.extend_from_slice(&cache);
        }
    }
    fn trng_cache_u8(&mut self) -> u8 {
        self.ensure_entropy(1);
        self.e_cache.pop().unwrap()
    }
    fn trng_cache_u32(&mut self) -> u32 {
        self.ensure_entropy(4);
        let ret = u32::from_le_bytes(self.e_cache[self.e_cache.len() - 4..].try_into().unwrap());
        self.e_cache.truncate(self.e_cache.len() - 4);
        ret
    }
    fn trng_cache_u64(&mut self) -> u64 {
        self.ensure_entropy(8);
        let ret = u64::from_le_bytes(self.e_cache[self.e_cache.len() - 8..].try_into().unwrap());
        self.e_cache.truncate(self.e_cache.len() - 8);
        ret
    }
    fn trng_cache_slice(&mut self, bucket: &mut [u8]) {
        self.ensure_entropy(bucket.len());
        for (src, dst) in self.e_cache.drain(
            (self.e_cache.len() - bucket.len())..
        ).zip(bucket.iter_mut()) {
            *dst = src;
        }
    }
    /// generates a 96-bit nonce using the CPRNG
    pub fn gen_nonce(&mut self) -> [u8; 12] {
        let mut nonce: [u8; 12] = [0; 12];
        self.trng_cache_slice(&mut nonce);
        nonce
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
    fn collect_fastspace(&mut self) -> Vec::<PhysPage> {
        let mut free_pool = Vec::<usize>::new();
        let max_entries = FASTSPACE_PAGES * PAGE_SIZE / core::mem::size_of::<PhysPage>();
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
                let index = self.trng_cache_u32() as usize % free_pool.len();
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
            free_pool.swap(i, self.trng_cache_u32() as usize % (i+1));
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

    /// this function is dangerous in that calling it will completely erase all of the previous data
    /// in the PDDB an replace it with a brand-spanking new, blank PDDB.
    /// The number of servers that can connect to the Spinor crate is strictly tracked, so we borrow a reference
    /// to the Spinor object allocated to the PDDB implementation for this operation.
    pub(crate) fn format_pddb(&mut self) -> Result<()> {
        if !self.rootkeys.is_initialized().unwrap() {
            return Err(Error::new(ErrorKind::Unsupported, "Root keys are not initialized; cannot format a PDDB without root keys!"));
        }
        // step 1. Erase the entire PDDB region.
        log::info!("Erasing the PDDB region");
        let blank_sector: [u8; PAGE_SIZE] = [0xff; PAGE_SIZE];

        // there is no convenience routine for erasing the entire disk. Maybe that's a good thing?
        for offset in (0..PDDB_A_LEN).step_by(PAGE_SIZE) {
            self.spinor.patch(
                self.pddb_mr.as_slice(),
                xous::PDDB_LOC,
                &blank_sector,
                offset as u32
            ).expect("couldn't erase memory");
        }

        // step 2. create our key material
        // consider: making ensure_aes_password() a pub-scoped function? let's see how this works in practice.
        //if !self.rootkeys.ensure_aes_password() {
        //    return Err(Error::new(ErrorKind::PermissionDenied, "unlock password was incorrect"));
        //}
        assert!(core::mem::size_of::<StaticCryptoData>() == PAGE_SIZE, "StaticCryptoData structure is not correctly sized");
        let mut system_basis_key: [u8; 32] = [0; 32];
        self.trng.fill_bytes(&mut system_basis_key);
        let mut basis_key_enc: [u8; 32] = system_basis_key.clone();
        self.system_basis_key = Some(system_basis_key); // causes system_basis_key to be owned by self
        log::info!("sanity check: plaintext system basis key: {:x?}", basis_key_enc);
        self.rootkeys.encrypt_block(Block::from_mut_slice(&mut basis_key_enc));
        log::info!("sanity check: encrypted system basis key: {:x?}", basis_key_enc);
        let mut crypto_keys = StaticCryptoData {
            system_key: [0; 32],
            salt_base: [0; 2048],
            reserved: [0; 2016],
        };
        // copy the encrypted key into the data structure for commit to Flash
        for (&src, dst) in basis_key_enc.iter().zip(crypto_keys.system_key.iter_mut()) {
            *dst = src;
        }
        self.trng.fill_bytes(&mut crypto_keys.salt_base);
        self.trng.fill_bytes(&mut crypto_keys.reserved);
        self.patch_keys(crypto_keys.deref(), 0);
        // now we have a copy of the AES key necessary to encrypt the default System basis that we created in step 2.

        // step 3. mbbb handling
        // mbbb should just be blank at this point, and the flash was erased in step 1, so there's nothing to do.

        // step 4. fscb handling
        // pick a set of random pages from the free pool and assign it to the fscb
        let free_pool = self.collect_fastspace();
        let mut fast_space = FastSpace {
            free_pool: [PhysPage(0); FASTSPACE_FREE_POOL_LEN],
        };
        for (&src, dst) in free_pool.iter().zip(fast_space.free_pool.iter_mut()) {
            *dst = src;
        }
        self.write_fast_space(&fast_space);

        // step 2. create the system basis root structure
        let mut name: [u8; PDDB_MAX_BASIS_NAME_LEN] = [0; PDDB_MAX_BASIS_NAME_LEN];
        for (&src, dst) in PDDB_DEFAULT_SYSTEM_BASIS.as_bytes().iter().zip(name.iter_mut()) {
            *dst = src;
        }
        let mut basis_root = BasisRoot {
            p_nonce: self.gen_nonce(),
            magic: api::PDDB_MAGIC,
            version: api::PDDB_VERSION,
            journal_rev: 0,
            name,
            age: 0,
            num_dictionaries: 0,
            prealloc_open_end: PageAlignedVa::from(INITIAL_BASIS_ALLOC * PAGE_SIZE),
        };
        // extract a slice-u8 that maps onto the basis_root record, allowing us to patch this into a FLASH page
        let br_slice: &[u8] = basis_root.deref();


        // step 4. Create a hashmap for our reverse PTE, and add it to the Pddb's cache
        // we don't have a fscb yet, and everything is free space, so we will manually place these initial entries.
        let mut basis_v2p_map = HashMap::<VirtAddr, PhysPage>::new();
        for (virt_page, phys_addr) in (
            self.data_phys_base.as_u32()..self.data_phys_base.as_u32() + basis_root.prealloc_open_end.as_u32()
        ).step_by(PAGE_SIZE).enumerate() {
            let mut rpte = PhysPage(0);
            rpte.set_page_number(phys_addr / PAGE_SIZE as u32);
            rpte.set_clean(true);
            rpte.set_valid(true);
            basis_v2p_map.insert((virt_page * PAGE_SIZE) as VirtAddr, rpte);
        }
        self.v2p_map.insert(basis_root.name, basis_v2p_map);

        // step 5. write the System basis to Flash, at the physical locations noted above
        let mut basis_ser = vec![];
        for &b in br_slice {
            basis_ser.push(b)
        }
        for _ in 0..basis_root.padding_count() {
            basis_ser.push(0)
        }
        // basis_ser can now be passed to an encryption function
        if let Some(system_basis_key) = self.system_basis_key {
            let key = Key::from_slice(&system_basis_key);
            let cipher = Aes256GcmSiv::new(key);
            let nonce_array = self.gen_nonce();
            let nonce = Nonce::from_slice(&nonce_array);
            let ciphertext = cipher.encrypt(nonce, &basis_ser[12..]);

            let ct_to_flash: &[u8] = ciphertext.as_ref().unwrap(); // this now contains the encrypted basis + 16-byte tag at the very end
            assert!( ( ct_to_flash.len() + basis_root.p_nonce.len()) & (PAGE_SIZE - 1) == 0, "Padding failure during basis serialization!");
            // we're now ready to write the encrypted basis to Flash.
            self.patch_data(&[&nonce_array, ct_to_flash].concat(), 0);

            // now fill in the rest of Flash with random data. This includes filling in the current Basis allocation
            // with random data, as that is the "free" state (as well as, at least facially, the "used" state)
            let start_offset = PageAlignedPa::from(ct_to_flash.len() + basis_root.p_nonce.len());
            let mut erase_buf: [u8; 4096] = [0; 4096];
            for page_offset in (start_offset.as_u32()..PDDB_A_LEN as u32).step_by(PAGE_SIZE) {
                self.trng.fill_bytes(&mut erase_buf);
                self.patch_data(&erase_buf, page_offset);
            }
        } else {
            panic!("invalid state"); // we should never hit this because we created the key earlier in the same routine.
        }

        // step 8. generate & write initial page table entries
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