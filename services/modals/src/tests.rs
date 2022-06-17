use super::*;
use gam::*;
use std::thread;
use xous_names::XousNames;
#[cfg(feature = "ditherpunk")]
use bitmap::PixelType;

const RADIO_TEST: [&'static str; 4] = ["zebra", "cow", "horse", "cat"];

const CHECKBOX_TEST: [&'static str; 5] = ["happy", "😃", "安", "peaceful", "...something else!"];

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
            let modals = Modals::new(&xns).unwrap();

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

            // 5. test image - because it reads a local file, only makes sense on hosted mode
            /*
             Note there are stack allocation challenges with crates `png_encode` and `jpg`

             png = {version = "0.17.5", optional = true}
             -------------------------------------------
             let decoder = png::Decoder::new(file);
             let mut reader = decoder.read_info().expect("failed to read png info");
             let mut buf = vec![0; reader.output_buffer_size()];
             let info = reader.next_frame(&mut buf).expect("failed to decode png");
             let width: usize = info.width.try_into().unwrap();
             let img = Img::new(buf, width);

             jpeg-decoder = {version = "0.2.6", optional = true}
             ---------------------------------------------------
             let mut decoder = jpeg_decoder::Decoder::new(file);
             decoder
                 .scale(Modals::MODAL_WIDTH as u16, Modals::MODAL_HEIGHT as u16)
                 .expect("failed to scale jpeg");
             let _reader = decoder.read_info().expect("failed to read png info");
             let pixels = decoder.decode().expect("failed to decode jpeg image");
             let info = decoder.info().unwrap();
             let width: usize = info.width.try_into().unwrap();
             let img = Img::new(pixels, width);
            */
            #[cfg(feature = "ditherpunk")]
            {
                log::info!("testing image");
                let img = clifford();
                modals.show_image(&img).expect("show image modal failed");
                log::info!("image modal test done");
            }
        }
    });
}

// https://sequelaencollection.home.blog/2d-chaotic-attractors/
#[cfg(feature = "ditherpunk")]
fn clifford() -> Img {
    const SIZE: u32 = Modals::MODAL_WIDTH - 10;
    const CENTER: f32 = (SIZE / 2) as f32;
    const SCALE: f32 = 60.0;
    let mut buf = vec![255u8; (SIZE * SIZE).try_into().unwrap()];
    let (a, b, c, d) = (-2.0, -2.4, 1.1, -0.9);
    let (mut x, mut y): (f32, f32) = (0.0, 0.0);
    for _ in 0..=4000000 {
        let x1 = f32::sin(a * y) + c * f32::cos(a * x);
        let y1 = f32::sin(b * x) + d * f32::cos(b * y);
        (x, y) = (x1, y1);
        let (a, b): (u32, u32) = ((x * SCALE + CENTER) as u32, (y * SCALE + CENTER) as u32);
        let i: usize = (a + SIZE * b).try_into().unwrap();
        if buf[i] > 0 {
            buf[i] -= 1;
        }
    }
    let mut rgb = vec![0u8; 3 * buf.len()];
    let mut i: usize = 0;
    while i < buf.len() {
        let j = 3 * i;
        (rgb[j], rgb[j + 1], rgb[j + 2]) = (buf[i], buf[i], buf[i]);
        i += 1;
    }
    Img::new(rgb, SIZE.try_into().unwrap(), PixelType::U8x3)
}

fn test_validator(input: TextEntryPayload) -> Option<xous_ipc::String<256>> {
    let text_str = input.as_str();
    match text_str.parse::<u32>() {
        Ok(_input_int) => None,
        _ => return Some(xous_ipc::String::<256>::from_str("enter an integer value")),
    }
}
