use gam::*;
use ime_plugin_api::ImefOpcode;
use num_traits::*;
use String;

// imef_conn must come from outside the scope of the macro because of hygeine rules.
macro_rules! emoji_item {
    ($emoji:expr, $imef_conn:ident) => {
        MenuItem {
            name: String::from(&$emoji.to_string()),
            action_conn: Some($imef_conn),
            action_opcode: ImefOpcode::ProcessKeys.to_u32().unwrap(),
            action_payload: MenuPayload::Scalar([
                $emoji.into(),
                '\u{0000}'.into(),
                '\u{0000}'.into(),
                '\u{0000}'.into(),
            ]),
            close_on_select: true,
        }
    };
}

// clearly, this does not scale. but, it's a good demo of the menu system.
// probably what this will need to turn into is a menu-of-menus....
pub(crate) fn emoji_menu(imef_conn: xous::CID) {
    // the menu-matic for the emoji menu has to be encapsulated in a thread otherwise we get a deadlock
    // while creating the menu items, because of the IMEF's unique place in the graphics hierarchy.
    let _ = std::thread::spawn({
        move || {
            menu_matic(
                vec![
                    emoji_item!('😃', imef_conn),
                    emoji_item!('😒', imef_conn),
                    emoji_item!('🤔', imef_conn),
                    emoji_item!('😅', imef_conn),
                    emoji_item!('🤣', imef_conn),
                    MenuItem {
                        name: String::from("Close Menu"),
                        action_conn: None,
                        action_opcode: 0,
                        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
                        close_on_select: true,
                    },
                ],
                gam::EMOJI_MENU_NAME,
                None,
            );
        }
    });
}
