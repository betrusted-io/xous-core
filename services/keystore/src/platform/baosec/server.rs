use bao1x_api::{BackupFlags, OneWayErr};
use bao1x_hal::acram::MAX_ONEWAY_COUNTERS;
use bao1x_hal::board::{BOOKEND_END, BOOKEND_START};
use bao1x_hal::rram::Reram;
use keystore_api::*;
use num_traits::*;
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
        log::error!("Collateral is not erased - protocol error for Baochip firmwares!");
        log::info!("{}COLLATERAL.FAIL,{}", BOOKEND_START, BOOKEND_END);
    }

    #[cfg(feature = "swap")]
    let xns = xous_names::XousNames::new().unwrap();

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
                if let Some(mem) = msg.body.memory_message_mut() {
                    let mut buffer = unsafe { Buffer::from_memory_message_mut(mem) };
                    // as_flat saves a copy step, but we have to deserialize some enums manually
                    let mut aes_op = buffer.to_original::<AesOp, _>().unwrap();
                    // TODO: add hardening checks to the deserialization of AesOp. Can't assume the
                    // values are correct coming into this server.
                    store.aes_op(&mut aes_op).expect("couldn't perform AES op");
                    buffer.replace(aes_op).unwrap();
                }
            }
            Opcode::AesKwp => {
                if let Some(mem) = msg.body.memory_message_mut() {
                    let mut buffer = unsafe { Buffer::from_memory_message_mut(mem) };
                    let mut kwp = buffer.to_original::<KeyWrapper, _>().unwrap();
                    // TODO: add hardening checks to the deserialization of AesOp. Can't assume the
                    // values are correct coming into this server.
                    store.aes_kwp(&mut kwp).expect("couldn't wrap key");
                    buffer.replace(kwp).unwrap();
                }
            }
            Opcode::Ephemeral => {
                if let Some(scalar) = msg.body.scalar_message_mut() {
                    let ephemeral_op: EphemeralOp =
                        num_traits::FromPrimitive::from_usize(scalar.arg1).expect("invalid ephemeral op");
                    let offset = match ephemeral_op {
                        EphemeralOp::GetLsb | EphemeralOp::SetLsb => 0,
                        EphemeralOp::GetMsb | EphemeralOp::SetMsb => bao1x_hal::buram::KEY_LEN / 2,
                    };
                    match ephemeral_op {
                        EphemeralOp::SetLsb | EphemeralOp::SetMsb => {
                            let mut key = backup_mgr.get_backup_key();
                            key[offset..offset + 4].copy_from_slice(&scalar.arg2.to_le_bytes());
                            key[offset + 4..offset + 8].copy_from_slice(&scalar.arg3.to_le_bytes());
                            key[offset + 8..offset + 12].copy_from_slice(&scalar.arg4.to_le_bytes());
                            // zeroize
                            scalar.arg1 = 0;
                            scalar.arg2 = 0;
                            scalar.arg3 = 0;
                            scalar.arg4 = 0;
                            backup_mgr.set_backup_key(key);
                        }
                        EphemeralOp::GetLsb | EphemeralOp::GetMsb => {
                            let key = backup_mgr.get_backup_key();
                            scalar.arg2 =
                                u32::from_le_bytes(key[offset..offset + 4].try_into().unwrap()) as usize;
                            scalar.arg3 =
                                u32::from_le_bytes(key[offset + 4..offset + 8].try_into().unwrap()) as usize;
                            scalar.arg4 =
                                u32::from_le_bytes(key[offset + 8..offset + 12].try_into().unwrap()) as usize;
                        }
                    }
                }
            }
            Opcode::GetFlags => {
                if let Some(scalar) = msg.body.scalar_message_mut() {
                    let flags = backup_mgr.get_flags();
                    scalar.arg1 = flags.raw_value() as usize;
                }
            }
            Opcode::SetFlags => {
                if let Some(scalar) = msg.body.scalar_message_mut() {
                    let flag = BackupFlags::new_with_raw_value(scalar.arg1 as u32);
                    backup_mgr.set_flags(flag);
                }
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
            Opcode::IsDeveloper => {
                if let Some(scalar) = msg.body.scalar_message_mut() {
                    if store.is_developer() { scalar.arg1 = 0 } else { scalar.arg1 = 1 }
                }
            }
            #[cfg(feature = "owc-inc")]
            Opcode::IncOneWayCounter => {
                if let Some(scalar) = msg.body.scalar_message_mut() {
                    if [scalar.arg2, scalar.arg3, scalar.arg4] == OWC_MAGIC_INC
                        && scalar.arg1 < MAX_ONEWAY_COUNTERS
                    {
                        match unsafe { store.owc.inc(scalar.arg1) } {
                            Ok(_) => {
                                scalar.arg1 = OneWayErr::None.to_usize().unwrap();
                            }
                            Err(e) => scalar.arg1 = e.to_usize().unwrap(),
                        }
                    } else {
                        scalar.arg1 = OneWayErr::InternalError.to_usize().unwrap();
                    }
                }
            }
            Opcode::GetOneWayCounter => {
                if let Some(scalar) = msg.body.scalar_message_mut() {
                    if [scalar.arg2, scalar.arg3, scalar.arg4] == OWC_MAGIC_GET
                        && scalar.arg1 < MAX_ONEWAY_COUNTERS
                    {
                        match store.owc.get(scalar.arg1) {
                            Ok(val) => {
                                scalar.arg1 = OneWayErr::None.to_usize().unwrap();
                                scalar.arg2 = val as usize;
                            }
                            Err(e) => scalar.arg1 = e.to_usize().unwrap(),
                        }
                    } else {
                        scalar.arg1 = OneWayErr::InternalError.to_usize().unwrap();
                    }
                }
            }
            #[cfg(feature = "app-keys")]
            Opcode::AppKeyOp => {
                if let Some(mem) = msg.body.memory_message_mut() {
                    let mut buffer = unsafe { Buffer::from_memory_message_mut(mem) };
                    // as_flat saves a copy step, but we have to deserialize some enums manually
                    let mut app_key = buffer.to_original::<AppKey, _>().unwrap();
                    match app_key {
                        AppKey::ReadRequest { guard, index } => {
                            if guard != APPKEY_GUARD {
                                app_key = AppKey::InternalError;
                            } else {
                                match store.read_app_key(index) {
                                    Ok(d) => app_key = AppKey::ReadResponse { data: d },
                                    Err(xous::Error::AccessDenied) => app_key = AppKey::AccessDenied,
                                    _ => app_key = AppKey::InternalError,
                                }
                            }
                        }
                        AppKey::Write { guard, index, data } => {
                            if guard != APPKEY_GUARD {
                                app_key = AppKey::InternalError
                            } else {
                                match store.write_app_key(&mut rram, index, data) {
                                    Ok(_) => app_key = AppKey::Success,
                                    Err(xous::Error::AccessDenied) => app_key = AppKey::AccessDenied,
                                    Err(xous::Error::HardwareError) => app_key = AppKey::WriteError,
                                    _ => app_key = AppKey::InternalError,
                                }
                            }
                        }
                        _ => app_key = AppKey::InternalError,
                    }
                    buffer.replace(app_key).unwrap();
                }
            }
            #[cfg(feature = "swap")]
            Opcode::EnsureSwapEncryption => {
                if let Some(mem) = msg.body.memory_message() {
                    let buffer = unsafe { Buffer::from_memory_message(mem) };
                    let swap_call = buffer.to_original::<SwapEncryptCall, _>().unwrap();
                    if let Ok(cid) = xns.request_connection_blocking(&swap_call.status_server) {
                        #[cfg(feature = "legacy-swap-key")]
                        let key = store.get_swap_key(&mut rram);
                        #[cfg(not(feature = "legacy-swap-key"))]
                        let key = store.get_swap_key();
                        if !store.is_developer() {
                            let _ = store
                                .ensure_swap_encryption(cid, swap_call.status_opcode, swap_call.token, key)
                                .inspect_err(|e| log::info!("Couldn't ensure swap encryption: {:?}", e));
                        }
                        // set completion to 100%
                        xous::try_send_message(
                            cid,
                            xous::Message::new_scalar(
                                swap_call.status_opcode as usize,
                                100,
                                swap_call.token[0] as usize,
                                swap_call.token[1] as usize,
                                swap_call.token[2] as usize,
                            ),
                        )
                        .ok();
                        log::info!("{}SWAP.ENCRYPTED,{}", BOOKEND_START, BOOKEND_END);
                    } else {
                        log::warn!(
                            "Swap encryption check skipped because the caller provided an invalid server name for status feedback"
                        );
                    }
                }
            }
            Opcode::InvalidCall => {
                log::error!("Invalid call in keystore: {:?}", opcode);
            }
        }
    }
}
