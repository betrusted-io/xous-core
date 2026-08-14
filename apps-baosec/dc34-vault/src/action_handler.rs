use std::sync::{Arc, Mutex, atomic::AtomicBool};
use std::thread;

use num_traits::*;
use xous::msg_blocking_scalar_unpack;
use xous_ipc::Buffer;

use crate::actions::ActionOp;
use crate::{VaultMode, itemcache::ItemLists};

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
pub struct SelectedEntry {
    pub key_guid: String,
    pub description: String,
    pub mode: VaultMode,
}

pub(crate) fn action_handler(
    main_conn: xous::CID,
    sid: xous::SID,
    mode: Arc<Mutex<VaultMode>>,
    item_lists: Arc<Mutex<ItemLists>>,
    action_active: Arc<AtomicBool>,
) {
    let _ = thread::spawn({
        move || {
            let mut manager = crate::actions::ActionManager::new(main_conn, mode, item_lists, action_active);
            loop {
                let msg = xous::receive_message(sid).unwrap();
                let opcode: Option<ActionOp> = FromPrimitive::from_usize(msg.body.id());
                log::debug!("{:?}", opcode);
                match opcode {
                    Some(ActionOp::MenuAddnew) => {
                        manager.activate();
                        manager.menu_addnew(); // this is responsible for updating the item cache
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuDeleteStage2) => {
                        let buffer =
                            unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let entry = buffer.to_original::<SelectedEntry, _>().unwrap();
                        manager.activate();
                        manager.menu_delete(entry);
                        manager.retrieve_db();
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuEditStage2) => {
                        let buffer =
                            unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let entry = buffer.to_original::<SelectedEntry, _>().unwrap();
                        manager.activate();
                        manager.menu_edit(&entry); // this is responsible for updating the item cache
                        manager.update_db_entry(&entry);
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuUnlockBasis) => {
                        manager.activate();
                        manager.unlock_basis();
                        manager.item_lists.lock().unwrap().clear(VaultMode::Password); // clear the cached item list for passwords (totp/fido are not cached and don't need clearing)
                        manager.retrieve_db();
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuManageBasis) => {
                        manager.activate();
                        manager.manage_basis();
                        manager.item_lists.lock().unwrap().clear(VaultMode::Password); // clear the cached item list for passwords
                        manager.retrieve_db();
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuClose) => {
                        // dummy activate/de-activate cycle because we have to trigger a redraw of the
                        // underlying UX
                        manager.activate();
                        manager.deactivate();
                    }
                    Some(ActionOp::UpdateOneItem) => {
                        let buffer =
                            unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let entry = buffer.to_original::<SelectedEntry, _>().unwrap();
                        manager.activate();
                        manager.update_db_entry(&entry);
                        manager.deactivate();
                    }
                    Some(ActionOp::UpdateMode) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        // the password DBs are now not shared between modes, so no need to re-retrieve it.
                        if manager.is_db_empty() {
                            manager.retrieve_db();
                        }
                        xous::return_scalar(msg.sender, 1).unwrap();
                    }),
                    Some(ActionOp::ReloadDb) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        manager.retrieve_db();
                        xous::return_scalar(msg.sender, 1).unwrap();
                    }),
                    Some(ActionOp::AcquireQr) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        manager.acquire_qr();
                        manager.retrieve_db();
                        xous::return_scalar(msg.sender, 1).unwrap();
                    }),
                    Some(ActionOp::Quit) => {
                        break;
                    }
                    None => {
                        log::error!("msg could not be decoded {:?}", msg);
                    }
                    #[cfg(feature = "vault-testing")]
                    Some(ActionOp::GenerateTests) => {
                        manager.populate_tests();
                        manager.retrieve_db();
                    }
                }
            }
            xous::destroy_server(sid).ok();
        }
    });
}
