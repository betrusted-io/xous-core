#[cfg(feature = "hwtest")]
mod hwtest;

mod ball;

use std::fmt::Write;
use std::thread;

use cram_hal_service::trng;
use graphics_server::api::GlyphStyle;
use graphics_server::Gid;
use graphics_server::*;
use locales::t;
use num_traits::*;

const SERVER_NAME_STATUS_GID: &str = "_Status bar GID receiver_";
const SERVER_NAME_STATUS: &str = "_Status_";

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum StatusOpcode {
    Quit,
}

fn main() {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);

    #[cfg(feature = "hwtest")]
    hwtest::hwtest();

    #[cfg(feature = "early-ball")]
    thread::spawn(move || {
        let mut count = 0;
        loop {
            log::info!("Still alive! #{}", count);
            count += 1;
            std::thread::sleep(std::time::Duration::from_millis(5000));
        }
    });

    #[cfg(feature = "early-ball")]
    thread::spawn(move || {
        let xns = xous_api_names::XousNames::new().unwrap();
        let mut ball = ball::Ball::new(&xns);
        log::info!("starting ball");
        loop {
            ball.update();
        }
    });

    let tt = xous_api_ticktimer::Ticktimer::new().unwrap();
    let xns = xous_api_names::XousNames::new().unwrap();

    let usb = usb_device_xous::UsbHid::new();

    let status_gam_getter =
        xns.register_name(SERVER_NAME_STATUS_GID, Some(1)).expect("can't register server");
    let mut canvas_gid: [u32; 4] = [0; 4];
    // wait until we're assigned a GID -- this is a one-time message from the GAM
    let msg = xous::receive_message(status_gam_getter).unwrap();
    log::trace!("GID assignment message: {:?}", msg);
    xous::msg_scalar_unpack!(msg, g0, g1, g2, g3, {
        canvas_gid[0] = g0 as u32;
        canvas_gid[1] = g1 as u32;
        canvas_gid[2] = g2 as u32;
        canvas_gid[3] = g3 as u32;
    });
    match xns.unregister_server(status_gam_getter) {
        Err(e) => {
            log::error!("couldn't unregister getter server: {:?}", e);
        }
        _ => {}
    }
    xous::destroy_server(status_gam_getter).unwrap();

    let status_gid: Gid = Gid::new(canvas_gid);
    // Expected connections:
    //   - from keyboard
    //   - from USB HID
    let status_sid = xns.register_name(SERVER_NAME_STATUS, Some(2)).unwrap();
    // create a connection for callback hooks
    let cb_cid = xous::connect(status_sid).unwrap();

    let gam = gam::Gam::new(&xns).expect("|status: can't connect to GAM");
    // screensize is controlled by the GAM, it's set in main.rs near the top
    let screensize = gam.get_canvas_bounds(status_gid).expect("|status: Couldn't get canvas size");

    // build security status textview
    let mut security_tv = TextView::new(
        status_gid,
        TextBounds::BoundingBox(Rectangle::new(
            Point::new(0, screensize.y / 2 + 1),
            Point::new(screensize.x, screensize.y),
        )),
    );

    security_tv.style = GlyphStyle::Tall; // was: Regular, but not available on this target
    security_tv.draw_border = false;
    security_tv.margin = Point::new(0, 0);
    security_tv.token = gam.claim_token(gam::STATUS_BAR_NAME).expect("couldn't request token"); // this is a shared magic word to identify this process
    security_tv.clear_area = true;
    security_tv.invert = true;
    write!(&mut security_tv, "{}", t!("secnote.startup", locales::LANG)).unwrap();
    gam.post_textview(&mut security_tv).unwrap();
    gam.draw_line(
        status_gid,
        Line::new_with_style(
            Point::new(0, screensize.y),
            screensize,
            DrawStyle::new(PixelColor::Light, PixelColor::Light, 1),
        ),
    )
    .unwrap();
    gam.redraw().unwrap(); // initial boot redraw

    // ------------------ start a 'gutter' thread to handle incoming events while we go through the
    // boot/autoupdate process
    let gutter = thread::spawn({
        let gutter_sid = status_sid.clone();
        move || {
            loop {
                let msg = xous::receive_message(gutter_sid).unwrap();
                let opcode: Option<StatusOpcode> = FromPrimitive::from_usize(msg.body.id());
                log::info!("Guttering {:?}", opcode);
                match opcode {
                    Some(StatusOpcode::Quit) => {
                        xous::return_scalar(msg.sender, 1).ok();
                        break;
                    }
                    _ => (), // ignore everything else.
                }
            }
        }
    });

    let modals = modals::Modals::new(&xns).unwrap();
    xous::send_message(
        cb_cid,
        xous::Message::new_blocking_scalar(StatusOpcode::Quit.to_usize().unwrap(), 0, 0, 0, 0),
    )
    .expect("couldn't exit the gutter server");
    gutter.join().expect("status boot gutter server did not exit gracefully");

    let mut ball = ball::Ball::new(&xns);
    log::info!("starting ball");
    loop {
        ball.update();
    }
}
