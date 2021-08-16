#![cfg_attr(all(target_os = "none", not(test)), no_std)]
//! Detailed docs are parked under Structs/RootKeys down below

pub mod api;
use api::*;

pub mod key2bits;

use xous::{CID, send_message, Message};
use xous_ipc::Buffer;
use num_traits::*;
use std::convert::TryInto;

pub use cipher::{self, BlockCipher, BlockDecrypt, BlockEncrypt, consts::U16};

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
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_KEYS).expect("Can't connect to Keys server");
        let index = if let Some(ki) = key_index {
            ki
        } else {
            AesRootkeyType::NoneSpecified
        };
        Ok(RootKeys {
            conn,
            key_index: index,
        })
    }
    pub fn conn(&self) -> CID {self.conn}
    pub fn get_try_init_keys_op(&self) -> u32 {
        Opcode::UxTryInitKeys.to_u32().unwrap()
    }
    pub fn get_update_gateware_op(&self) -> u32 {
        Opcode::UxUpdateGateware.to_u32().unwrap()
    }
    pub fn get_try_selfsign_op(&self) -> u32 {
        Opcode::UxSelfSignXous.to_u32().unwrap()
    }

    /// this function takes a boot gateware that has a "null" key (all zeros) and:
    /// 0. confirms that JTAG is accessible by reading out the ID, and that the eFuse is writeable, and all 0's
    /// 1. prompts of the update password, and confirms that the existing efuse key decrypts to 0. if not -- we were already fused, abort.
    /// 2. creates a new eFuse key using the TRNG
    /// 3. inserts a revised KEYROM into the base image while copying & encrypting with the new key to the staging area
    /// 4. verifies the xilinx-hmac on the staged image
    /// 5. copies the staged image into the boot area
    /// 6. burns the eFuse key using JTAG-local commands
    /// 7. suspends the device with auto-resume so that the new gateware is in effect
    /// 8. reads back the eFuse key from the KEYROM to confirm everything went as planned, compares to previously computed result
    /// 9. clears the eFuse key from RAM.
    pub fn seal_boot_gateware(&mut self) -> Result<(), xous::Error> {
        unimplemented!();
    }

    /// this initiates an attempt to update passwords. User must unlock their device first, and can cancel out if not expected.
    pub fn try_update_password(&mut self, _which: PasswordType) -> Result<(), xous::Error> {
        unimplemented!();
    }

    /// checks to see if the KEYROM has been initialized, and if not, generates keys. In the process of doing so, the user will be
    /// prompted to enter passwords. It also automatically self-signs everything -- presumably, if you were comfortable enough to
    /// use this firmware to make your keys, you also trusted it.
    /// it will then update the bitstream with your keys.
    pub fn try_init_keys(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::UxTryInitKeys.to_usize().unwrap(),
            0, 0, 0, 0)
        ).map(|_| ())
    }

    pub fn is_initialized(&self) -> Result<bool, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::KeysInitialized.to_usize().unwrap(), 0, 0, 0, 0)
        ).expect("couldn't send KeysInitialized check message");
        if let xous::Result::Scalar1(result) = response {
            if result != 0 {return Ok(true)} else {return Ok(false)}
        } else {
            log::error!("unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }

    /// this will check the signature on the gateware.
    /// returns None if no keys have been initialized
    /// returns true if the gateware passes, false if it fails
    pub fn check_gateware_signature(&self) -> Result<Option<bool>, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::CheckGatewareSignature.to_usize().unwrap(), 0, 0, 0, 0)
        )?;
        if let xous::Result::Scalar1(result) = response {
            if result == 2 { // uninit keys case
                Ok(None)
            } else if result == 1 { // passed
                Ok(Some(true))
            } else { // everything else -- fail
                Ok(Some(false))
            }
        } else {
            Err(xous::Error::InternalError)
        }
    }

    pub fn is_efuse_secured(&self) -> Result<Option<bool>, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::IsEfuseSecured.to_usize().unwrap(), 0, 0, 0, 0)
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
    pub fn is_jtag_working(&self) -> Result<bool, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::IsJtagWorking.to_usize().unwrap(), 0, 0, 0, 0)
        )?;
        if let xous::Result::Scalar1(result) = response {
            if result == 1 {
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Err(xous::Error::InternalError)
        }
    }

    pub fn test_ux(&mut self, arg: usize) {
        send_message(self.conn,
            Message::new_scalar(Opcode::TestUx.to_usize().unwrap(),
            arg, 0, 0, 0)
        ).expect("couldn't send test message");
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for RootKeys {
    fn drop(&mut self) {
        log::debug!("dropping rootkeys object");
        // the connection to the server side must be reference counted, so that multiple instances of this object within
        // a single process do not end up de-allocating the CID on other threads before they go out of scope.
        // Note to future me: you want this. Don't get rid of it because you think, "nah, nobody will ever make more than one copy of this object".
        if REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
    }
}

/// 128-bit AES block
pub type Block = cipher::generic_array::GenericArray<u8, cipher::consts::U16>;
/// 16 x 128-bit AES blocks to be processed in bulk
pub type ParBlocks = cipher::generic_array::GenericArray<Block, cipher::consts::U16>;

impl BlockCipher for RootKeys {
    type BlockSize = U16;   // 128-bit cipher
    type ParBlocks = U16;   // 256-byte "chunk" if doing more than one block at a time, for better efficiency
}

impl BlockEncrypt for RootKeys {
    fn encrypt_block(&self, block: &mut Block) {
        let op = AesOp {
            key_index: self.key_index.to_u8().unwrap(),
            block: AesBlockType::SingleBlock(block.as_slice().try_into().unwrap()),
            aes_op: AesOpType::Encrypt,
        };
        let mut buf = Buffer::into_buf(op).unwrap();
        buf.lend_mut(self.conn, Opcode::AesOperation.to_u32().unwrap()).expect("couldn't initiate encrypt_block operation");
        let ret_op = buf.as_flat::<AesOp, _>().expect("got the wrong type of data structure back for encrypt_block");
        if let ArchivedAesBlockType::SingleBlock(b) = ret_op.block {
            for (&src, dst) in b.iter().zip(block.as_mut_slice().iter_mut()) {
                *dst = src;
            }
        }
    }
    fn encrypt_par_blocks(&self, blocks: &mut ParBlocks) {
        let mut pb_buf: [[u8; 16]; 16] = [[0; 16]; 16];
        for (dst_block, src_block) in pb_buf.iter_mut().zip(blocks.as_slice().iter()) {
            for (dst, &src) in dst_block.iter_mut().zip(src_block.as_slice().iter()) {
                *dst = src;
            }
        }
        let op = AesOp {
            key_index: self.key_index.to_u8().unwrap(),
            block: AesBlockType::ParBlock(pb_buf),
            aes_op: AesOpType::Encrypt,
        };
        let mut buf = Buffer::into_buf(op).unwrap();
        buf.lend_mut(self.conn, Opcode::AesOperation.to_u32().unwrap()).expect("couldn't initiate encrypt_block operation");
        let ret_op = buf.as_flat::<AesOp, _>().expect("got the wrong type of data structure back for encrypt_block");
        if let ArchivedAesBlockType::ParBlock(pb) = ret_op.block {
            for (b, pbs) in pb.iter().zip(blocks.as_mut_slice().iter_mut()) {
                for (&src, dst) in b.iter().zip(pbs.as_mut_slice().iter_mut()) {
                    *dst = src;
                }
            }
        }
    }
}

impl BlockDecrypt for RootKeys {
    fn decrypt_block(&self, block: &mut Block) {
        let op = AesOp {
            key_index: self.key_index.to_u8().unwrap(),
            block: AesBlockType::SingleBlock(block.as_slice().try_into().unwrap()),
            aes_op: AesOpType::Decrypt,
        };
        let mut buf = Buffer::into_buf(op).unwrap();
        buf.lend_mut(self.conn, Opcode::AesOperation.to_u32().unwrap()).expect("couldn't initiate encrypt_block operation");
        let ret_op = buf.as_flat::<AesOp, _>().expect("got the wrong type of data structure back for encrypt_block");
        if let ArchivedAesBlockType::SingleBlock(b) = ret_op.block {
            for (&src, dst) in b.iter().zip(block.as_mut_slice().iter_mut()) {
                *dst = src;
            }
        }
    }
    fn decrypt_par_blocks(&self, blocks: &mut ParBlocks) {
        let mut pb_buf: [[u8; 16]; 16] = [[0; 16]; 16];
        for (dst_block, src_block) in pb_buf.iter_mut().zip(blocks.as_slice().iter()) {
            for (dst, &src) in dst_block.iter_mut().zip(src_block.as_slice().iter()) {
                *dst = src;
            }
        }
        let op = AesOp {
            key_index: self.key_index.to_u8().unwrap(),
            block: AesBlockType::ParBlock(pb_buf),
            aes_op: AesOpType::Decrypt,
        };
        let mut buf = Buffer::into_buf(op).unwrap();
        buf.lend_mut(self.conn, Opcode::AesOperation.to_u32().unwrap()).expect("couldn't initiate encrypt_block operation");
        let ret_op = buf.as_flat::<AesOp, _>().expect("got the wrong type of data structure back for encrypt_block");
        if let ArchivedAesBlockType::ParBlock(pb) = ret_op.block {
            for (b, pbs) in pb.iter().zip(blocks.as_mut_slice().iter_mut()) {
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
    use super::*;

    #[test]
    fn hash_with_fixed_salt() {
        let salt: [u8; 16] = [
            38, 113, 212, 141, 108, 213, 195, 166, 201, 38, 20, 13, 47, 40, 104, 18,
        ];
        let mut output: [u8; 24] = [0; 24];

        let pw = "My S3cre7 P@55w0rd!";

        crate::bcrypt::bcrypt(5,  &salt, pw, &mut output);

        assert_eq!(output, [22, 80, 102, 192, 193, 204, 118, 167, 41, 102, 241, 75, 103, 49, 4, 245, 194, 145, 85, 104, 179, 60, 88, 53]);
    }

    #[test]
    fn hash_with_max_len() {
        let salt: [u8; 16] = [
            38, 113, 212, 141, 108, 213, 195, 166, 201, 38, 20, 13, 47, 40, 104, 18,
        ];
        let mut output: [u8; 24] = [0; 24];
        let pw = "this is a test of a very long password that is exactly 72 characters lon";
        crate::bcrypt::bcrypt(10,  &salt, pw, &mut output);
        assert_eq!(output, [46, 39, 41, 217, 39, 103, 62, 189, 120, 3, 248, 84, 175, 40, 134, 190, 76, 43, 232, 147, 129, 237, 116, 61]);
    }

    #[test]
    fn hash_with_longer_than_max_len() {
        let salt: [u8; 16] = [
            38, 113, 212, 141, 108, 213, 195, 166, 201, 38, 20, 13, 47, 40, 104, 18,
        ];
        let mut output: [u8; 24] = [0; 24];
        let pw = "this is a test of a very long password that is exactly 72 characters long, but this one is even longer";
        crate::bcrypt::bcrypt(10,  &salt, pw, &mut output);
        assert_eq!(output, [46, 39, 41, 217, 39, 103, 62, 189, 120, 3, 248, 84, 175, 40, 134, 190, 76, 43, 232, 147, 129, 237, 116, 61]);
    }
}