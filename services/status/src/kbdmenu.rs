use gam::*;
use num_traits::*;
use keyboard::KeyMap;

use crate::StatusOpcode;

pub fn create_kbd_menu(status_conn: xous::CID, kbd_mgr: xous::SID) -> MenuMatic {
    let mut menu_items = Vec::<MenuItem>::new();

    // what the Rust: I have a into trait for usize on KeyMap. I want to go to a u32 in a single line of code.
    // you would think you could do... `KeyMap::Qwerty.into::<usize>() as u32`. But no, you can't. Into doesn't
    // take a generic. But if I just did .into() with a u32 Rust isn't careless enough to assume I want to go to a usize
    // then a u32. But I really don't want to implement a second set of into() traits just for u32. So ugh, now I
    // create a typed binding and then cast that binding to a u32 to work around this. idk.
    let code: usize = KeyMap::Qwerty.into();
    menu_items.push(MenuItem {
        name: xous_ipc::String::from_str("QWERTY"),
        action_conn: Some(status_conn),
        action_opcode: StatusOpcode::SetKeyboard.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([code as u32, 0, 0, 0]),
        close_on_select: true,
    });
    let code: usize = KeyMap::Azerty.into();
    menu_items.push(MenuItem {
        name: xous_ipc::String::from_str("AZERTY"),
        action_conn: Some(status_conn),
        action_opcode: StatusOpcode::SetKeyboard.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([code as u32, 0, 0, 0]),
        close_on_select: true,
    });
    let code: usize = KeyMap::Qwertz.into();
    menu_items.push(MenuItem {
        name: xous_ipc::String::from_str("QWERTZ"),
        action_conn: Some(status_conn),
        action_opcode: StatusOpcode::SetKeyboard.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([code as u32, 0, 0, 0]),
        close_on_select: true,
    });
    let code: usize = KeyMap::Dvorak.into();
    menu_items.push(MenuItem {
        name: xous_ipc::String::from_str("Dvorak"),
        action_conn: Some(status_conn),
        action_opcode: StatusOpcode::SetKeyboard.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([code as u32, 0, 0, 0]),
        close_on_select: true,
    });
    #[cfg(feature="tts")]
    {
        let code: usize = KeyMap::Braille.into();
        menu_items.push(MenuItem {
            name: xous_ipc::String::from_str("Braille"),
            action_conn: Some(status_conn),
            action_opcode: StatusOpcode::SetKeyboard.to_u32().unwrap(),
            action_payload: MenuPayload::Scalar([code as u32, 0, 0, 0]),
            close_on_select: true,
        });
    }

    menu_matic(menu_items, gam::KBD_MENU_NAME, Some(kbd_mgr)).expect("couldn't create MenuMatic manager")
}
