#![allow(dead_code)]
use std::thread;
use gam::*;

use xous_names::XousNames;

const RADIO_TEST: [&'static str; 4] = [
    "zebra",
    "cow",
    "horse",
    "cat",
];

const CHECKBOX_TEST: [&'static str; 5] = [
    "happy",
    "😃",
    "安",
    "peaceful",
    "...something else!",
];

pub(crate) fn spawn_test() {
    // spawn two threads that compete for modal resources, to test the interlocking mechanisms
    thread::spawn({
        move || {
            let xns = XousNames::new().unwrap();
            let modals = modals::Modals::new(&xns).unwrap();
            let tt = ticktimer_server::Ticktimer::new().unwrap();

            // test progress bar
            modals.start_progress("Progress Quest", 100, 500, 100).expect("couldn't raise progress bar");
            for i in (100..500).step_by(8) {
                modals.update_progress(i).expect("couldn't update progress bar");
                tt.sleep_ms(100).unwrap();
            }
            modals.finish_progress().expect("couldn't dismiss progress bar");

            // test check box
            for item in CHECKBOX_TEST {
                modals.add_list_item(item).expect("couldn't build checkbox list");
            }
            match modals.get_checkbox("You can have it all:") {
                Ok(things) => {
                    log::info!("The user picked {} things:", things.len());
                    for thing in things {
                        log::info!("{}", thing);
                    }
                },
                _ => log::error!("get_checkbox failed"),
            }

            // test notificatons
            log::info!("testing notification");
            modals.show_notification("This is a test!").expect("notification failed");
            log::info!("notification test done");
        }
    });

    thread::spawn({
        move || {
            let xns = XousNames::new().unwrap();
            let modals = modals::Modals::new(&xns).unwrap();

            // test radio box
            for item in RADIO_TEST {
                modals.add_list_item(item).expect("couldn't build radio item list");
            }
            match modals.get_radiobutton("Pick an animal") {
                Ok(animal) => log::info!("{} was picked", animal),
                _ => log::error!("get_radiobutton failed"),
            }

            // test the modal dialog box function
            log::info!("test text input");
            match modals.get_text("Test input", Some(test_validator), None) {
                Ok(text) => {
                    log::info!("Input: {}", text.0);
                }
                _ => {
                    log::error!("get_text failed");
                }
            }
            log::info!("text input test done");

            // test notificatons
            log::info!("testing notification");
            modals.show_notification("这是一个测验!").expect("notification failed");
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
