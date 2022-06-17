use gam::*;
use num_traits::*;

use locales::t;
use crate::VaultOp;

pub fn create_submenu(vault_conn: xous::CID, menu_mgr: xous::SID) -> MenuMatic {
    let mut menu_items = Vec::<MenuItem>::new();

    /* menu_items.push(MenuItem {
        name: xous_ipc::String::from_str(t!("vault.menu_autotype", xous::LANG)),
        action_conn: Some(vault_conn),
        action_opcode: VaultOp::MenuAutotype.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    }); */
    menu_items.push(MenuItem {
        name: xous_ipc::String::from_str(t!("vault.menu_edit", xous::LANG)),
        action_conn: Some(vault_conn),
        action_opcode: VaultOp::MenuEdit.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: xous_ipc::String::from_str(t!("vault.menu_delete", xous::LANG)),
        action_conn: Some(vault_conn),
        action_opcode: VaultOp::MenuDelete.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: xous_ipc::String::from_str(t!("vault.menu_close", xous::LANG)),
        action_conn: None,
        action_opcode: 0,
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });

    menu_matic(menu_items, gam::APP_MENU_0_VAULT, Some(menu_mgr)).expect("couldn't create MenuMatic manager")
}
