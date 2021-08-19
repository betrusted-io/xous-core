use ime_frontend::ImeFrontEndApi;
use xous::msg_scalar_unpack;
use num_traits::*;
use xous_ipc::String;

use gam::*;

// clearly, this does not scale. but, it's a good demo of the menu system.
// probably what this will need to turn into is a menu-of-menus....
pub(crate) fn emoji_menu_thread() {
    let mut menu = Menu::new(crate::EMOJI_MENU_NAME);

    let xns = xous_names::XousNames::new().unwrap();
    let imef = ime_plugin_api::ImeFrontEnd::new(&xns).expect("Couldn't connect to IME front end");

    let smile_item = MenuItem {
        name: String::<64>::from_str("😃"),
        action_conn: imef.conn(),
        action_opcode: imef.getop_process_keys(),
        action_payload: MenuPayload::Scalar(['😃'.into(), '\u{0000}'.into(), '\u{0000}'.into(), '\u{0000}'.into(), ]),
        close_on_select: true,
    };
    menu.add_item(smile_item);

    let frown_item = MenuItem {
        name: String::<64>::from_str("😒"),
        action_conn: imef.conn(),
        action_opcode: imef.getop_process_keys(),
        action_payload: MenuPayload::Scalar(['😒'.into(), '\u{0000}'.into(), '\u{0000}'.into(), '\u{0000}'.into(), ]),
        close_on_select: true,
    };
    menu.add_item(frown_item);

    let thinking_item = MenuItem {
        name: String::<64>::from_str("🤔"),
        action_conn: imef.conn(),
        action_opcode: imef.getop_process_keys(),
        action_payload: MenuPayload::Scalar(['🤔'.into(), '\u{0000}'.into(), '\u{0000}'.into(), '\u{0000}'.into(), ]),
        close_on_select: true,
    };
    menu.add_item(thinking_item);

    let sweat_item = MenuItem {
        name: String::<64>::from_str("😅"),
        action_conn: imef.conn(),
        action_opcode: imef.getop_process_keys(),
        action_payload: MenuPayload::Scalar(['😅'.into(), '\u{0000}'.into(), '\u{0000}'.into(), '\u{0000}'.into(), ]),
        close_on_select: true,
    };
    menu.add_item(sweat_item);

    let rofl_item = MenuItem {
        name: String::<64>::from_str("🤣"),
        action_conn: imef.conn(),
        action_opcode: imef.getop_process_keys(),
        action_payload: MenuPayload::Scalar(['🤣'.into(), '\u{0000}'.into(), '\u{0000}'.into(), '\u{0000}'.into(), ]),
        close_on_select: true,
    };
    menu.add_item(rofl_item);

    let close_item = MenuItem {
        name: String::<64>::from_str("Close Menu"),
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