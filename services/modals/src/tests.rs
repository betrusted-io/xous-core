#![allow(dead_code)]
use gam::*;
use std::thread;

use xous_names::XousNames;

const RADIO_TEST: [&'static str; 4] = ["zebra", "cow", "horse", "cat"];

const CHECKBOX_TEST: [&'static str; 5] = ["happy", "😃", "安", "peaceful", "...something else!"];

/// This is an integration test of the Modals crate. It creates two competing threads
/// that both try to throw up dialog boxes at the same time. Normally you *don't* want
/// to do that, but we should still handle that case gracefully since it does happen
/// sometimes.
///
/// Each thread will create a series of Modal primitives, including progess bars, notifications,
/// check boxes and radio boxes.
pub(crate) fn spawn_test() {
    // spawn two threads that compete for modal resources, to test the interlocking mechanisms
    thread::spawn({
        move || {
            let xns = XousNames::new().unwrap();
            let modals = modals::Modals::new(&xns).unwrap();
            let tt = ticktimer_server::Ticktimer::new().unwrap();

            // 0. multi-modal test
            log::info!(
                "modal data: {:#?}",
                modals
                    .alert_builder("Four items with maybe defaults. Press select to close.")
                    .field(Some("first".to_string()), None)
                    .field(Some("second".to_string()), None)
                    .field(None, None)
                    .field(Some("fourth".to_string()), None)
                    .build()
            );

            // 1. test progress bar
            // The start and end items are deliberately structured to be not zero-indexed; the use of PDDB_LOC is just a
            // convenient global constant.
            modals
                .start_progress(
                    "Progress Quest",
                    xous::PDDB_LOC,
                    xous::PDDB_LOC + 64 * 1024 * 128,
                    xous::PDDB_LOC,
                )
                .expect("couldn't raise progress bar");
            for i in (xous::PDDB_LOC..xous::PDDB_LOC + 64 * 1024 * 128).step_by(64 * 1024 * 16) {
                modals
                    .update_progress(i)
                    .expect("couldn't update progress bar");
                tt.sleep_ms(100).unwrap();
            }
            modals
                .finish_progress()
                .expect("couldn't dismiss progress bar");

            // 2. test check box
            let items: Vec<&str> = CHECKBOX_TEST.iter().map(|s| s.to_owned()).collect();
            modals
                .add_list(items)
                .expect("couldn't build checkbox list");
            match modals.get_checkbox("You can have it all:") {
                Ok(things) => {
                    log::info!("The user picked {} things:", things.len());
                    for thing in things {
                        log::info!("{}", thing);
                    }
                }
                _ => log::error!("get_checkbox failed"),
            }
            log::info!("Checkbox indices selected = {:?}", modals.get_check_index());

            // 3. test notificatons
            log::info!("testing notification");
            modals
                .show_notification("This is a test!", None)
                .expect("notification failed");
            log::info!("notification test done");
        }
    });

    thread::spawn({
        move || {
            let xns = XousNames::new().unwrap();
            let modals = modals::Modals::new(&xns).unwrap();

            // 1. test radio box
            for item in RADIO_TEST {
                modals
                    .add_list_item(item)
                    .expect("couldn't build radio item list");
            }
            match modals.get_radiobutton("Pick an animal") {
                Ok(animal) => log::info!("{} was picked", animal),
                _ => log::error!("get_radiobutton failed"),
            }
            log::info!(
                "Radio index selected = {:?}",
                modals.get_radio_index().unwrap()
            );

            // 2. test the modal dialog box function
            log::info!("test text input");
            match modals
                .alert_builder("Test input")
                .field(None, Some(test_validator))
                .build()
            {
                Ok(text) => {
                    log::info!("Input: {}", text.content()[0].content);
                }
                _ => {
                    log::error!("get_text failed");
                }
            }
            log::info!("text input test done");

            // 3. test notificatons
            log::info!("testing notification");
            modals
                .show_notification("这是一个测验!", Some("这是一个测验!"))
                .expect("notification failed");
            log::info!("notification test done");

            // 4. test qrcode
            log::info!("testing qrcode");
            modals
                .show_notification(
                    "Please contribute to xous-core",
                    Some("https://github.com/betrusted-io/xous-core"),
                )
                .expect("qrcode failed");
            log::info!("qrcode test done");
        }
    });
}

fn test_validator(input: TextEntryPayload) -> Option<xous_ipc::String<256>> {
    let text_str = input.as_str();
    match text_str.parse::<u32>() {
        Ok(_input_int) => None,
        _ => return Some(xous_ipc::String::<256>::from_str("enter an integer value")),
    }
}
