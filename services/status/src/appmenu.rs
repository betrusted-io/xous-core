use gam::*;
use locales::t;
use num_traits::*;

use crate::StatusOpcode;

pub fn create_app_menu(status_conn: xous::CID) {
    let mut menu_items = Vec::<MenuItem>::new();

    menu_items.push(MenuItem {
        name: xous_ipc::String::from_str(t!("appmenu.shellchat", xous::LANG)),
        action_conn: Some(status_conn),
        action_opcode: StatusOpcode::SwitchToShellchat.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });

    menu_items.push(MenuItem {
        name: xous_ipc::String::from_str(t!("appmenu.ball", xous::LANG)),
        action_conn: Some(status_conn),
        action_opcode: StatusOpcode::SwitchToBall.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });

    menu_items.push(MenuItem {
        name: xous_ipc::String::from_str(t!("mainmenu.closemenu", xous::LANG)),
        action_conn: None,
        action_opcode: 0,
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_matic(menu_items, gam::APP_MENU_NAME, None);
}
