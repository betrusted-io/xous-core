pub use cipher::{
    BlockBackend, BlockCipher, BlockClosure, BlockDecrypt, BlockEncrypt, BlockSizeUser, ParBlocksSizeUser,
    consts::U16, generic_array::GenericArray, inout::InOut,
};
use keystore_api::*;
use num_traits::*;
use xous::{CID, Message, send_message};
use xous_ipc::Buffer;
pub(crate) type BatchBlocks = GenericArray<Block, U16>;
use std::convert::TryInto;

#[derive(Debug)] // there is no confidential information in the external structure; it's safe to Debug it
pub struct Keystore {
    conn: CID,
    /// This is a domain separator for derived keys.
    domain: Option<String>,
}
impl Keystore {
    pub fn new(xns: &xous_names::XousNames) -> Self {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(SERVER_NAME_KEYS).expect("Can't connect to Keys server");
        Self { conn, domain: None }
    }

    pub fn new_key(xns: &xous_names::XousNames, key_type: &str) -> Self {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(SERVER_NAME_KEYS).expect("Can't connect to Keys server");
        Self { conn, domain: Some(key_type.to_owned()) }
    }

    pub fn clear_password(&self) {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::ClearPasswordCacheEntry.to_usize().unwrap(), 0, 0, 0, 0),
        ) {
            // it's a blocking scalar just to ensure that the password *has* cleared
            // any error in this routine should rightfully be a panic.
            Ok(xous::Result::Scalar5(_, _, _, _, _)) => (),
            _ => panic!("clear_password() failed with internal error"),
        }
    }

    pub fn get_dna(&self) -> u64 {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::GetDna.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .unwrap();
        if let xous::Result::Scalar5(_, val1, val2, _, _) = response {
            (val1 as u64) | ((val2 as u64) << 32)
        } else {
            panic!("get_dna() failed with internal error");
        }
    }

    /// This routine confirms that the keystore has all the user secrets necessary to decrypt
    /// the current password.
    ///
    /// On Precursor, this call would have prompted for the "unlock PIN",
    /// which is a shallow layer of security on top of the key store. The exact behavior of what
    /// this call entails is up to the keystore implementation.
    ///
    /// On Baosec, it's envisioned that there is a user setting which specifies three possible behaviors:
    ///   - None -- the master key is ready to use without any further derivation
    ///   - PIN -- Requires a 4-6 digit code to unlock the device; retry limits, time-outs and self-wipes may
    ///     also be configured
    ///   - QR -- Requires a high-strength 256-bit password from a scanned QR code as a strong additional
    ///     layer of security in case the device is lost or stolen.
    pub fn ensure_password(&self) -> PasswordState {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::EnsurePassword.to_usize().unwrap(), 0, 0, 0, 0),
        ) {
            Ok(xous::Result::Scalar5(_arg0, arg1, arg2, arg3, _arg4)) => {
                log::info!("got {:x}, {:x}, {:x}", arg1, arg2, arg3);
                (arg1, arg2, arg3).into()
            }
            _ => panic!("ensure_password() failed with internal error"),
        }
    }

    pub fn wrap_key(&self, input: &[u8]) -> Result<Vec<u8>, KeywrapError> {
        if input.len() > MAX_WRAP_DATA {
            // of course, the underlying crypto can handle a much larger piece of data,
            // but the intention of this API is to wrap crypto keys -- not bulk data. So for simplicity
            // we're going to limit the size of wrapped data to 2kiB (typically it's envisioned you're
            // wrapping data on the order of 32-512 bytes)
            return Err(KeywrapError::InvalidDataSize);
        }
        if let Some(domain) = &self.domain {
            let mut alloc = KeyWrapper {
                data: [0u8; MAX_WRAP_DATA + 8],
                len: input.len() as u32,
                domain: domain.to_owned(),
                op: KeyWrapOp::Wrap,
                result: Some(KeywrapError::IntegrityCheckFailed), /* initialize to a default value that
                                                                   * throws
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
        } else {
            // no domain separator was specified
            Err(KeywrapError::IntegrityCheckFailed)
        }
    }

    pub fn unwrap_key(&self, wrapped: &[u8], expected_len: usize) -> Result<Vec<u8>, KeywrapError> {
        if wrapped.len() > MAX_WRAP_DATA + 8 {
            return Err(KeywrapError::InvalidDataSize);
        }
        if let Some(domain) = &self.domain {
            let mut alloc = KeyWrapper {
                data: [0u8; MAX_WRAP_DATA + 8],
                len: wrapped.len() as u32,
                domain: domain.to_owned(),
                op: KeyWrapOp::Unwrap,
                result: Some(KeywrapError::IntegrityCheckFailed), /* initialize to a default value that
                                                                   * throws
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
        } else {
            // no domain separator was specified
            Err(KeywrapError::IntegrityCheckFailed)
        }
    }

    #[inline(always)]
    pub(crate) fn get_enc_backend(&self) -> KeystoreEnc<'_> { KeystoreEnc(self) }

    #[inline(always)]
    pub(crate) fn get_dec_backend(&self) -> KeystoreDec<'_> { KeystoreDec(self) }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Keystore {
    fn drop(&mut self) {
        // the connection to the server side must be reference counted, so that multiple instances of this
        // object within a single process do not end up de-allocating the CID on other threads before
        // they go out of scope. Note to future me: you want this. Don't get rid of it because you
        // think, "nah, nobody will ever make more than one copy of this object".
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
        // if there was object-specific state (such as a one-time use server for async callbacks, specific to
        // the object instance), de-allocate those items here. They don't need a reference count
        // because they are object-specific
    }
}

impl BlockSizeUser for Keystore {
    type BlockSize = U16;
}

impl BlockCipher for Keystore {}

impl BlockEncrypt for Keystore {
    fn encrypt_with_backend(&self, f: impl BlockClosure<BlockSize = U16>) {
        f.call(&mut self.get_enc_backend())
    }
}

impl BlockDecrypt for Keystore {
    fn decrypt_with_backend(&self, f: impl BlockClosure<BlockSize = U16>) {
        f.call(&mut self.get_dec_backend())
    }
}

pub(crate) struct KeystoreEnc<'a>(&'a Keystore);

impl<'a> BlockSizeUser for KeystoreEnc<'a> {
    type BlockSize = U16;
}

impl<'a> ParBlocksSizeUser for KeystoreEnc<'a> {
    type ParBlocksSize = U16;
}

impl<'a> BlockBackend for KeystoreEnc<'a> {
    fn proc_block(&mut self, mut block: InOut<'_, '_, Block>) {
        if let Some(domain) = &self.0.domain {
            let op = AesOp {
                domain: domain.to_owned(),
                block: AesBlockType::SingleBlock(block.clone_in().as_slice().try_into().unwrap()),
                aes_op: AesOpType::Encrypt,
            };
            let mut buf = Buffer::into_buf(op).unwrap();
            buf.lend_mut(self.0.conn, Opcode::AesOracle.to_u32().unwrap())
                .expect("couldn't initiate encrypt_block operation");
            let ret_op = buf
                .as_flat::<AesOp, _>()
                .expect("got the wrong type of data structure back for encrypt_block");
            if let ArchivedAesBlockType::SingleBlock(b) = ret_op.block {
                *block.get_out() = *Block::from_slice(&b);
            }
        }
    }

    fn proc_par_blocks(&mut self, mut blocks: InOut<'_, '_, BatchBlocks>) {
        if let Some(domain) = &self.0.domain {
            let mut pb_buf: [[u8; 16]; PAR_BLOCKS] = [[0; 16]; PAR_BLOCKS];
            for (dst_block, src_block) in pb_buf.iter_mut().zip(blocks.clone_in().as_slice().iter()) {
                for (dst, &src) in dst_block.iter_mut().zip(src_block.as_slice().iter()) {
                    *dst = src;
                }
            }
            let op = AesOp {
                domain: domain.to_owned(),
                block: AesBlockType::ParBlock(pb_buf),
                aes_op: AesOpType::Encrypt,
            };
            let mut buf = Buffer::into_buf(op).unwrap();
            buf.lend_mut(self.0.conn, Opcode::AesOracle.to_u32().unwrap())
                .expect("couldn't initiate encrypt_block operation");
            let ret_op = buf
                .as_flat::<AesOp, _>()
                .expect("got the wrong type of data structure back for encrypt_block");
            if let ArchivedAesBlockType::ParBlock(pb) = ret_op.block {
                for (b, pbs) in pb.iter().zip(blocks.get_out().iter_mut()) {
                    for (&src, dst) in b.iter().zip(pbs.as_mut_slice().iter_mut()) {
                        *dst = src;
                    }
                }
            }
        }
    }
}

pub(crate) struct KeystoreDec<'a>(&'a Keystore);

impl<'a> BlockSizeUser for KeystoreDec<'a> {
    type BlockSize = U16;
}

impl<'a> ParBlocksSizeUser for KeystoreDec<'a> {
    type ParBlocksSize = U16;
}
impl<'a> BlockBackend for KeystoreDec<'a> {
    fn proc_block(&mut self, mut block: InOut<'_, '_, Block>) {
        if let Some(domain) = &self.0.domain {
            let op = AesOp {
                domain: domain.to_owned(),
                block: AesBlockType::SingleBlock(block.clone_in().as_slice().try_into().unwrap()),
                aes_op: AesOpType::Decrypt,
            };
            let mut buf = Buffer::into_buf(op).unwrap();
            buf.lend_mut(self.0.conn, Opcode::AesOracle.to_u32().unwrap())
                .expect("couldn't initiate encrypt_block operation");
            let ret_op = buf
                .as_flat::<AesOp, _>()
                .expect("got the wrong type of data structure back for encrypt_block");
            if let ArchivedAesBlockType::SingleBlock(b) = ret_op.block {
                *block.get_out() = *Block::from_slice(&b);
            }
        }
    }

    fn proc_par_blocks(&mut self, mut blocks: InOut<'_, '_, BatchBlocks>) {
        if let Some(domain) = &self.0.domain {
            let mut pb_buf: [[u8; 16]; 16] = [[0; 16]; 16];
            for (dst_block, src_block) in pb_buf.iter_mut().zip(blocks.clone_in().as_slice().iter()) {
                for (dst, &src) in dst_block.iter_mut().zip(src_block.as_slice().iter()) {
                    *dst = src;
                }
            }
            let op = AesOp {
                domain: domain.to_owned(),
                block: AesBlockType::ParBlock(pb_buf),
                aes_op: AesOpType::Decrypt,
            };
            let mut buf = Buffer::into_buf(op).unwrap();
            buf.lend_mut(self.0.conn, Opcode::AesOracle.to_u32().unwrap())
                .expect("couldn't initiate encrypt_block operation");
            let ret_op = buf
                .as_flat::<AesOp, _>()
                .expect("got the wrong type of data structure back for encrypt_block");
            if let ArchivedAesBlockType::ParBlock(pb) = ret_op.block {
                for (b, pbs) in pb.iter().zip(blocks.get_out().iter_mut()) {
                    for (&src, dst) in b.iter().zip(pbs.as_mut_slice().iter_mut()) {
                        *dst = src;
                    }
                }
            }
        }
    }
}
