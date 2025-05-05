#![cfg_attr(all(target_os = "none", not(test)), no_std)]
//! Detailed docs are parked under Structs/RootKeys down below

use keystore_api::*;

pub mod key2bits;

use std::convert::TryInto;

pub use cipher::{
    BlockBackend, BlockCipher, BlockClosure, BlockDecrypt, BlockEncrypt, BlockSizeUser, ParBlocksSizeUser,
    consts::U16, generic_array::GenericArray, inout::InOut,
};
use num_traits::*;
use xous::{CID, Message, send_message};
use xous_ipc::Buffer;
use xous_semver::SemVer;
pub(crate) type BatchBlocks = GenericArray<Block, U16>;

pub enum ImageType {
    All,
    Gateware,
    Loader,
    Kernel,
}

#[doc = include_str!("../README.md")]
#[derive(Debug)] // there is no confidential information in the external structure; it's safe to Debug it
pub struct RootKeys {
    conn: CID,
    // index of the key to use for the next encrypt/decrypt ops
    key_index: AesRootkeyType,
}
impl RootKeys {
    pub fn new(xns: &xous_names::XousNames, key_index: Option<AesRootkeyType>) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns
            .request_connection_blocking(rootkeys_api::SERVER_NAME_KEYS)
            .expect("Can't connect to Keys server");
        let index = if let Some(ki) = key_index { ki } else { AesRootkeyType::NoneSpecified };
        Ok(RootKeys { conn, key_index: index })
    }

    pub fn conn(&self) -> CID { self.conn }

    pub fn get_try_init_keys_op(&self) -> u32 { Opcode::UxTryInitKeys.to_u32().unwrap() }

    pub fn get_update_gateware_op(&self) -> u32 { Opcode::UxUpdateGateware.to_u32().unwrap() }

    pub fn get_blind_copy_gateware_op(&self) -> u32 { Opcode::UxBlindCopy.to_u32().unwrap() }

    pub fn get_try_selfsign_op(&self) -> u32 { Opcode::UxSelfSignXous.to_u32().unwrap() }

    /// this initiates an attempt to update passwords. User must unlock their device first, and can cancel out
    /// if not expected.
    pub fn try_update_password(&mut self, _which: PasswordType) -> Result<(), xous::Error> {
        unimplemented!();
    }

    /// checks to see if the KEYROM has been initialized, and if not, generates keys. In the process of doing
    /// so, the user will be prompted to enter passwords. It also automatically self-signs everything --
    /// presumably, if you were comfortable enough to use this firmware to make your keys, you also
    /// trusted it. it will then update the bitstream with your keys.
    pub fn try_init_keys(&self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::UxTryInitKeys.to_usize().unwrap(), 0, 0, 0, 0))
            .map(|_| ())
    }

    pub fn is_initialized(&self) -> Result<bool, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::KeysInitialized.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't send KeysInitialized check message");
        if let xous::Result::Scalar1(result) = response {
            if result != 0 { return Ok(true) } else { return Ok(false) }
        } else {
            log::error!("unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }

    pub fn is_zero_key(&self) -> Result<Option<bool>, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::IsZeroKey.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't send IsZeroKey check message");
        if let xous::Result::Scalar2(result, valid) = response {
            if valid != 0 {
                if result != 0 { return Ok(Some(true)) } else { return Ok(Some(false)) }
            } else {
                Ok(None)
            }
        } else {
            log::error!("unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }

    pub fn is_dont_ask_set(&self) -> Result<bool, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::IsDontAskSet.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't send IsDontAsk check message");
        if let xous::Result::Scalar1(result) = response {
            if result != 0 { return Ok(true) } else { return Ok(false) }
        } else {
            log::error!("unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }

    pub fn do_update_gw_ux_flow(&self) {
        send_message(
            self.conn,
            Message::new_scalar(Opcode::UxUpdateGateware.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't send message to root keys");
    }

    pub fn do_update_gw_ux_flow_blocking(&self) {
        send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::UxUpdateGateware.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't send message to root keys");
    }

    pub fn do_init_keys_ux_flow(&self) {
        send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::UxTryInitKeys.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't send message to root keys");
    }

    pub fn do_create_backup_ux_flow(&self, metadata: BackupHeader, checksums: Option<Checksums>) {
        let mut alloc = BackupHeaderIpc::default();
        let mut data = [0u8; core::mem::size_of::<BackupHeader>()];
        data.copy_from_slice(metadata.as_ref());
        alloc.data = Some(data);
        alloc.checksums = checksums;
        let buf = Buffer::into_buf(alloc).unwrap();
        buf.send(self.conn, Opcode::CreateBackup.to_u32().unwrap()).unwrap();
    }

    pub fn do_restore_backup_ux_flow(&self) {
        send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::DoRestore.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't send message to root keys");
    }

    pub fn do_erase_backup(&self) {
        send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::EraseBackupBlock.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't send message to root keys");
    }

    pub fn do_reset_dont_ask_init(&self) {
        send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::ResetDontAsk.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't send message to root keys");
    }

    /// Returns the raw, unchecked restore header. Further checking may be done at the restore point,
    /// but the purpose of this is to just decide if we should even try to initiate a restore.
    pub fn get_restore_header(&self) -> Result<Option<BackupHeader>, xous::Error> {
        let alloc = BackupHeaderIpc::default();
        let mut buf = Buffer::into_buf(alloc).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::ShouldRestore.to_u32().unwrap())
            .or(Err(xous::Error::InternalError))?;
        let ret = buf.to_original::<BackupHeaderIpc, _>().unwrap();
        if let Some(d) = ret.data {
            let mut header = BackupHeader::default();
            header.copy_from_slice(&d);
            Ok(Some(header))
        } else {
            Ok(None)
        }
    }

    /// this will return the version number, but always report the commit as None.
    /// The 32-bit commit ref isn't useful in version comparisons, it's more to assist
    /// maintainer debug. Including this would convert this from a scalar message to
    /// a memory message.
    ///
    /// Returns `xous::Error::InvalidString` if the staged area is blank.
    pub fn staged_semver(&self) -> Result<SemVer, xous::Error> {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::StagedSemver.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .map_err(|_| xous::Error::InternalError)?
        {
            xous::Result::Scalar2(a, b) => {
                let mut raw_ver = [0u8; 16];
                // this takes advantage of our knowledge of the internal structure of a semver binary record
                // namely, the last two words are for the commit and if they are zero they end up as None.
                raw_ver[0..4].copy_from_slice(&(a as u32).to_le_bytes());
                raw_ver[4..8].copy_from_slice(&(b as u32).to_le_bytes());
                // here, we do the check to see if the gateware area was *blank* in which case, we'd get an
                // all-FF's version
                if raw_ver[0..4] == [0xffu8; 4] {
                    Err(xous::Error::InvalidString)
                } else {
                    Ok(raw_ver.into())
                }
            }
            _ => Err(xous::Error::InternalError),
        }
    }

    /// This function will try to perform a gateware update under the assumption that there are no root keys.
    /// It will "silently fail" (i.e. return false but not panic) if any of numerous conditions/checks are not
    /// met.
    pub fn try_nokey_soc_update(&self) -> bool {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::TryNoKeySocUpdate.to_usize().unwrap(), 0, 0, 0, 0),
        ) {
            Ok(xous::Result::Scalar1(r)) => {
                if r != 0 {
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// returns `true` if we should prompt the user for an update
    pub fn prompt_for_update(&self) -> bool {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::ShouldPromptForUpdate.to_usize().unwrap(), 0, 0, 0, 0),
        ) {
            Ok(xous::Result::Scalar1(r)) => {
                if r != 0 {
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// sets the should prompt for user update to the given status
    pub fn set_update_prompt(&self, should_prompt: bool) {
        send_message(
            self.conn,
            Message::new_scalar(
                Opcode::SetPromptForUpdate.to_usize().unwrap(),
                if should_prompt { 1 } else { 0 },
                0,
                0,
                0,
            ),
        )
        .expect("couldn't set update prompt");
    }

    /// this will check the signature on the gateware.
    /// returns None if no keys have been initialized
    /// returns true if the gateware passes, false if it fails
    pub fn check_gateware_signature(&self) -> Result<Option<bool>, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::CheckGatewareSignature.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar1(result) = response {
            if result == 2 {
                // uninit keys case
                Ok(None)
            } else if result == 1 {
                // passed
                Ok(Some(true))
            } else {
                // everything else -- fail
                Ok(Some(false))
            }
        } else {
            Err(xous::Error::InternalError)
        }
    }

    pub fn is_efuse_secured(&self) -> Result<Option<bool>, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::IsEfuseSecured.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar1(result) = response {
            if result == 2 {
                Ok(None)
            } else if result == 1 {
                Ok(Some(true))
            } else {
                Ok(Some(false))
            }
        } else {
            Err(xous::Error::InternalError)
        }
    }

    #[cfg(feature = "efuse")]
    pub fn do_efuse_burn(&self) {
        send_message(self.conn, Message::new_scalar(Opcode::BurnEfuse.to_usize().unwrap(), 0, 0, 0, 0))
            .expect("couldn't initiate eFuse burn");
    }

    pub fn is_jtag_working(&self) -> Result<bool, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::IsJtagWorking.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar1(result) = response {
            if result == 1 { Ok(true) } else { Ok(false) }
        } else {
            Err(xous::Error::InternalError)
        }
    }

    fn ensure_aes_password(&self) -> bool {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(
                Opcode::UxAesEnsurePassword.to_usize().unwrap(),
                self.key_index as usize,
                0,
                0,
                0,
            ),
        )
        .expect("failed to ensure password is current");
        if let xous::Result::Scalar1(result) = response {
            if result != 1 {
                log::error!("there was a problem ensuring our password was unlocked, aborting!");
                return false;
            }
        } else {
            log::error!("there was a problem ensuring our password was unlocked, aborting!");
            return false;
        }
        true
    }

    pub fn test_ux(&mut self, arg: usize) {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::TestUx.to_usize().unwrap(), arg, 0, 0, 0),
        )
        .expect("couldn't send test message");
        log::info!("test_ux response: {:?}", response);
    }

    pub fn bbram_provision(&self) {
        send_message(self.conn, Message::new_scalar(Opcode::BbramProvision.to_usize().unwrap(), 0, 0, 0, 0))
            .expect("couldn't send bbram provision message");
    }

    pub fn clear_password(&self, pass_type: AesRootkeyType) {
        send_message(
            self.conn,
            Message::new_blocking_scalar(
                Opcode::ClearPasswordCacheEntry.to_usize().unwrap(),
                pass_type.to_usize().unwrap(),
                0,
                0,
                0,
            ),
        )
        .expect("couldn't send bbram provision message");
    }

    pub fn wrap_key(&self, input: &[u8]) -> Result<Vec<u8>, KeywrapError> {
        if input.len() > MAX_WRAP_DATA {
            // of course, the underlying crypto can handle a much larger piece of data,
            // but the intention of this API is to wrap crypto keys -- not bulk data. So for simplicity
            // we're going to limit the size of wrapped data to 2kiB (typically it's envisioned you're
            // wrapping data on the order of 32-512 bytes)
            return Err(KeywrapError::InvalidDataSize);
        }
        if !self.ensure_aes_password() {
            return Err(KeywrapError::IntegrityCheckFailed);
        }
        let mut alloc = KeyWrapper {
            data: [0u8; MAX_WRAP_DATA + 8],
            len: input.len() as u32,
            key_index: self.key_index.to_u8().unwrap(),
            op: KeyWrapOp::Wrap,
            result: Some(KeywrapError::IntegrityCheckFailed), /* initialize to a default value that throws
                                                               * an error if it wasn't modified by the
                                                               * recipient */
            // this field is ignored on the Wrap side
            expected_len: 0,
        };
        for (&src, dst) in input.iter().zip(alloc.data.iter_mut()) {
            *dst = src;
        }
        let mut buf = Buffer::into_buf(alloc).or(Err(KeywrapError::IntegrityCheckFailed))?;
        buf.lend_mut(self.conn, Opcode::AesKwp.to_u32().unwrap())
            .or(Err(KeywrapError::IntegrityCheckFailed))?;
        let ret = buf.to_original::<KeyWrapper, _>().unwrap();
        match ret.result {
            None => {
                // no error, proceed
                Ok(ret.data[..ret.len as usize].to_vec())
            }
            Some(err) => Err(err),
        }
    }

    pub fn unwrap_key(&self, wrapped: &[u8], expected_len: usize) -> Result<Vec<u8>, KeywrapError> {
        if wrapped.len() > MAX_WRAP_DATA + 8 {
            return Err(KeywrapError::InvalidDataSize);
        }
        if !self.ensure_aes_password() {
            return Err(KeywrapError::IntegrityCheckFailed);
        }
        let mut alloc = KeyWrapper {
            data: [0u8; MAX_WRAP_DATA + 8],
            len: wrapped.len() as u32,
            key_index: self.key_index.to_u8().unwrap(),
            op: KeyWrapOp::Unwrap,
            result: Some(KeywrapError::IntegrityCheckFailed), /* initialize to a default value that throws
                                                               * an error if it wasn't modified by the
                                                               * recipient */
            expected_len: expected_len as u32,
        };
        for (&src, dst) in wrapped.iter().zip(alloc.data.iter_mut()) {
            *dst = src;
        }
        let mut buf = Buffer::into_buf(alloc).or(Err(KeywrapError::IntegrityCheckFailed))?;
        buf.lend_mut(self.conn, Opcode::AesKwp.to_u32().unwrap())
            .or(Err(KeywrapError::IntegrityCheckFailed))?;
        let ret = buf.to_original::<KeyWrapper, _>().unwrap();
        // note: this return vector (which may contain a key) is not zeroized on drop anymore due to a
        // regression correlated to a fix to a crypto algorithm. This was pushed as a hotfix for v0.9.9.
        match ret.result {
            None => {
                // no error, proceed
                if ret.len as usize != expected_len {
                    // The key wrapper will return a vector that is rounded to the nearest 8-bytes, with
                    // a zero-pad on the excess data. Ensure that the zero-pad is intact before lopping it
                    // off.
                    if (ret.len as usize) < expected_len {
                        Err(KeywrapError::InvalidOutputSize)
                    } else {
                        let mut all_zeroes = true;
                        for &d in ret.data[expected_len..ret.len as usize].iter() {
                            if d != 0 {
                                all_zeroes = false;
                                break;
                            }
                        }
                        if all_zeroes {
                            Ok(ret.data[..expected_len as usize].to_vec())
                        } else {
                            Err(KeywrapError::InvalidDataSize)
                        }
                    }
                } else {
                    Ok(ret.data[..expected_len as usize].to_vec())
                }
            }
            Some(err) => {
                // note: the error message may contain a plaintext copy of the key that is not
                // zeroized on drop. However, it only happens in the case of a migration from
                // a legacy version (pre v0.9.8) PDDB, by the time this matters it should be
                // a non-issue.
                Err(err)
            }
        }
    }

    #[inline(always)]
    pub(crate) fn get_enc_backend(&self) -> RootKeysEnc<'_> { RootKeysEnc(self) }

    #[inline(always)]
    pub(crate) fn get_dec_backend(&self) -> RootKeysDec<'_> { RootKeysDec(self) }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for RootKeys {
    fn drop(&mut self) {
        log::debug!("dropping rootkeys object");
        // the connection to the server side must be reference counted, so that multiple instances of this
        // object within a single process do not end up de-allocating the CID on other threads before
        // they go out of scope. Note to future me: you want this. Don't get rid of it because you
        // think, "nah, nobody will ever make more than one copy of this object".
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}

impl BlockSizeUser for RootKeys {
    type BlockSize = U16;
}

impl BlockCipher for RootKeys {}

impl BlockEncrypt for RootKeys {
    fn encrypt_with_backend(&self, f: impl BlockClosure<BlockSize = U16>) {
        f.call(&mut self.get_enc_backend())
    }
}

impl BlockDecrypt for RootKeys {
    fn decrypt_with_backend(&self, f: impl BlockClosure<BlockSize = U16>) {
        f.call(&mut self.get_dec_backend())
    }
}

pub(crate) struct RootKeysEnc<'a>(&'a RootKeys);

impl<'a> BlockSizeUser for RootKeysEnc<'a> {
    type BlockSize = U16;
}

impl<'a> ParBlocksSizeUser for RootKeysEnc<'a> {
    type ParBlocksSize = U16;
}

impl<'a> BlockBackend for RootKeysEnc<'a> {
    fn proc_block(&mut self, mut block: InOut<'_, '_, Block>) {
        if !self.0.ensure_aes_password() {
            return;
        }
        let op = AesOp {
            key_index: self.0.key_index.to_u8().unwrap(),
            block: AesBlockType::SingleBlock(block.clone_in().as_slice().try_into().unwrap()),
            aes_op: AesOpType::Encrypt,
        };
        let mut buf = Buffer::into_buf(op).unwrap();
        buf.lend_mut(self.0.conn, Opcode::AesOracle.to_u32().unwrap())
            .expect("couldn't initiate encrypt_block operation");
        let ret_op =
            buf.as_flat::<AesOp, _>().expect("got the wrong type of data structure back for encrypt_block");
        if let ArchivedAesBlockType::SingleBlock(b) = ret_op.block {
            *block.get_out() = *Block::from_slice(&b);
        }
    }

    fn proc_par_blocks(&mut self, mut blocks: InOut<'_, '_, BatchBlocks>) {
        if !self.0.ensure_aes_password() {
            return;
        }
        let mut pb_buf: [[u8; 16]; PAR_BLOCKS] = [[0; 16]; PAR_BLOCKS];
        for (dst_block, src_block) in pb_buf.iter_mut().zip(blocks.clone_in().as_slice().iter()) {
            for (dst, &src) in dst_block.iter_mut().zip(src_block.as_slice().iter()) {
                *dst = src;
            }
        }
        let op = AesOp {
            key_index: self.0.key_index.to_u8().unwrap(),
            block: AesBlockType::ParBlock(pb_buf),
            aes_op: AesOpType::Encrypt,
        };
        let mut buf = Buffer::into_buf(op).unwrap();
        buf.lend_mut(self.0.conn, Opcode::AesOracle.to_u32().unwrap())
            .expect("couldn't initiate encrypt_block operation");
        let ret_op =
            buf.as_flat::<AesOp, _>().expect("got the wrong type of data structure back for encrypt_block");
        if let ArchivedAesBlockType::ParBlock(pb) = ret_op.block {
            for (b, pbs) in pb.iter().zip(blocks.get_out().iter_mut()) {
                for (&src, dst) in b.iter().zip(pbs.as_mut_slice().iter_mut()) {
                    *dst = src;
                }
            }
        }
    }
}

pub(crate) struct RootKeysDec<'a>(&'a RootKeys);

impl<'a> BlockSizeUser for RootKeysDec<'a> {
    type BlockSize = U16;
}

impl<'a> ParBlocksSizeUser for RootKeysDec<'a> {
    type ParBlocksSize = U16;
}
impl<'a> BlockBackend for RootKeysDec<'a> {
    fn proc_block(&mut self, mut block: InOut<'_, '_, Block>) {
        if !self.0.ensure_aes_password() {
            return;
        }
        let op = AesOp {
            key_index: self.0.key_index.to_u8().unwrap(),
            block: AesBlockType::SingleBlock(block.clone_in().as_slice().try_into().unwrap()),
            aes_op: AesOpType::Decrypt,
        };
        let mut buf = Buffer::into_buf(op).unwrap();
        buf.lend_mut(self.0.conn, Opcode::AesOracle.to_u32().unwrap())
            .expect("couldn't initiate encrypt_block operation");
        let ret_op =
            buf.as_flat::<AesOp, _>().expect("got the wrong type of data structure back for encrypt_block");
        if let ArchivedAesBlockType::SingleBlock(b) = ret_op.block {
            *block.get_out() = *Block::from_slice(&b);
        }
    }

    fn proc_par_blocks(&mut self, mut blocks: InOut<'_, '_, BatchBlocks>) {
        if !self.0.ensure_aes_password() {
            return;
        }
        let mut pb_buf: [[u8; 16]; 16] = [[0; 16]; 16];
        for (dst_block, src_block) in pb_buf.iter_mut().zip(blocks.clone_in().as_slice().iter()) {
            for (dst, &src) in dst_block.iter_mut().zip(src_block.as_slice().iter()) {
                *dst = src;
            }
        }
        let op = AesOp {
            key_index: self.0.key_index.to_u8().unwrap(),
            block: AesBlockType::ParBlock(pb_buf),
            aes_op: AesOpType::Decrypt,
        };
        let mut buf = Buffer::into_buf(op).unwrap();
        buf.lend_mut(self.0.conn, Opcode::AesOracle.to_u32().unwrap())
            .expect("couldn't initiate encrypt_block operation");
        let ret_op =
            buf.as_flat::<AesOp, _>().expect("got the wrong type of data structure back for encrypt_block");
        if let ArchivedAesBlockType::ParBlock(pb) = ret_op.block {
            for (b, pbs) in pb.iter().zip(blocks.get_out().iter_mut()) {
                for (&src, dst) in b.iter().zip(pbs.as_mut_slice().iter_mut()) {
                    *dst = src;
                }
            }
        }
    }
}

#[cfg(test)]
mod bcrypt;

// some short tests to just confirm we're not totally broken.
#[cfg(test)]
mod tests {
    #[test]
    fn hash_with_fixed_salt() {
        let salt: [u8; 16] = [38, 113, 212, 141, 108, 213, 195, 166, 201, 38, 20, 13, 47, 40, 104, 18];
        let mut output: [u8; 24] = [0; 24];

        let pw = "My S3cre7 P@55w0rd!";

        crate::bcrypt::bcrypt(5, &salt, pw, &mut output);

        assert_eq!(
            output,
            [
                22, 80, 102, 192, 193, 204, 118, 167, 41, 102, 241, 75, 103, 49, 4, 245, 194, 145, 85, 104,
                179, 60, 88, 53
            ]
        );
    }

    #[test]
    fn hash_with_max_len() {
        let salt: [u8; 16] = [38, 113, 212, 141, 108, 213, 195, 166, 201, 38, 20, 13, 47, 40, 104, 18];
        let mut output: [u8; 24] = [0; 24];
        let pw = "this is a test of a very long password that is exactly 72 characters lon";
        crate::bcrypt::bcrypt(10, &salt, pw, &mut output);
        assert_eq!(
            output,
            [
                46, 39, 41, 217, 39, 103, 62, 189, 120, 3, 248, 84, 175, 40, 134, 190, 76, 43, 232, 147, 129,
                237, 116, 61
            ]
        );
    }

    #[test]
    fn hash_with_longer_than_max_len() {
        let salt: [u8; 16] = [38, 113, 212, 141, 108, 213, 195, 166, 201, 38, 20, 13, 47, 40, 104, 18];
        let mut output: [u8; 24] = [0; 24];
        let pw = "this is a test of a very long password that is exactly 72 characters long, but this one is even longer";
        crate::bcrypt::bcrypt(10, &salt, pw, &mut output);
        assert_eq!(
            output,
            [
                46, 39, 41, 217, 39, 103, 62, 189, 120, 3, 248, 84, 175, 40, 134, 190, 76, 43, 232, 147, 129,
                237, 116, 61
            ]
        );
    }
}
