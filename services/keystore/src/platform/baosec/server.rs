use bao1x_hal::board::{BOOKEND_END, BOOKEND_START};
use bao1x_hal::rram::Reram;
use keystore_api::*;
use xous::SID;
use xous_ipc::Buffer;

use crate::platform::KeyStore;

pub fn keystore(sid: SID) -> ! {
    let hal = bao1x_hal_service::Hal::new();
    let mut rram = Reram::new();
    let mut store = KeyStore::init_mappings(&mut rram);
    let mut backup_mgr = bao1x_hal::buram::BackupManager::new();
    // does nothing if the keys are already initialized
    store.ensure_system_init(&mut rram);
    // derive the master key
    store.derive_master_key();

    if store.is_collateral_erased() {
        log::info!("{}COLLATERAL.ERASED,{}", BOOKEND_START, BOOKEND_END);
    } else {
        panic!("Collateral is not erased - protocol error for Baochip firmwares!");
    }

    let mut msg_opt = None;

    // allow preemption once the keystore has claimed locks on all its critical resources
    hal.set_preemption(true);
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
                store.aes_op(&mut aes_op).expect("couldn't perform AES op");
                buffer.replace(aes_op).unwrap();
            }
            Opcode::AesKwp => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut kwp = buffer.to_original::<KeyWrapper, _>().unwrap();
                store.aes_kwp(&mut kwp).expect("couldn't wrap key");
                buffer.replace(kwp).unwrap();
            }
            Opcode::EphemeralOp => {
                // this will use the backup_mgr to store ephemeral secrets
                todo!()
            }
            Opcode::Bootwait => {
                if let Some(scalar) = msg.body.scalar_message_mut() {
                    let is_some = scalar.arg1 != 0;
                    let enable = scalar.arg2 != 0;
                    match store.set_bootwait(if is_some { Some(enable) } else { None }) {
                        Ok(last) => {
                            if last {
                                scalar.arg1 = 1;
                            } else {
                                scalar.arg1 = 0;
                            }
                        }
                        _ => panic!("Couldn't set bootwait"),
                    }
                }
            }
            Opcode::InvalidCall => {
                log::error!("Invalid call in keystore: {:?}", opcode);
            }
        }
    }
}
