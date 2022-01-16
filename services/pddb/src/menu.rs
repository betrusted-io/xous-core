use xous::msg_scalar_unpack;
use num_traits::*;
use xous_ipc::String;
use locales::t;
use crate::api::*;

use gam::*;

pub(crate) const PDDB_MENU_NAME: &'static str = "pddb menu";

pub(crate) fn pddb_menu(conn: xous::CID) {
    let mut menu = Menu::new(PDDB_MENU_NAME);

    menu.add_item(
        MenuItem {
            name: String::from_str(t!("pddb.menu.listbasis", xous::LANG)),
            action_conn: Some(conn),
            action_opcode: Opcode::MenuListBasis.to_u32().unwrap(),
            action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
            close_on_select: true,
        }
    );
    loop {
        let msg = xous::receive_message(menu.sid).unwrap();
        log::trace!("message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(MenuOpcode::Redraw) => {
                menu.redraw();
            },
            Some(MenuOpcode::Rawkeys) => msg_scalar_unpack!(msg, k1, k2, k3, k4, {
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
            },
            None => {
                log::error!("unknown opcode {:?}", msg.body.id());
            }
        }
    }
    log::trace!("menu thread exit, destroying servers");
    // do we want to add a deregister_ux call to the system?
    xous::destroy_server(menu.sid).unwrap();
}