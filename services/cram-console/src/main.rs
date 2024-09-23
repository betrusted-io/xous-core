#[cfg(feature = "hwtest")]
mod hwtest;

mod app_autogen;
mod appmenu;
mod ball;
mod cmds;
mod mainmenu;
mod repl;
mod shell;

use std::collections::HashMap;
use std::fmt::Write;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;

use cmds::*;
use cram_hal_service::keyboard;
use graphics_server::Gid;
use graphics_server::api::GlyphStyle;
use graphics_server::*;
use locales::t;
use num_traits::*;
use xous::{CID, Message, msg_scalar_unpack, send_message};

const SERVER_NAME_STATUS_GID: &str = "_Status bar GID receiver_";
const SERVER_NAME_STATUS: &str = "_Status_";

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum StatusOpcode {
    /// indicates time for periodic update of the status bar
    Pump,

    /// Raise the PDDB menu
    SubmenuPddb,
    /// Raise the App menu
    SubmenuApp,

    /// Tells keyboard watching thread that a new keypress happened.
    Keypress,

    /// Raise the Shellchat app
    SwitchToShellchat,
    /// Switch to an app
    SwitchToApp,

    Quit,
}

static mut CB_TO_MAIN_CONN: Option<CID> = None;

pub fn pump_thread(conn: usize, pump_run: Arc<AtomicBool>) {
    let ticktimer = ticktimer::Ticktimer::new().unwrap();
    loop {
        if pump_run.load(Ordering::Relaxed) {
            match send_message(
                conn as u32,
                Message::new_scalar(StatusOpcode::Pump.to_usize().unwrap(), 0, 0, 0, 0),
            ) {
                Err(xous::Error::ServerNotFound) => break,
                Ok(xous::Result::Ok) => {}
                _ => panic!("unhandled error in status pump thread"),
            }
        }
        ticktimer.sleep_ms(1000).unwrap();
    }
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
        let xns = xous_names::XousNames::new().unwrap();
        let mut ball = ball::Ball::new(&xns);
        log::info!("starting ball");
        loop {
            ball.update();
        }
    });

    let tt = ticktimer::Ticktimer::new().unwrap();
    let xns = xous_names::XousNames::new().unwrap();

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

    // --------------------------- graphical loop timing
    let mut stats_phase: usize = 0;
    let mut secnotes_force_redraw = false;
    let secnotes_interval = 4;

    let gam = gam::Gam::new(&xns).expect("|status: can't connect to GAM");
    // screensize is controlled by the GAM, it's set in main.rs near the top
    let screensize = gam.get_canvas_bounds(status_gid).expect("|status: Couldn't get canvas size");

    // ------------------ render initial graphical display, so we don't seem broken on boot
    const CPU_BAR_WIDTH: i16 = 46;
    const CPU_BAR_OFFSET: i16 = 8;
    let time_rect = Rectangle::new_with_style(
        Point::new(0, 0),
        Point::new(screensize.x / 2 - CPU_BAR_WIDTH / 2 - 1 + CPU_BAR_OFFSET, screensize.y / 2 - 1),
        DrawStyle::new(PixelColor::Light, PixelColor::Light, 0),
    );
    // build uptime text view: left half of status bar
    let mut uptime_tv =
        TextView::new(status_gid, TextBounds::GrowableFromTl(time_rect.tl(), time_rect.width() as _));
    uptime_tv.untrusted = false;
    uptime_tv.style = GlyphStyle::Tall;
    uptime_tv.draw_border = false;
    uptime_tv.margin = Point::new(3, 0);
    write!(uptime_tv, "{}", t!("secnote.startup", locales::LANG))
        .expect("|status: couldn't init uptime text");
    gam.post_textview(&mut uptime_tv).expect("|status: can't draw battery stats");

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

    // ------------------------ measure current security state and adjust messaging
    let sec_notes = Arc::new(Mutex::new(HashMap::<String, String>::new()));
    let mut last_sec_note_index = 0;
    let mut last_sec_note_size = 0;

    // ---------------------------- build menus
    // used to hide time when the PDDB is not mounted
    #[cfg(feature = "pddb")]
    let pddb_poller = pddb::PddbMountPoller::new();
    #[cfg(not(feature = "pddb"))]
    gam.allow_mainmenu().ok();

    // these menus stake a claim on some security-sensitive connections; occupy them upstream of trying to do
    // an update
    log::debug!("starting main menu thread");
    let main_menu_sid = xous::create_server().unwrap();
    let status_cid = xous::connect(status_sid).unwrap();
    let _menu_manager = mainmenu::create_main_menu(main_menu_sid, status_cid);
    appmenu::create_app_menu(xous::connect(status_sid).unwrap());
    let kbd = Arc::new(Mutex::new(keyboard::Keyboard::new(&xns).unwrap()));

    // ---------------------------- Background processes that claim contexts
    // must be upstream of the update check, because we need to occupy the keyboard
    // server slot to prevent e.g. a keyboard logger from taking our passwords!
    kbd.lock()
        .unwrap()
        .register_observer(SERVER_NAME_STATUS, StatusOpcode::Keypress.to_u32().unwrap() as usize);

    let _modals = modals::Modals::new(&xns).unwrap();

    shell::start_shell();

    let pump_run = Arc::new(AtomicBool::new(false));
    let pump_conn = xous::connect(status_sid).unwrap();
    let _ = thread::spawn({
        let pump_run = pump_run.clone();
        move || {
            pump_thread(pump_conn as _, pump_run);
        }
    });

    pump_run.store(true, Ordering::Relaxed); // start status thread updating
    loop {
        let msg = xous::receive_message(status_sid).unwrap();
        let opcode: Option<StatusOpcode> = FromPrimitive::from_usize(msg.body.id());
        log::debug!("{:?}", opcode);
        match opcode {
            Some(StatusOpcode::Pump) => {
                let elapsed_time = tt.elapsed_ms();
                {
                    // update the time field
                    // have to clear the entire rectangle area, because the text has a variable width and
                    // dirty text will remain if the text is shortened
                    gam.draw_rectangle(status_gid, time_rect).ok();
                    uptime_tv.clear_str();

                    write!(
                        &mut uptime_tv,
                        " {}{}:{:02}:{:02}",
                        t!("stats.uptime", locales::LANG),
                        (elapsed_time / 3_600_000),
                        (elapsed_time / 60_000) % 60,
                        (elapsed_time / 1000) % 60,
                    )
                    .expect("|status: can't write string");
                    gam.post_textview(&mut uptime_tv).expect("|status: can't draw uptime");
                    if let Some(bounds) = uptime_tv.bounds_computed {
                        if bounds.height() as i16 > screensize.y / 2 + 1 {
                            // the clipping rectangle limits the bounds to the overall height of the status
                            // area, so the overlap between status and secnotes
                            // must be managed within this server
                            log::info!(
                                "Status text overstepped its intended bound. Forcing secnotes redraw."
                            );
                        }
                    }
                }

                // update the security status, if any
                if secnotes_force_redraw
                    || sec_notes.lock().unwrap().len() != last_sec_note_size
                    || ((stats_phase % secnotes_interval) == 0)
                {
                    log::debug!("updating lock state text");
                    if sec_notes.lock().unwrap().len() != last_sec_note_size {
                        last_sec_note_size = sec_notes.lock().unwrap().len();
                        if last_sec_note_size > 0 {
                            last_sec_note_index = last_sec_note_size - 1;
                        }
                    }

                    security_tv.clear_str();
                    if last_sec_note_size > 0 {
                        for (index, v) in sec_notes.lock().unwrap().values().enumerate() {
                            if index == last_sec_note_index {
                                write!(&mut security_tv, "{}", v.as_str()).unwrap();
                                last_sec_note_index = (last_sec_note_index + 1) % last_sec_note_size;
                                break;
                            }
                        }
                    } else {
                        write!(&mut security_tv, "{}", t!("secnote.allclear", locales::LANG)).unwrap();
                    }

                    secnotes_force_redraw = false;
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
                }

                log::trace!("status redraw## update");
                gam.redraw().expect("|status: couldn't redraw");

                stats_phase = stats_phase.wrapping_add(1);
            }
            Some(StatusOpcode::SubmenuPddb) => {
                tt.sleep_ms(100).ok(); // yield for a moment to allow the previous menu to close
                gam.raise_menu(gam::PDDB_MENU_NAME).expect("couldn't raise PDDB submenu");
            }
            Some(StatusOpcode::SubmenuApp) => {
                tt.sleep_ms(100).ok(); // yield for a moment to allow the previous menu to close
                gam.raise_menu(gam::APP_MENU_NAME).expect("couldn't raise App submenu");
            }
            Some(StatusOpcode::SwitchToShellchat) => {
                tt.sleep_ms(100).ok();
                sec_notes.lock().unwrap().remove(&"current_app".to_string());
                sec_notes
                    .lock()
                    .unwrap()
                    .insert("current_app".to_string(), format!("Running: Shellchat").to_string());
                gam.switch_to_app(gam::APP_NAME_SHELLCHAT, security_tv.token.unwrap())
                    .expect("couldn't raise shellchat");
                secnotes_force_redraw = true;
                send_message(cb_cid, Message::new_scalar(StatusOpcode::Pump.to_usize().unwrap(), 0, 0, 0, 0))
                    .expect("couldn't trigger status update");
            }
            Some(StatusOpcode::SwitchToApp) => msg_scalar_unpack!(msg, index, _, _, _, {
                tt.sleep_ms(100).ok();
                let app_name = app_autogen::app_index_to_name(index).expect("app index not found");
                app_autogen::app_dispatch(&gam, security_tv.token.unwrap(), index)
                    .expect("cannot switch to app");
                sec_notes.lock().unwrap().remove(&"current_app".to_string());
                sec_notes
                    .lock()
                    .unwrap()
                    .insert("current_app".to_string(), format!("Running: {}", app_name).to_string());
                secnotes_force_redraw = true;
                send_message(cb_cid, Message::new_scalar(StatusOpcode::Pump.to_usize().unwrap(), 0, 0, 0, 0))
                    .expect("couldn't trigger status update");
            }),
            Some(StatusOpcode::Keypress) => {
                // placeholder
            }

            Some(StatusOpcode::Quit) => {
                xous::return_scalar(msg.sender, 1).ok();
                break;
            }
            None => {
                log::error!("|status: received unknown Opcode");
            }
        }
    }
    log::trace!("status thread exit, destroying servers");
    unsafe {
        if let Some(cb) = CB_TO_MAIN_CONN {
            xous::disconnect(cb).unwrap();
        }
    }
    unsafe {
        xous::disconnect(pump_conn).unwrap();
    }
    xns.unregister_server(status_sid).unwrap();
    xous::destroy_server(status_sid).unwrap();
    log::trace!("status thread quitting");
    xous::terminate_process(0)
}
