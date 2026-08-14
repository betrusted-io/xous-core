use num_traits::*;
use ux_api::menu::*;

use crate::ActionOp;
use crate::VaultOp;

pub fn create_submenu(vault_conn: xous::CID, actions_conn: xous::CID, menu_mgr: xous::SID) -> MenuMatic {
    let mut menu_items = Vec::<MenuItem>::new();

    menu_items.push(MenuItem {
        name: String::from("Help"),
        action_conn: Some(vault_conn),
        action_opcode: VaultOp::DefconHelp.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: String::from("About"),
        action_conn: Some(vault_conn),
        action_opcode: VaultOp::About.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: String::from("Token Mode"),
        action_conn: Some(vault_conn),
        action_opcode: VaultOp::TokenMode.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: String::from("Close Menu"),
        action_conn: Some(actions_conn),
        action_opcode: ActionOp::MenuClose.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: String::from("Power Off"),
        action_conn: Some(vault_conn),
        action_opcode: VaultOp::PowerOff.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });

    menu_matic(menu_items, "DC34 Badge", Some(menu_mgr), vault_conn, VaultOp::MenuDone.to_usize().unwrap())
        .expect("couldn't create MenuMatic manager")
}
