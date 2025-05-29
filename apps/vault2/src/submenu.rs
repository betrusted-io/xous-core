use gam::*;
use locales::t;
use num_traits::*;
use vault::VaultOp;

use crate::actions::ActionOp;

pub fn create_submenu(vault_conn: xous::CID, actions_conn: xous::CID, menu_mgr: xous::SID) -> MenuMatic {
    let mut menu_items = Vec::<MenuItem>::new();

    menu_items.push(MenuItem {
        name: String::from(t!("vault.menu_edit", locales::LANG)),
        action_conn: Some(vault_conn),
        action_opcode: VaultOp::MenuEditStage1.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: String::from(t!("vault.menu_delete", locales::LANG)),
        action_conn: Some(vault_conn),
        action_opcode: VaultOp::MenuDeleteStage1.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: String::from(t!("vault.menu_unlock_basis", locales::LANG)),
        action_conn: Some(actions_conn),
        action_opcode: ActionOp::MenuUnlockBasis.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: String::from(t!("vault.menu_manage_basis", locales::LANG)),
        action_conn: Some(actions_conn),
        action_opcode: ActionOp::MenuManageBasis.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: String::from(t!("vault.menu_change_font", locales::LANG)),
        action_conn: Some(vault_conn),
        action_opcode: VaultOp::MenuChangeFont.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    #[cfg(feature = "vault-testing")]
    menu_items.push(MenuItem {
        name: String::from("Generate test vectors"),
        action_conn: Some(actions_conn),
        action_opcode: ActionOp::GenerateTests.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: String::from(t!("vault.menu_readout_mode", locales::LANG)),
        action_conn: Some(vault_conn),
        action_opcode: VaultOp::MenuReadoutMode.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: String::from(t!("prefs.autotype_rate", locales::LANG)),
        action_conn: Some(vault_conn),
        action_opcode: VaultOp::MenuAutotypeRate.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: String::from(t!("vault.menu_set_lefty_mode", locales::LANG)),
        action_conn: Some(vault_conn),
        action_opcode: VaultOp::MenuLeftyMode.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: String::from(t!("vault.menu_close", locales::LANG)),
        action_conn: Some(actions_conn),
        action_opcode: ActionOp::MenuClose.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });

    menu_matic(menu_items, gam::APP_MENU_0_VAULT, Some(menu_mgr)).expect("couldn't create MenuMatic manager")
}
