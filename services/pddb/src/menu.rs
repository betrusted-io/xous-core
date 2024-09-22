use gam::*;
use locales::t;
use num_traits::*;
use String;

use crate::api::*;

pub(crate) fn pddb_menu(conn: xous::CID) {
    let mut menu_items = Vec::<MenuItem>::new();

    menu_items.push(MenuItem {
        name: String::from(t!("pddb.menu.listbasis", locales::LANG)),
        action_conn: Some(conn),
        action_opcode: Opcode::MenuListBasis.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: String::from(t!("pddb.menu.change_unlock_pin", locales::LANG)),
        action_conn: Some(conn),
        action_opcode: Opcode::MenuChangePin.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menu_items.push(MenuItem {
        name: String::from(t!("mainmenu.closemenu", locales::LANG)),
        action_conn: None,
        action_opcode: 0,
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });

    menu_matic(menu_items, PDDB_MENU_NAME, None);
}
