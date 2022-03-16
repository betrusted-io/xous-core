use gam::*;
use locales::t;
use root_keys::RootKeys;
use std::sync::{Arc, Mutex};
use xous_ipc::String;
use num_traits::*;

use crate::StatusOpcode;

#[allow(unused_variables)] // quiets a warning about unused com that is emitted in tts config. Would be nice to make this more targeted...
pub fn create_main_menu(keys: Arc<Mutex<RootKeys>>, status_conn: xous::CID, com: &com::Com) {
    let key_conn = keys.lock().unwrap().conn();

    let mut menuitems = Vec::<MenuItem>::new();

    // no backlight on versions with no display
    #[cfg(not(feature="tts"))]
    menuitems.push(MenuItem {
        name: String::from_str(t!("mainmenu.backlighton", xous::LANG)),
        action_conn: Some(com.conn()),
        action_opcode: com.getop_backlight(),
        action_payload: MenuPayload::Scalar([191 >> 3, 191 >> 3, 0, 0]),
        close_on_select: true,
    });

    #[cfg(not(feature="tts"))]
    menuitems.push(MenuItem {
        name: String::from_str(t!("mainmenu.backlightoff", xous::LANG)),
        action_conn: Some(com.conn()),
        action_opcode: com.getop_backlight(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });

    menuitems.push(MenuItem {
        name: String::from_str(t!("mainmenu.sleep", xous::LANG)),
        action_conn: Some(status_conn),
        action_opcode: StatusOpcode::TrySuspend.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });

    menuitems.push(MenuItem {
        name: String::from_str(t!("mainmenu.app", xous::LANG)),
        action_conn: Some(status_conn),
        action_opcode: StatusOpcode::SubmenuApp.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });

    let key_init = keys.lock().unwrap().is_initialized().unwrap();
    if !key_init {
        menuitems.push(MenuItem {
            name: String::from_str(t!("mainmenu.init_keys", xous::LANG)),
            action_conn: Some(key_conn),
            action_opcode: keys.lock().unwrap().get_try_init_keys_op(),
            action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
            close_on_select: true,
        });
    } else {
        menuitems.push(MenuItem {
            name: String::from_str(t!("mainmenu.provision_gateware", xous::LANG)),
            action_conn: Some(key_conn),
            action_opcode: keys.lock().unwrap().get_update_gateware_op(),
            action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
            close_on_select: true,
        });

        menuitems.push(MenuItem {
            name: String::from_str(t!("mainmenu.selfsign", xous::LANG)),
            action_conn: Some(key_conn),
            action_opcode: keys.lock().unwrap().get_try_selfsign_op(),
            action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
            close_on_select: true,
        });
    }

    menuitems.push(MenuItem {
        name: String::from_str(t!("mainmenu.set_rtc", xous::LANG)),
        action_conn: Some(status_conn),
        action_opcode: StatusOpcode::UxSetTime.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });

    menuitems.push(MenuItem {
        name: String::from_str(t!("mainmenu.reboot", xous::LANG)),
        action_conn: Some(status_conn),
        action_opcode: StatusOpcode::Reboot.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });

    menuitems.push(MenuItem {
        name: String::from_str(t!("mainmenu.pddb", xous::LANG)),
        action_conn: Some(status_conn),
        action_opcode: StatusOpcode::SubmenuPddb.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });

    menuitems.push(MenuItem {
        name: String::from_str(t!("mainmenu.kbd", xous::LANG)),
        action_conn: Some(status_conn),
        action_opcode: StatusOpcode::SubmenuKbd.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menuitems.push(MenuItem {
        name: String::from_str(t!("mainmenu.battery_disconnect", xous::LANG)),
        action_conn: Some(status_conn),
        action_opcode: StatusOpcode::BatteryDisconnect.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });
    menuitems.push(MenuItem {
        name: String::from_str(t!("mainmenu.closemenu", xous::LANG)),
        action_conn: None,
        action_opcode: 0,
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    });

    menu_matic(menuitems, MAIN_MENU_NAME, None);
}
