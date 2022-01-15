use gam::*;
use locales::t;
use root_keys::RootKeys;
use std::sync::{Arc, Mutex};
use xous_ipc::String;
use num_traits::*;

use crate::StatusOpcode;

// this is the provider for the main menu, it's built into the GAM so we always have at least this
// root-level menu available
pub fn main_menu_thread(keys: Arc<Mutex<RootKeys>>, status_sid: xous::SID) {
    let key_conn = keys.lock().unwrap().conn();
    let status_conn = xous::connect(status_sid).unwrap();
    let mut menu = Menu::new(gam::api::MAIN_MENU_NAME);

    let xns = xous_names::XousNames::new().unwrap();
    let com = com::Com::new(&xns).unwrap();

    let blon_item = MenuItem {
        name: String::<64>::from_str(t!("mainmenu.backlighton", xous::LANG)),
        action_conn: com.conn(),
        action_opcode: com.getop_backlight(),
        action_payload: MenuPayload::Scalar([191 >> 3, 191 >> 3, 0, 0]),
        close_on_select: true,
    };
    menu.add_item(blon_item);

    let bloff_item = MenuItem {
        name: String::<64>::from_str(t!("mainmenu.backlightoff", xous::LANG)),
        action_conn: com.conn(),
        action_opcode: com.getop_backlight(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    };
    menu.add_item(bloff_item);

    let sleep_item = MenuItem {
        name: String::<64>::from_str(t!("mainmenu.sleep", xous::LANG)),
        action_conn: status_conn,
        action_opcode: StatusOpcode::TrySuspend.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    };
    menu.add_item(sleep_item);

    let key_init = keys.lock().unwrap().is_initialized().unwrap();
    if !key_init {
        let initkeys_item = MenuItem {
            name: String::<64>::from_str(t!("mainmenu.init_keys", xous::LANG)),
            action_conn: key_conn,
            action_opcode: keys.lock().unwrap().get_try_init_keys_op(),
            action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
            close_on_select: true,
        };
        menu.add_item(initkeys_item);
    } else {
        let provision_item = MenuItem {
            name: String::<64>::from_str(t!("mainmenu.provision_gateware", xous::LANG)),
            action_conn: key_conn,
            action_opcode: keys.lock().unwrap().get_update_gateware_op(),
            action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
            close_on_select: true,
        };
        menu.add_item(provision_item);

        let selfsign_item = MenuItem {
            name: String::<64>::from_str(t!("mainmenu.selfsign", xous::LANG)),
            action_conn: key_conn,
            action_opcode: keys.lock().unwrap().get_try_selfsign_op(),
            action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
            close_on_select: true,
        };
        menu.add_item(selfsign_item);
    }

    let setrtc_item = MenuItem {
        name: String::<64>::from_str(t!("mainmenu.set_rtc", xous::LANG)),
        action_conn: status_conn,
        action_opcode: StatusOpcode::UxSetTime.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    };
    menu.add_item(setrtc_item);

    let reboot_item = MenuItem {
        name: String::<64>::from_str(t!("mainmenu.reboot", xous::LANG)),
        action_conn: status_conn,
        action_opcode: StatusOpcode::Reboot.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: true,
    };
    menu.add_item(reboot_item);

    let close_item = MenuItem {
        name: String::<64>::from_str(t!("mainmenu.closemenu", xous::LANG)),
        action_conn: menu.gam.conn(),
        action_opcode: menu.gam.getop_revert_focus(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
        close_on_select: false, // don't close because we're already closing
    };
    menu.add_item(close_item);

    loop {
        let msg = xous::receive_message(menu.sid).unwrap();
        log::trace!("message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(MenuOpcode::Redraw) => {
                menu.redraw();
            }
            Some(MenuOpcode::Rawkeys) => xous::msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                let keys = [
                    if let Some(a) = core::char::from_u32(k1 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(k2 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(k3 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(k4 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                ];
                menu.key_event(keys);
            }),
            Some(MenuOpcode::Quit) => {
                break;
            }
            None => {
                log::error!("unknown opcode {:?}", msg.body.id());
            }
        }
    }
    log::trace!("menu thread exit, destroying servers");
    // do we want to add a deregister_ux call to the system?
    xous::destroy_server(menu.sid).unwrap();
}
