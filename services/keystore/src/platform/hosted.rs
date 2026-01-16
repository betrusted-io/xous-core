use std::convert::TryInto;

use aes::Aes256;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray};
use hkdf::Hkdf;
use keystore_api::*;
use sha2::Sha256;
use xous::SID;
use xous_ipc::Buffer;

/// Any old key for the hosted mode testing. 0 is as good a number as any other.
const WELL_KNOWN_KEY: &'static str = "0000000000000000000000000000000000000000000000000000000000000000";

pub fn keystore(sid: SID) -> ! {
    let mut msg_opt = None;

    loop {
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        let msg = msg_opt.as_mut().unwrap();
        let opcode = num_traits::FromPrimitive::from_usize(msg.body.id()).unwrap_or(Opcode::InvalidCall);
        log::debug!("{:?}", opcode);
        match opcode {
            Opcode::AesOracle => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                // as_flat saves a copy step, but we have to deserialize some enums manually
                let mut aes_op = buffer.to_original::<AesOp, _>().unwrap();
                let op = match aes_op.aes_op {
                    // seems stupid, but we have to do this because we want to have zeroize on the AesOp
                    // record, and it means we can't have Copy on this.
                    AesOpType::Decrypt => AesOpType::Decrypt,
                    AesOpType::Encrypt => AesOpType::Encrypt,
                };
                let mut ikm = [0u8; 32];
                ikm.copy_from_slice(&hex::decode(WELL_KNOWN_KEY).unwrap());
                let hk = Hkdf::<Sha256>::new(None, &ikm);
                let mut okm = [0u8; 32];
                hk.expand(&aes_op.domain.as_bytes(), &mut okm)
                    .expect("32 is a valid length for Sha256 to output");
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
                buffer.replace(aes_op).unwrap();
            }
            Opcode::AesKwp => {
                let mut ikm = [0u8; 32];
                ikm.copy_from_slice(&hex::decode(WELL_KNOWN_KEY).unwrap());
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut kwp = buffer.to_original::<KeyWrapper, _>().unwrap();
                use aes_kw::Kek;
                use aes_kw::KekAes256;
                let keywrapper: KekAes256 = Kek::from(ikm);
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
                buffer.replace(kwp).unwrap();
            }
            Opcode::EphemeralOp => {
                todo!()
            }
            _ => {
                log::error!("Invalid call in keystore: {:?}", opcode);
            }
        }
    }
}
