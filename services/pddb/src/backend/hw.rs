use std::convert::TryInto;

use crate::*;
use aes_gcm_siv::{Aes256GcmSiv, Nonce, Key};
use aes_gcm_siv::aead::{NewAead, Aead};
use rand_core::{CryptoRng, RngCore};
use cipher::{BlockCipher, BlockDecrypt};
use root_keys::api::{AesRootkeyType, Block};
use core::ops::Deref;
use core::convert::TryFrom;

use std::collections::HashMap;
use std::io::{Result, Error, ErrorKind};

/// Implementation-specific PDDB structures: for Precursor/Xous OS pair

pub(crate) const MBBB_PAGES: usize = 10;
pub(crate) const FSCB_PAGES: usize = 10;
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
    pt_phys_base: PageAlignedU32,
    /// local key store -- one page, to store exactly one key, used for the system basis.
    /// the rest of the keys are generated on the fly entirely from the user password + a salt also stored in this page
    key_phys_base: PageAlignedU32,
    /// make before break buffer base -- location in FLASH, offset from physical bottom of pddb_mr
    mbbb_phys_base: PageAlignedU32,
    /// free space circular buffer base -- location in FLASH, offset from physical bottom of pddb_mr
    fscb_phys_base: PageAlignedU32,
    data_phys_base: PageAlignedU32,
    system_basis_key: Option<[u8; 32]>,
    v2p_map: HashMap<BasisRootName, HashMap<VirtAddr, PhysPage>>,
}

impl PddbOs {
    pub fn new() -> PddbOs {
        let xns = xous_names::XousNames::new().unwrap();
        let pddb = xous::syscall::map_memory(
            xous::MemoryAddress::new(xous::PDDB_LOC as usize),
            None,
            xous::PDDB_LEN as usize,
            xous::MemoryFlags::R,
        )
        .expect("Couldn't map the PDDB memory range");

        // the mbbb is located one page off from the Page Table
        let key_phys_base = PageAlignedU32::from(core::mem::size_of::<PageTableInFlash>());
        let mbbb_phys_base = key_phys_base + PageAlignedU32::from(PAGE_SIZE);
        let fscb_phys_base = PageAlignedU32::from(mbbb_phys_base.as_u32() + MBBB_PAGES as u32 * PAGE_SIZE as u32);
        PddbOs {
            spinor: spinor::Spinor::new(&xns).unwrap(),
            rootkeys: root_keys::RootKeys::new(&xns, Some(AesRootkeyType::User0)).expect("FATAL: couldn't access RootKeys!"),
            pddb_mr: pddb,
            trng: trng::Trng::new(&xns).unwrap(),
            pt_phys_base: PageAlignedU32::from(0 as u32),
            key_phys_base,
            mbbb_phys_base,
            fscb_phys_base,
            data_phys_base: PageAlignedU32::from(fscb_phys_base.as_u32() + FSCB_PAGES as u32 * PAGE_SIZE as u32),
            system_basis_key: None,
            v2p_map: HashMap::<BasisRootName, HashMap<VirtAddr, PhysPage>>::new(),
        }
    }

    /// generates a 96-bit nonce using the CPRNG
    pub fn gen_nonce(&mut self) -> [u8; 12] {
        let mut nonce: [u8; 12] = [0; 12];
        self.trng.fill_bytes(&mut nonce);
        nonce
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

        for offset in (0..xous::PDDB_LEN).step_by(PAGE_SIZE) {
            self.spinor.patch(
                self.pddb_mr.as_slice(),
                xous::PDDB_LOC,
                &blank_sector,
                offset
            ).expect("couldn't erase memory");
        }

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
            prealloc_open_end: PageAlignedU64::from(INITIAL_BASIS_ALLOC * PAGE_SIZE),
        };
        // extract a slice-u8 that maps onto the basis_root record, allowing us to patch this into a FLASH page
        let br_slice: &[u8] = basis_root.deref();

        // step 3. create our key material
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
        self.rootkeys.decrypt_block(Block::from_mut_slice(&mut basis_key_enc));
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
        self.spinor.patch(
            self.pddb_mr.as_slice(),
            xous::PDDB_LOC,
            crypto_keys.deref(),
            self.key_phys_base.as_u32()
        ).expect("couldn't burn keys");
        // now we have a copy of the AES key necessary to encrypt the default System basis that we created in step 2.

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

        // step 5. write the basis to Flash, at the physical locations noted above
        let mut basis_ser = vec![];
        for &b in br_slice {
            basis_ser.push(b)
        }
        for _ in (0..basis_root.padding_count()) {
            basis_ser.push(0)
        }
        // basis_ser can now be passed to an encryption function
        if let Some(system_basis_key) = self.system_basis_key {
            let key = Key::from_slice(&system_basis_key);
            let cipher = Aes256GcmSiv::new(key);
            let nonce = Nonce::from_slice(&basis_root.p_nonce);
            let ciphertext = cipher.encrypt(nonce, &basis_ser[12..]);

            let ct_to_flash: &[u8] = ciphertext.as_ref().unwrap(); // this now contains the encrypted basis + 16-byte tag at the very end
            assert!( ( ct_to_flash.len() + basis_root.p_nonce.len()) & (PAGE_SIZE - 1) == 0, "Padding failure during basis serialization!");
            // we're now ready to write the encrypted basis to Flash.
            self.spinor.patch(
                self.pddb_mr.as_slice(),
                xous::PDDB_LOC,
                &[&basis_root.p_nonce, ct_to_flash].concat(),
                self.data_phys_base.as_u32()
            ).expect("couldn't write basis structure");
            // now fill in the rest of Flash with random data. Because the rest of the Basis is blank, random data is "just fine".
            // LEFT OFF HERE
        } else {
            panic!("invalid state"); // we should never hit this because we created the key earlier in the same routine.
        }

        // step 6. generate & write initial page table entries
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