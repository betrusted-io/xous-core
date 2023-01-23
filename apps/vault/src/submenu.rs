use gam::*;
use num_traits::*;

use locales::t;
use vault::VaultOp;
use crate::actions::ActionOp;

pub fn create_submenu(vault_conn: xous::CID, actions_conn: xous::CID, menu_mgr: xous::SID) -> MenuMatic {
    let mut menu_items = Vec::<MenuItem>::new();

    menu_items.push(MenuItem {
        name: xous_ipc::String::from_str(t!("vault.menu_edit", xous::LANG)),
        action_conn: Some(vault_conn),
        action_opcode: VaultOp::MenuEditStage1.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: xous_ipc::String::from_str(t!("vault.menu_delete", xous::LANG)),
        action_conn: Some(vault_conn),
        action_opcode: VaultOp::MenuDeleteStage1.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: xous_ipc::String::from_str(t!("vault.menu_unlock_basis", xous::LANG)),
        action_conn: Some(actions_conn),
        action_opcode: ActionOp::MenuUnlockBasis.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: xous_ipc::String::from_str(t!("vault.menu_manage_basis", xous::LANG)),
        action_conn: Some(actions_conn),
        action_opcode: ActionOp::MenuManageBasis.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: xous_ipc::String::from_str(t!("vault.menu_change_font", xous::LANG)),
        action_conn: Some(vault_conn),
        action_opcode: VaultOp::MenuChangeFont.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    #[cfg(feature="vault-testing")]
    menu_items.push(MenuItem {
        name: xous_ipc::String::from_str("Generate test vectors"),
        action_conn: Some(actions_conn),
        action_opcode: ActionOp::GenerateTests.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: xous_ipc::String::from_str(t!("vault.menu_readout_mode", xous::LANG)),
        action_conn: Some(vault_conn),
        action_opcode: VaultOp::MenuReadoutMode.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: xous_ipc::String::from_str(t!("vault.menu_close", xous::LANG)),
        action_conn: Some(actions_conn),
        action_opcode: ActionOp::MenuClose.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });

    menu_matic(menu_items, gam::APP_MENU_0_VAULT, Some(menu_mgr)).expect("couldn't create MenuMatic manager")
}
