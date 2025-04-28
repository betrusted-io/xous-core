use std::thread;

#[cfg(feature = "ditherpunk")]
use bitmap::PixelType;
#[cfg(not(any(feature = "hosted-baosec", feature = "cramium-soc")))]
use gam::*;
#[cfg(any(feature = "hosted-baosec", feature = "cramium-soc"))]
use ux_api::widgets::*;
use xous_names::XousNames;

use super::*;

const RADIO_TEST: [&'static str; 4] = ["zebra", "cow", "horse", "cat"];

const CHECKBOX_TEST: [&'static str; 5] =
    ["happy", "😃", "安", "peace &\n tranquility", "Once apon a time, in a land far far away, there was a"];

/// This is an integration test of the Modals crate. It creates two competing threads
/// that both try to throw up dialog boxes at the same time. Normally you *don't* want
/// to do that, but we should still handle that case gracefully since it does happen
/// sometimes.
///
/// Each thread will create a series of Modal primitives, including progess bars, notifications,
/// check boxes and radio boxes.
pub fn spawn_test() {
    // spawn two threads that compete for modal resources, to test the interlocking mechanisms

    thread::spawn({
        move || {
            let xns = XousNames::new().unwrap();
            let modals = Modals::new(&xns).unwrap();
            let tt = ticktimer_server::Ticktimer::new().unwrap();

            // 0. multi-modal test
            log::info!(
                "modal data: {:#?}",
                modals
                    .alert_builder("Four items.")
                    .field(Some("first".to_string()), None)
                    .field(Some("second".to_string()), None)
                    .field(None, None)
                    .field(Some("fourth".to_string()), None)
                    .build()
            );

            // 1. test progress bar
            // The start and end items are deliberately structured to be not zero-indexed; the use of PDDB_LOC
            // is just a convenient global constant.
            modals
                .start_progress(
                    "Progress Quest",
                    xous::PDDB_LOC,
                    xous::PDDB_LOC + 64 * 1024 * 128,
                    xous::PDDB_LOC,
                )
                .expect("couldn't raise progress bar");
            for i in (xous::PDDB_LOC..xous::PDDB_LOC + 64 * 1024 * 128).step_by(64 * 1024 * 16) {
                modals.update_progress(i).expect("couldn't update progress bar");
                tt.sleep_ms(100).unwrap();
            }
            modals.finish_progress().expect("couldn't dismiss progress bar");

            // 2. test check box
            let items: Vec<&str> = CHECKBOX_TEST.iter().map(|s| s.to_owned()).collect();
            modals.add_list(items).expect("couldn't build checkbox list");
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
            modals.show_notification("This is a test!", None).expect("notification failed");
            log::info!("notification test done");

            // 4. bip39 display test
            let refnum = 0b00000110001101100111100111001010000110110010100010110101110011111101101010011100000110000110101100110110011111100010011100011110u128;
            let refvec = refnum.to_be_bytes().to_vec();
            modals.show_bip39(Some("Some bip39 words"), &refvec).expect("couldn't show bip39 words");

            // 5. bip39 input test
            log::info!(
                "type these words: alert record income curve mercy tree heavy loan hen recycle mean devote"
            );
            match modals.input_bip39(Some("Input BIP39 words")) {
                Ok(data) => {
                    log::info!("got bip39 input: {:x?}", data);
                    log::info!("reference: 0x063679ca1b28b5cfda9c186b367e271e");
                }
                Err(e) => log::error!("couldn't get input: {:?}", e),
            }

            // 6. human interaction-enabled slider
            log::info!("testing human interaction-enabled slider");
            let result = modals
                .slider("Human interaction-enabled slider!", 0, 100, 50, 1)
                .expect("slider test failed");

            modals
                .show_notification(&format!("Slider value: {}", result), None)
                .expect("cannot show slider result notification");

            log::info!("slider test done");
        }
    });

    thread::spawn({
        move || {
            let xns = XousNames::new().unwrap();
            let modals = Modals::new(&xns).unwrap();

            // 1. test radio box
            for item in RADIO_TEST {
                modals.add_list_item(item).expect("couldn't build radio item list");
            }
            match modals.get_radiobutton("Pick an animal") {
                Ok(animal) => log::info!("{} was picked", animal),
                _ => log::error!("get_radiobutton failed"),
            }
            log::info!("Radio index selected = {:?}", modals.get_radio_index().unwrap());

            // 2. test the modal dialog box function
            log::info!("test text input");
            match modals
                .alert_builder("Enter a number")
                .field(Some("-0.0".to_string()), Some(test_validator))
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
            modals.show_notification("这是一个测验!", Some("这是一个测验!")).expect("notification failed");
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

            // 5. test image - because it reads a local file, only makes sense on hosted mode
            #[cfg(feature = "ditherpunk")]
            {
                const BORDER: u32 = 3;
                let modal_size = gam::Point::new(
                    (gam::IMG_MODAL_WIDTH - 2 * BORDER).try_into().unwrap(),
                    (gam::IMG_MODAL_HEIGHT - 2 * BORDER).try_into().unwrap(),
                );
                let bm = gam::Bitmap::from_img(&clifford(), Some(modal_size));
                log::info!("showing image");
                modals.show_image(bm).expect("show image modal failed");
                log::info!("image modal test done");
            }

            // 6. test that human-interactable slider modal
            log::info!("testing human interaction-enabled modal");
        }
    });
}

// https://sequelaencollection.home.blog/2d-chaotic-attractors/
#[cfg(feature = "ditherpunk")]
fn clifford() -> Img {
    // width & height chosen to force resize & rotation
    const WIDTH: u32 = gam::IMG_MODAL_HEIGHT + 2;
    const HEIGHT: u32 = gam::IMG_MODAL_WIDTH + 2;
    const X_CENTER: f32 = (WIDTH / 2) as f32;
    const Y_CENTER: f32 = (HEIGHT / 2) as f32;
    const SCALE: f32 = WIDTH as f32 / 5.1;
    const STEP: u8 = 16;
    const ITERATIONS: u32 = 200000;
    let mut buf = vec![255u8; (WIDTH * HEIGHT).try_into().unwrap()];
    let (a, b, c, d) = (-2.0, -2.4, 1.1, -0.9);
    let (mut x, mut y): (f32, f32) = (0.0, 0.0);

    log::info!("generating image");
    for _ in 0..=ITERATIONS {
        // this takes a couple minutes to run
        let x1 = f32::sin(a * y) + c * f32::cos(a * x);
        let y1 = f32::sin(b * x) + d * f32::cos(b * y);
        (x, y) = (x1, y1);
        let (a, b): (u32, u32) = ((x * SCALE + X_CENTER) as u32, (y * SCALE + Y_CENTER) as u32);
        let i: usize = (a + WIDTH * b).try_into().unwrap();
        if buf[i] >= STEP {
            buf[i] -= STEP;
        }
    }
    log::info!("done: {:x?}", &buf[..32]);
    Img::new(buf, WIDTH.try_into().unwrap(), PixelType::U8)
}

fn test_validator(input: &TextEntryPayload) -> Option<String> {
    let text_str = input.as_str();
    match text_str.parse::<f32>() {
        Ok(_input_int) => None,
        _ => return Some(String::from("enter a number")),
    }
}
