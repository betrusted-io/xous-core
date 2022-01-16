use ime_plugin_api::ImefOpcode;
use num_traits::*;
use xous_ipc::String;

use gam::*;

use crate::EMOJI_MENU_NAME;

// clearly, this does not scale. but, it's a good demo of the menu system.
// probably what this will need to turn into is a menu-of-menus....
pub(crate) fn emoji_menu(imef_conn: xous::CID) {
    // the menu-matic for the emoji menu has to be encapsulated in a thread otherwise we get a deadlock
    // while creating the menu items, because of the IMEF's unique place in the graphics hierarchy.
    let _ = std::thread::spawn({
        move || {
            menu_matic(
                vec![
                    MenuItem {
                        name: String::from_str("😃"),
                        action_conn: Some(imef_conn),
                        action_opcode: ImefOpcode::ProcessKeys.to_u32().unwrap(),
                        action_payload: MenuPayload::Scalar(['😃'.into(), '\u{0000}'.into(), '\u{0000}'.into(), '\u{0000}'.into(), ]),
                        close_on_select: true,
                    },
                    MenuItem {
                        name: String::from_str("😒"),
                        action_conn: Some(imef_conn),
                        action_opcode: ImefOpcode::ProcessKeys.to_u32().unwrap(),
                        action_payload: MenuPayload::Scalar(['😒'.into(), '\u{0000}'.into(), '\u{0000}'.into(), '\u{0000}'.into(), ]),
                        close_on_select: true,
                    },
                    MenuItem {
                        name: String::from_str("🤔"),
                        action_conn: Some(imef_conn),
                        action_opcode: ImefOpcode::ProcessKeys.to_u32().unwrap(),
                        action_payload: MenuPayload::Scalar(['🤔'.into(), '\u{0000}'.into(), '\u{0000}'.into(), '\u{0000}'.into(), ]),
                        close_on_select: true,
                    },
                    MenuItem {
                        name: String::from_str("😅"),
                        action_conn: Some(imef_conn),
                        action_opcode: ImefOpcode::ProcessKeys.to_u32().unwrap(),
                        action_payload: MenuPayload::Scalar(['😅'.into(), '\u{0000}'.into(), '\u{0000}'.into(), '\u{0000}'.into(), ]),
                        close_on_select: true,
                    },
                    MenuItem {
                        name: String::from_str("🤣"),
                        action_conn: Some(imef_conn),
                        action_opcode: ImefOpcode::ProcessKeys.to_u32().unwrap(),
                        action_payload: MenuPayload::Scalar(['🤣'.into(), '\u{0000}'.into(), '\u{0000}'.into(), '\u{0000}'.into(), ]),
                        close_on_select: true,
                    },
                    MenuItem {
                        name: String::from_str("Close Menu"),
                        action_conn: None,
                        action_opcode: 0,
                        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
                        close_on_select: true,
                    },
                ],
                EMOJI_MENU_NAME,
                None
            );
        }
    });
}
