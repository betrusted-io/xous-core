use aes::Aes256;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray};
use bao1x_api::{
    BOOT0_PUBKEY_FAIL, BoardTypeCoding, BootWaitCoding, CP_ID, DEVELOPER_MODE, OEM_MODE,
    SLOT_ELEMENT_LEN_BYTES, UUID,
};
use bao1x_hal::board::{BOOKEND_END, BOOKEND_START};
use bao1x_hal::{
    acram::{OneWayCounter, SlotManager},
    board::{CHAFF_KEYS, COLLATERAL, NUISANCE_KEYS_0, NUISANCE_KEYS_1, ROOT_SEED, THE_FLAG_1},
    rram::Reram,
};
use hkdf::Hkdf;
use keystore_api::KeyWrapper;
use rand::prelude::*;
use sha2::Sha256;

use crate::*;

const KEY_LEN: usize = bao1x_api::SLOT_ELEMENT_LEN_BYTES;

pub struct KeyStore {
    slot_mgr: SlotManager,
    owc: OneWayCounter,
    master_key: Option<[u8; KEY_LEN]>,
}

impl KeyStore {
    pub fn init_mappings(rram: &mut Reram) -> Self {
        let slot_mgr = SlotManager::new();
        slot_mgr.register_mapping(rram);
        let owc = OneWayCounter::new();
        owc.register_mapping(rram);

        Self { slot_mgr, owc, master_key: None }
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

        // one routine works for both dabao and baosec
        let board_type = self.owc.get_decoded::<BoardTypeCoding>().unwrap();
        if self.owc.get(bao1x_api::IN_SYSTEM_BOOT_SETUP_DONE).unwrap() == 0 {
            log::info!("System setup not yet done. Initializing secret identifiers...");
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
    }

    /// returns `true` if collateral is erased
    pub fn is_collateral_erased(&mut self) -> bool {
        let collateral = self.slot_mgr.read(&COLLATERAL).unwrap();
        let check_val = vec![bao1x_hal::sigcheck::ERASE_VALUE; COLLATERAL.len() * SLOT_ELEMENT_LEN_BYTES];
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
        #[cfg(feature = "hazardous-debug")]
        {
            // analyze part of the key and print some informative statements during hazardous-debug
            // we want to make sure that keys are actually being initialized, erased, or denied
            // look at only 64 bits out of the 256 bit key - it's enough that it's highly unlikely that
            // these 64 bits would match any of the zero/erased values, but not so much that if a
            // dev image is accidentally signed & released that it'd be a serious threat to security
            // as you'd still have 192 bits of secret material
            let root_seed = self.slot_mgr.read(&ROOT_SEED).unwrap();
            if root_seed[..8] == [0u8; 8] {
                log::info!("{}KEYSTORE.ZERO,{}", BOOKEND_START, BOOKEND_END);
            } else if root_seed[..8] == [bao1x_hal::sigcheck::ERASE_VALUE; 8] {
                log::info!("{}KEYSTORE.ERASED,{}", BOOKEND_START, BOOKEND_END);
            } else {
                log::info!("{}KEYSTORE.KEYPASS,{}", BOOKEND_START, BOOKEND_END);
            }
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
}
