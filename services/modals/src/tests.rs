#![allow(dead_code)]
use std::thread;
use gam::*;

use xous_names::XousNames;

pub(crate) fn spawn_test() {
    thread::spawn({
        move || {
            let xns = XousNames::new().unwrap();

            // test the modal dialog box function
            let modals = modals::Modals::new(&xns).unwrap();
            log::info!("test text input");
            match modals.get_text_input("Test input", Some(test_validator), None) {
                Ok(text) => {
                    log::info!("Input: {}", text.0);
                }
                _ => {
                    log::error!("get_text_input failed");
                }
            }
            log::info!("text input test done");

            log::info!("testing notification");
            modals.show_notification("This is a test!\n这是一个测验!").expect("notification failed");
            log::info!("notification test done");
        }
    });
}

fn test_validator(input: TextEntryPayload, _opcode: u32) -> Option<xous_ipc::String::<256>> {
    let text_str = input.as_str();
    match text_str.parse::<u32>() {
        Ok(_input_int) => None,
        _ => return Some(xous_ipc::String::<256>::from_str("enter an integer value"))
    }
}
