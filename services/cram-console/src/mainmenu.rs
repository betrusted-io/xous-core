use String;
use gam::*;
use locales::t;
use num_traits::*;

use crate::StatusOpcode;

#[allow(unused_variables)] // quiets a warning about unused com that is emitted in tts config. Would be nice to make this more targeted...
pub fn create_main_menu(menu_management_sid: xous::SID, status_conn: xous::CID) -> MenuMatic {
    let mut menuitems = Vec::<MenuItem>::new();

    menuitems.push(MenuItem {
        name: String::from(t!("mainmenu.app", locales::LANG)),
        action_conn: Some(status_conn),
        action_opcode: StatusOpcode::SubmenuApp.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });

    #[cfg(feature = "pddb")]
    menuitems.push(MenuItem {
        name: String::from(t!("mainmenu.pddb", locales::LANG)),
        action_conn: Some(status_conn),
        action_opcode: StatusOpcode::SubmenuPddb.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });

    menuitems.push(MenuItem {
        name: String::from(t!("mainmenu.closemenu", locales::LANG)),
        action_conn: None,
        action_opcode: 0,
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });

    menu_matic(menuitems, MAIN_MENU_NAME, Some(menu_management_sid)).unwrap()
}
