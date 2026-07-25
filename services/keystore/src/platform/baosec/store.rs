use aes::Aes256;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray};
use aes_gcm_siv::{AeadInPlace, Aes256GcmSiv, Nonce, Tag};
use bao1x_api::{
    BOOT0_PUBKEY_FAIL, BoardTypeCoding, BootWaitCoding, CP_ID, DEVELOPER_MODE, OEM_MODE,
    SLOT_ELEMENT_LEN_BYTES, UUID, signatures::SWAP_VERSION, signatures::SwapSourceHeader,
};
use bao1x_hal::board::{BOOKEND_END, BOOKEND_START, SWAP_KEY};
use bao1x_hal::udma::FLASH_SECTOR_LEN;
use bao1x_hal::{
    acram::{OneWayCounter, SlotManager},
    board::{CHAFF_KEYS, COLLATERAL, NUISANCE_KEYS_0, NUISANCE_KEYS_1, ROOT_SEED, THE_FLAG_1},
    rram::Reram,
};
use hkdf::Hkdf;
use keystore_api::KeyWrapper;
use rand::prelude::*;
use sha2::Sha256;
use xous::MemoryRange;

use crate::*;

const KEY_LEN: usize = bao1x_api::SLOT_ELEMENT_LEN_BYTES;

pub struct KeyStore {
    slot_mgr: SlotManager,
    pub owc: OneWayCounter,
    master_key: Option<[u8; KEY_LEN]>,
    swap_range: MemoryRange,
}

impl KeyStore {
    pub fn init_mappings(rram: &mut Reram) -> Self {
        let slot_mgr = SlotManager::new();
        slot_mgr.register_mapping(rram);
        let owc = OneWayCounter::new();
        owc.register_mapping(rram);

        let swap_range = xous::syscall::map_memory(
            None,
            // by requesting the offset into MMAP_VIRT_BASE, we (a) ensure the offset of the virtual
            // pages maintain a 1:1 correspondence with the offsets in SPI flash and (b)
            // by simply dereferencing any slices derived from virtual area we get a correct pointer
            // that we can pass directly into the swapper to update the correct sector of memory.
            xous::MemoryAddress::new(xous::arch::MMAP_VIRT_BASE),
            bao1x_hal::board::SWAP_FLASH_RESERVED_LEN as usize,
            xous::MemoryFlags::R | xous::MemoryFlags::VIRT,
        )
        .expect("Couldn't map the swap memory range");

        Self { slot_mgr, owc, master_key: None, swap_range }
    }

    fn swap_slice(&self) -> &[u8] {
        // safety: all values are representable in a u8
        unsafe { self.swap_range.as_slice() }
    }

    fn system_init_inner(&mut self, rram: &mut Reram) {
        // one routine works for both dabao and baosec
        let board_type = self.owc.get_decoded::<BoardTypeCoding>().unwrap();

        let xns = xous_names::XousNames::new().unwrap();
        let mut trng = bao1x_hal_service::trng::Trng::new(&xns).unwrap();
        // generate all the keys
        let key_set = if board_type == bao1x_api::BoardTypeCoding::Baosec {
            &bao1x_api::baosec::KEY_SLOTS[..]
        } else {
            &bao1x_api::dabao::KEY_SLOTS[..]
        };
        let mut success = true;
        for key_range in key_set.iter() {
            if *key_range == THE_FLAG_1 {
                // don't overwrite the flag, it's pre-loaded from static data
                continue;
            }
            let mut storage = Vec::<u8>::with_capacity(key_range.len() * SLOT_ELEMENT_LEN_BYTES);
            storage.resize(key_range.len() * SLOT_ELEMENT_LEN_BYTES, 0);
            trng.fill_bytes(&mut storage);
            match self.slot_mgr.write(rram, key_range, &storage) {
                Ok(_) => {}
                Err(e) => {
                    success = false;
                    log::error!("Couldn't initialize slot {:?}: {:?}", key_range, e);
                }
            }
        }
        // once all values are written, advance the IN_SYSTEM_BOOT_SETUP_DONE state
        // safety: the offset is correct because we're pulling it from our pre-defined constants and
        // those are manually checked.
        if success {
            unsafe { self.owc.inc(bao1x_api::IN_SYSTEM_BOOT_SETUP_DONE).unwrap() };
        }
        log::info!("Secret ID init done.");
        log::info!("{}KEYSTORE.INITDONE,{}", BOOKEND_START, BOOKEND_END);
    }

    pub fn ensure_system_init(&mut self, rram: &mut Reram) {
        // debug coreuser status
        /*
        let coreuser_range = xous::map_memory(
            xous::MemoryAddress::new(utralib::utra::coreuser::HW_COREUSER_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("Couldn't map coreuser range");
        let coreuser = utralib::CSR::new(coreuser_range.as_mut_ptr() as *mut u32);
        log::info!("coreuser status: {:x}", coreuser.r(utralib::utra::coreuser::STATUS));
        */

        if self.owc.get(bao1x_api::IN_SYSTEM_BOOT_SETUP_DONE).unwrap() == 0 {
            log::info!("System setup not yet done. Initializing secret identifiers...");
            self.system_init_inner(rram);
        }
    }

    /// returns `true` if collateral is erased
    pub fn is_collateral_erased(&mut self) -> bool {
        let collateral = self.slot_mgr.read(&COLLATERAL).unwrap();
        let check_val = vec![bao1x_hal::ERASE_VALUE; COLLATERAL.len() * SLOT_ELEMENT_LEN_BYTES];
        // log::info!("collateral: {:x?}", &collateral);
        // log::info!("check_val: {:x?}", &check_val);
        collateral == &check_val
    }

    pub fn derive_master_key(&mut self) {
        let mut ikm = Vec::<u8>::new();
        let mut salt = Vec::<u8>::new();
        let mut info = Vec::<u8>::new();
        let mut master_key = [0u8; KEY_LEN];

        // All APIs below are .unwrap() because any access errors that occur are hard faults and
        // should stop/panic at that line. Kicking the error up the stack isn't useful.

        // read key material
        ikm.extend_from_slice(self.slot_mgr.read(&ROOT_SEED).unwrap());

        // Analyze part of the key and print some informative statements.
        // We want to make sure that keys are actually being initialized, erased, or denied
        // look at only 64 bits out of the 256 bit key - it's enough that it's highly unlikely that
        // these 64 bits would match any of the zero/erased values, but not so much that if these
        // values are leaked,  that it'd be a serious threat to security as you'd still have 192 bits of
        // secret material, not to mention all the additional nuisance keys and chaff keys yet to come.
        let root_seed = self.slot_mgr.read(&ROOT_SEED).unwrap();
        if root_seed[..8] == [0u8; 8] {
            log::info!("{}KEYSTORE.ZERO,{}", BOOKEND_START, BOOKEND_END);
        } else if root_seed[..8] == [bao1x_hal::ERASE_VALUE; 8] {
            log::info!("{}KEYSTORE.ERASED,{}", BOOKEND_START, BOOKEND_END);
        } else {
            log::info!("{}KEYSTORE.KEYPASS,{}", BOOKEND_START, BOOKEND_END);
        }

        // build nuisance key offsets - this is direct readout hardening
        let nk0 = NUISANCE_KEYS_0.try_into_data_iter().unwrap();
        let nk1 = NUISANCE_KEYS_1.try_into_data_iter().unwrap();
        let nk = nk0.chain(nk1).collect::<Vec<usize>>();
        let nk_len = nk.len();

        // get chaff offsets - this is power side channel hardening
        let mut chaff = CHAFF_KEYS.try_into_data_iter().unwrap().collect::<Vec<usize>>();
        // compute chaff permutation order
        let mut rng = rand::thread_rng();
        chaff.shuffle(&mut rng);
        let mut ct_chaff = chaff.clone();
        ct_chaff.shuffle(&mut rng);
        let mut chaff_xor = [0u8; KEY_LEN];
        let mut ct_xor = [0u8; KEY_LEN]; // constant-time alternative

        // reserve enough space so we don't risk invoking the allocator during the extensions
        // we don't know what the allocator does, if it's data-dependent, that's a side channel.
        // length is the master seed(1) + chaff(1) + nuisance(nk_len)
        ikm.reserve((nk_len + 1 + 1) * KEY_LEN);

        // guarantee an even distribution of 1/0 in the selectors
        let mut selector = vec![true, false].repeat(nk.len() / 2);
        // shuffle the order
        selector.shuffle(&mut rng);
        for nk_offset in nk.into_iter() {
            if let Some(branch) = selector.pop() {
                if branch {
                    if let Some(chaff_offset) = chaff.pop() {
                        // safety: the offset is generated by the try_into_data_iter method, which generates
                        // safe offsets
                        for (cx, &src) in
                            chaff_xor.iter_mut().zip(unsafe { self.slot_mgr.read_data_slot(chaff_offset) })
                        {
                            *cx ^= src;
                        }
                    }
                } else {
                    // redundant "dummy" path to make constant time
                    if let Some(chaff_offset) = ct_chaff.pop() {
                        // safety: the offset is generated by the try_into_data_iter method, which generates
                        // safe offsets
                        for (cx, &src) in
                            ct_xor.iter_mut().zip(unsafe { self.slot_mgr.read_data_slot(chaff_offset) })
                        {
                            *cx ^= src;
                        }
                    }
                }
            }
            // flush the cache so that the read out chaff isn't cached for the next iteration, thus creating
            // a side-channel
            bao1x_hal::cache_flush();

            // get the nuisance key, in-order
            // safety: the offset is generated by the try_into_data_iter method, which generates safe offsets
            ikm.extend_from_slice(unsafe { self.slot_mgr.read_data_slot(nk_offset) });
        }
        // Drain the remainder. There's always going to be four keys left over because the NUISANCE array
        // isn't evenly sized due to the -A1 stepping bug. I think this isn't damning because the actual
        // ordering of the chaff is unknown at this point, and it's always the same amount.
        for remaining_chaff in chaff.drain(..) {
            for (cx, &src) in
                chaff_xor.iter_mut().zip(unsafe { self.slot_mgr.read_data_slot(remaining_chaff) })
            {
                *cx ^= src;
            }
        }

        #[cfg(feature = "hazardous-debug")]
        // leak only part of the chaff, see below comment about hedging against accidental signing of a debug
        // image
        log::info!("chaff: {:x?}", &chaff_xor[..8]);
        ikm.extend_from_slice(&chaff_xor);
        assert!(ikm.len() == (nk_len + 1 + 1) * KEY_LEN); // sanity check that all keys were in fact added

        // add salt
        // UUID is a random unique number made by the TRNG
        salt.extend_from_slice(self.slot_mgr.read(&UUID).unwrap());
        // CP_ID is a sequentially incrementing number that is included just in case something went horribly
        // wrong with the TRNG during ID generation - this gives us at least *some* guaranteed uniqueness,
        // but not randomness.
        salt.extend_from_slice(self.slot_mgr.read(&CP_ID).unwrap());

        // add info
        if self.owc.get(DEVELOPER_MODE).unwrap() != 0 {
            info.extend_from_slice(b"dev");
        } else {
            info.extend_from_slice(b"sec");
        }

        if self.owc.get(OEM_MODE).unwrap() != 0 {
            info.extend_from_slice(b"oem");
        }

        if self.owc.get(BOOT0_PUBKEY_FAIL).unwrap() != 0 {
            info.extend_from_slice(b"tampered");
        }

        #[cfg(feature = "hazardous-debug")]
        for (i, chunk) in ikm.chunks(32).enumerate() {
            // subsample the keyspace - we're using hazardous-debug in CI to confirm correctness
            // of keys, which creates some risk that these images are abused in production (e.g. if
            // one is accidentally signed where this is on). Subsampling the keys reduces the risk
            // in case such an image gets out.
            if i % 8 == 0 {
                log::info!("ikm({:4}): {:x?}", i * 32, chunk);
            }
        }
        log::debug!("salt: {:x?}", salt); // not hazardous, these are public values
        let hk = Hkdf::<Sha256>::new(Some(&salt[..]), &ikm);
        log::debug!("info: {:x?}", info); // not hazardous, these are public values
        hk.expand(&info, &mut master_key).unwrap();

        // do something with ct_xor that guarantees it's never optimized out
        if ct_xor.iter().all(|&cx| cx == 0) {
            log::warn!("Chaff result is all-0. Confirming that we're in developer mode");
            log::info!("{}KEYSTORE.ZEROCHAFF,{}", BOOKEND_START, BOOKEND_END);
            // panic if we're not in developer mode
            assert!(
                self.owc.get(DEVELOPER_MODE).unwrap() != 0,
                "Either we have the most improbable chaff, or (more likely) the chaff is set to all 0's"
            );
            // check that "the flag" is erased in this mode by leaking the first few bytes
            #[cfg(feature = "hazardous-debug")]
            log::info!(
                "{}KEYSTORE.ERASEDFLAG,{},{:x?}",
                BOOKEND_START,
                BOOKEND_END,
                &self.slot_mgr.read(&THE_FLAG_1).unwrap()[..4]
            );
        } else {
            log::info!("{}KEYSTORE.OKCHAFF,{}", BOOKEND_START, BOOKEND_END);
        }
        self.master_key = Some(master_key);
    }

    pub fn aes_kwp(&self, kwp: &mut KeyWrapper) -> Result<(), xous::Error> {
        use aes_kw::Kek;
        use aes_kw::KekAes256;
        let keywrapper: KekAes256 = Kek::from(self.master_key.ok_or(xous::Error::UseBeforeInit)?);
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
                            }
                            aes_kw::Error::InvalidOutputSize { expected } => {
                                log::info!("invalid output size {}", expected);
                                KeywrapError::InvalidOutputSize
                            }
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
                            aes_kw::Error::IntegrityCheckFailed => KeywrapError::IntegrityCheckFailed,
                            aes_kw::Error::InvalidDataSize => KeywrapError::InvalidDataSize,
                            aes_kw::Error::InvalidKekSize { size } => {
                                log::info!("invalid size {}", size); // weird. can't name this _size
                                KeywrapError::InvalidKekSize
                            }
                            aes_kw::Error::InvalidOutputSize { expected } => {
                                log::info!("invalid output size {}", expected);
                                KeywrapError::InvalidOutputSize
                            }
                        });
                    }
                }
            }
        }
        Ok(())
    }

    pub fn aes_op(&mut self, aes_op: &mut AesOp) -> Result<(), xous::Error> {
        let op = match aes_op.aes_op {
            // seems stupid, but we have to do this because we want to have zeroize on the AesOp
            // record, and it means we can't have Copy on this.
            AesOpType::Decrypt => AesOpType::Decrypt,
            AesOpType::Encrypt => AesOpType::Encrypt,
        };
        let hk = Hkdf::<Sha256>::new(None, &self.master_key.ok_or(xous::Error::UseBeforeInit)?);
        let mut okm = [0u8; 32];
        hk.expand(&aes_op.domain.as_bytes(), &mut okm).expect("32 is a valid length for Sha256 to output");
        let cipher = Aes256::new(GenericArray::from_slice(&okm));

        // deserialize the specifier
        match aes_op.block {
            AesBlockType::SingleBlock(b) => {
                match op {
                    AesOpType::Decrypt => cipher.decrypt_block(&mut b.try_into().unwrap()),
                    AesOpType::Encrypt => cipher.encrypt_block(&mut b.try_into().unwrap()),
                }
                aes_op.block = AesBlockType::SingleBlock(b);
            }
            AesBlockType::ParBlock(mut pb) => {
                match op {
                    AesOpType::Decrypt => {
                        for block in pb.iter_mut() {
                            cipher.decrypt_block(block.try_into().unwrap());
                        }
                    }
                    AesOpType::Encrypt => {
                        for block in pb.iter_mut() {
                            cipher.encrypt_block(block.try_into().unwrap());
                        }
                    }
                }
                aes_op.block = AesBlockType::ParBlock(pb);
            }
        };
        Ok(())
    }

    pub fn set_bootwait(&self, enable: Option<bool>) -> Result<bool, xous::Error> {
        let previous = self.owc.get_decoded::<bao1x_api::BootWaitCoding>().expect("couldn't fetch flag");
        if let Some(enable) = enable {
            while self.owc.get_decoded::<bao1x_api::BootWaitCoding>().expect("couldn't fetch flag")
                != if enable { bao1x_api::BootWaitCoding::Enable } else { bao1x_api::BootWaitCoding::Disable }
            {
                self.owc.inc_coded::<bao1x_api::BootWaitCoding>().unwrap();
            }
        }
        match previous {
            BootWaitCoding::Enable => Ok(true),
            BootWaitCoding::Disable => Ok(false),
        }
    }

    pub fn is_developer(&self) -> bool { self.owc.get(bao1x_api::DEVELOPER_MODE).unwrap() != 0 }

    #[cfg(not(feature = "legacy-swap-key"))]
    pub(crate) fn get_swap_key(&self) -> [u8; 32] {
        self.slot_mgr.read(&SWAP_KEY).expect("couldn't access swap key").try_into().unwrap()
    }

    #[cfg(feature = "legacy-swap-key")]
    /// For legacy systems, the key isn't initialized on boot. If it's all 0, assume this is the case
    /// and generate a fresh key.
    pub(crate) fn get_swap_key(&self, rram: &mut Reram) -> [u8; 32] {
        let key_slice = self.slot_mgr.read(&SWAP_KEY).expect("couldn't access swap key");
        let mut key = [0u8; 32];
        key.copy_from_slice(&key_slice);
        if key == [0u8; 32] {
            let xns = xous_names::XousNames::new().unwrap();
            let mut trng = bao1x_hal_service::trng::Trng::new(&xns).unwrap();
            trng.fill_bytes(&mut key);
            match self.slot_mgr.write(rram, &SWAP_KEY, &key) {
                Err(e) => {
                    log::warn!("{:?}: Couldn't initialize the swap key - swap will be insecure!", e);
                }
                _ => {}
            };
            key
        } else {
            key
        }
    }

    #[cfg(feature = "app-keys")]
    pub fn read_app_key(&self, index: usize) -> Result<[u8; 32], xous::Error> {
        use bao1x_api::common::APPLICATION;
        let real_index = index + bao1x_api::common::APPLICATION.get_base();
        if real_index < APPLICATION.get_base() || real_index >= APPLICATION.get_base() + APPLICATION.len() {
            return Err(xous::Error::AccessDenied);
        } else {
            use bao1x_api::SlotIndex;

            let slot =
                SlotIndex::Data(real_index, bao1x_api::PartitionAccess::Open, bao1x_api::RwPerms::ReadWrite);
            if let Ok(result) = self.slot_mgr.read(&slot) {
                let mut storage = [0u8; 32];
                storage.copy_from_slice(result);
                Ok(storage)
            } else {
                Err(xous::Error::AccessDenied)
            }
        }
    }

    #[cfg(feature = "app-keys")]
    // indices are absolute offsets
    pub fn write_app_key(&self, rram: &mut Reram, index: usize, data: [u8; 32]) -> Result<(), xous::Error> {
        use bao1x_api::common::APPLICATION;
        let real_index = index + bao1x_api::common::APPLICATION.get_base();
        if real_index < APPLICATION.get_base() || real_index >= APPLICATION.get_base() + APPLICATION.len() {
            return Err(xous::Error::AccessDenied);
        } else {
            use bao1x_api::SlotIndex;

            let slot =
                SlotIndex::Data(real_index, bao1x_api::PartitionAccess::Open, bao1x_api::RwPerms::ReadWrite);
            match self.slot_mgr.write(rram, &slot, &data) {
                Ok(_) => {
                    // log::info!("write success: {:?} {:x?}", slot, data);
                    Ok(())
                }
                Err(e) => {
                    log::warn!("Write app key error: {:?}", e);
                    match e {
                        bao1x_api::AccessError::WriteError => Err(xous::Error::HardwareError),
                        _ => Err(xous::Error::AccessDenied),
                    }
                }
            }
        }
    }

    #[cfg(feature = "swap")]
    fn get_swap_header(&self) -> Result<SwapSourceHeader, xous::Error> {
        // safety: swap_range is aligned to 4096-byte boundary and filled with initialized data
        // ... or so we hope! we have to validate these fields, as they are attacker-controlled.
        // the alignment is at least true as we control that
        let ssh: &SwapSourceHeader =
            unsafe { (self.swap_range.as_ptr() as *const SwapSourceHeader).as_ref().unwrap() };

        if ssh.version != SWAP_VERSION {
            return Err(xous::Error::VerificationError);
        }
        Ok(ssh.clone())
    }

    /// Implements the encryption check & call. Requires a connection, opcode and token
    /// so that it can provide regular status updates to a UI layer that is outside of
    /// this crate.
    #[cfg(feature = "swap")]
    pub fn ensure_swap_encryption(
        &mut self,
        cid: xous::CID,
        opcode: u32,
        token: [u32; 3],
        key: [u8; 32],
    ) -> Result<(), xous::Error> {
        use xous_swapper::FlashPage;
        use zeroize::Zeroize;

        const REPORT_INTERVAL: usize = 8;

        let ssh = self.get_swap_header()?;
        assert!(ssh.mac_offset & 0xFFF == 0, "MAC offset has improper alignment");
        let image_mac_start = ssh.mac_offset as usize + 0x1000;
        let mut current_mac_offset = image_mac_start;

        let zero_cipher = Aes256GcmSiv::new((&[0u8; 32]).into());
        let dest_cipher = Aes256GcmSiv::new(&key.into());

        let mut page_buf = FlashPage::new();
        let mut tags = Vec::<Tag>::new();
        const MAX_TAGS: usize = FLASH_SECTOR_LEN as usize / size_of::<Tag>();

        let swapper = xous_swapper::Swapper::new().unwrap();

        // swap image starts at 0x1000 and continues until mac_offset.
        // The offset of 0x1000 is currently a hard-coded constant. It's used in
        // loader/src/platform/bao1x/swap.rs and also in tools/src/swap_writer.rs
        for (i, offset) in (0x1000..ssh.mac_offset as usize + 0x1000).step_by(FLASH_SECTOR_LEN).enumerate() {
            let ciphertext = &self.swap_slice()[offset..offset + FLASH_SECTOR_LEN];
            page_buf.data.copy_from_slice(ciphertext);
            let mut nonce = [0u8; size_of::<Nonce>()];
            nonce[..4].copy_from_slice(&(offset as u32 - 0x1000).to_be_bytes());
            nonce[4..].copy_from_slice(&ssh.partial_nonce);
            let mut aad_buf = [0u8; 64];
            aad_buf[..ssh.aad_len as usize].copy_from_slice(&ssh.aad[..ssh.aad_len as usize]);
            let aad = &aad_buf[..ssh.aad_len as usize];
            let tag_loc = image_mac_start + ((offset - 0x1000) / 0x1000) * size_of::<Tag>();
            let tag = &self.swap_slice()[tag_loc..tag_loc + size_of::<Tag>()];
            let nonce = Nonce::from_slice(&nonce);
            // log::info!("aad: {:x?}", &aad);
            // log::info!("nonce: {:x?}", &nonce);
            // log::info!("tag: {:x?}", &tag);
            // log::info!("data: {:x?}...{:x?}", &page_buf.data[..16], &page_buf.data[4080..]);
            if zero_cipher.decrypt_in_place_detached(nonce, &aad, &mut page_buf.data, tag.into()).is_err() {
                log::debug!("zero cipher fail");
                if offset == 0x1000 {
                    // if we have an error on the first block, it might be that we're already encrypted
                    if dest_cipher
                        .decrypt_in_place_detached(nonce, &aad, &mut page_buf.data, tag.into())
                        .is_ok()
                    {
                        // we're already encrypted - nothing to do
                        return Ok(());
                    }
                }
                log::debug!("didn't succeed fallback verification");
                // just bail if we can't get the data - even if we're halfway through...
                return Err(xous::Error::VerificationError);
            }
            // now page_buf has our data - let's encrypt to the new key
            let new_tag = dest_cipher
                .encrypt_in_place_detached(nonce, &aad, &mut page_buf.data)
                .expect("couldn't encrypt to new key");
            tags.push(new_tag);

            // commit the page buf
            swapper.write_page(offset, &page_buf)?;

            // check and see if we need to commit tags
            if tags.len() == MAX_TAGS {
                // we have enough tags to write over a full page of tags - since we are going in linear
                // order through memory, it's safe to replace a page of tags after we've read and encrypted
                // the last page of tags.
                page_buf.data.zeroize();
                for (tag, chunk) in tags.iter().zip(page_buf.data.chunks_exact_mut(size_of::<Tag>())) {
                    chunk.copy_from_slice(&tag[..]);
                }
                swapper.write_page(current_mac_offset, &page_buf)?;
                current_mac_offset += FLASH_SECTOR_LEN;
                tags.clear();
            }

            // report status to the caller - %age completion. This is a slow operation and users
            // are impatient.
            if i % REPORT_INTERVAL == 0 {
                let percentage = (100 * i) / (ssh.mac_offset as usize / FLASH_SECTOR_LEN);
                xous::try_send_message(
                    cid,
                    xous::Message::new_scalar(
                        opcode as usize,
                        percentage,
                        token[0] as usize,
                        token[1] as usize,
                        token[2] as usize,
                    ),
                )
                .ok();
            }
        }
        // the final page may not have filled up the whole tag buffer. clear the remaining tags
        page_buf.data.zeroize();
        for (tag, chunk) in tags.iter().zip(page_buf.data.chunks_exact_mut(size_of::<Tag>())) {
            chunk.copy_from_slice(&tag[..]);
        }
        swapper.write_page(current_mac_offset, &page_buf)?;

        Ok(())
    }
}
