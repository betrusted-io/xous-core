use num_traits::*;
use ux_api::menu::*;

// use crate::ActionOp;
use crate::VaultOp;

pub fn create_submenu(vault_conn: xous::CID, _actions_conn: xous::CID, menu_mgr: xous::SID) -> MenuMatic {
    let mut menu_items = Vec::<MenuItem>::new();

    menu_items.push(MenuItem {
        name: String::from("Continue"),
        action_conn: Some(vault_conn),
        action_opcode: VaultOp::TourContinue.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: String::from("Skip tour"),
        action_conn: Some(vault_conn),
        action_opcode: VaultOp::TourLater.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: String::from("Never show again"),
        action_conn: Some(vault_conn),
        action_opcode: VaultOp::TourNever.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });

    menu_matic(menu_items, "Tour Options", Some(menu_mgr), vault_conn, VaultOp::MenuDone.to_usize().unwrap())
        .expect("couldn't create MenuMatic manager")
}
