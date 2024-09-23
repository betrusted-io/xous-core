use gam::*;
use locales::t;
use num_traits::*;

use crate::{StatusOpcode, app_autogen};

pub fn create_app_menu(status_conn: xous::CID) {
    let mut menu_items = Vec::<MenuItem>::new();

    menu_items.push(MenuItem {
        name: String::from(t!("appmenu.shellchat", locales::LANG)),
        action_conn: Some(status_conn),
        action_opcode: StatusOpcode::SwitchToShellchat.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });

    // insert the application menu items
    app_autogen::app_menu_items(&mut menu_items, status_conn);

    menu_items.push(MenuItem {
        name: String::from(t!("mainmenu.closemenu", locales::LANG)),
        action_conn: None,
        action_opcode: 0,
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_matic(menu_items, gam::APP_MENU_NAME, None);
}
