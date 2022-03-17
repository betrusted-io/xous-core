#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod mainmenu;
use mainmenu::*;
mod appmenu;
use appmenu::*;
mod kbdmenu;
use kbdmenu::*;
mod app_autogen;

use com::api::*;
use core::fmt::Write;
use num_traits::*;
use xous::{msg_scalar_unpack, send_message, Message, CID};
use graphics_server::*;
use graphics_server::api::GlyphStyle;
use locales::t;
use gam::modal::*;
use gam::{GamObjectList, GamObjectType};
use llio::Weekday;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg_attr(
    not(any(target_os = "none", target_os = "xous")),
    allow(unused_imports)
)]
use std::thread;

const SERVER_NAME_STATUS: &str = "_Status bar manager_";
const SERVER_NAME_STATUS_GID: &str = "_Status bar GID receiver_";

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum StatusOpcode {
    /// for passing battstats on to the main thread from the callback
    BattStats,
    /// for passing DateTime
    DateTime,
    /// indicates time for periodic update of the status bar
    Pump,
    /// Pulls up the time setting UI
    UxSetTime,
    /// Initiates a reboot
    Reboot,

    /// Raise the PDDB menu
    SubmenuPddb,
    /// Raise the App menu
    SubmenuApp,
    /// Raise the Keyboard layout menu
    SubmenuKbd,

    /// Raise the Shellchat app
    SwitchToShellchat,
    /// Switch to an app
    SwitchToApp,

    /// Set the keyboard map
    SetKeyboard,

    /// Suspend handler from the main menu
    TrySuspend,
    /// Ship mode handler for the main menu
    BatteryDisconnect,
    /// for returning wifi stats
    WifiStats,
    Quit,
}

static mut CB_TO_MAIN_CONN: Option<CID> = None;
fn battstats_cb(stats: BattStats) {
    if let Some(cb_to_main_conn) = unsafe { CB_TO_MAIN_CONN } {
        let rawstats: [usize; 2] = stats.into();
        send_message(
            cb_to_main_conn,
            xous::Message::new_scalar(
                StatusOpcode::BattStats.to_usize().unwrap(),
                rawstats[0],
                rawstats[1],
                0,
                0,
            ),
        )
        .unwrap();
    }
}

pub fn dt_callback(dt: llio::DateTime) {
    //log::info!("dt_callback received with {:?}", dt);
    if let Some(cb_to_main_conn) = unsafe { CB_TO_MAIN_CONN } {
        let buf = xous_ipc::Buffer::into_buf(dt)
            .or(Err(xous::Error::InternalError))
            .unwrap();
        buf.send(cb_to_main_conn, StatusOpcode::DateTime.to_u32().unwrap())
            .unwrap();
    }
}

pub fn pump_thread(conn: usize, pump_run: Arc<AtomicBool>) {
    let ticktimer = ticktimer_server::Ticktimer::new().unwrap();
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
#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // 1 connection exactly -- from the GAM to set our canvas GID
    let status_gam_getter = xns
        .register_name(SERVER_NAME_STATUS_GID, Some(1))
        .expect("can't register server");
    let mut canvas_gid: [u32; 4] = [0; 4];
    // wait unil we're assigned a GID -- this is a one-time message from the GAM
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

    // ok, now that we have a GID, we can continue on with our merry way
    let status_gid: Gid = Gid::new(canvas_gid);
    log::trace!("|status: my canvas {:?}", status_gid);

    log::debug!("|status: registering GAM|status thread");
    // we have one connection, from the status main loop; but we make it with a local call, not using xns. so there's 0 in xns.
    let status_sid = xns
        .register_name(SERVER_NAME_STATUS, Some(0))
        .expect("|status: can't register server");
    // create a connection for callback hooks
    let cb_cid = xous::connect(status_sid).unwrap();
    unsafe { CB_TO_MAIN_CONN = Some(cb_cid) };
    let pump_run = Arc::new(AtomicBool::new(true));
    let pump_conn = xous::connect(status_sid).unwrap();
    let _ = thread::spawn({
        let pump_run = pump_run.clone();
        move || {
            pump_thread(pump_conn as _, pump_run);
        }
    });

    let gam = gam::Gam::new(&xns).expect("|status: can't connect to GAM");
    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");
    let susres = susres::Susres::new_without_hook(&xns).unwrap();
    let mut netmgr = net::NetManager::new();

    // screensize is controlled by the GAM, it's set in main.rs near the top
    let screensize = gam
        .get_canvas_bounds(status_gid)
        .expect("|status: Couldn't get canvas size");
    // layout: 336 px wide
    // 0                   150 150 200
    // Feb 05 15:00 (00:06:23) xxxx     3.72V/-100mA/99%
    const CPU_BAR_WIDTH: i16 = 50;
    let time_rect = Rectangle::new_with_style(
        Point::new(0, 0),
        Point::new(screensize.x / 2 - CPU_BAR_WIDTH / 2 - 1, screensize.y / 2 - 1),
        DrawStyle::new(PixelColor::Light, PixelColor::Light, 0)
    );
    let cpuload_rect = Rectangle::new_with_style(
        Point::new(screensize.x / 2 - CPU_BAR_WIDTH / 2, 0),
        Point::new(screensize.x / 2 + CPU_BAR_WIDTH / 2, screensize.y / 2 + 1),
        DrawStyle::new(PixelColor::Light, PixelColor::Dark, 1),
    );
    let stats_rect = Rectangle::new_with_style(
        Point::new(screensize.x / 2 + CPU_BAR_WIDTH / 2 + 1, 0),
        Point::new(screensize.x, screensize.y / 2 - 1),
        DrawStyle::new(PixelColor::Light, PixelColor::Light, 0),
    );

    log::debug!("|status: building textview objects");
    // build uptime text view: left half of status bar
    let mut uptime_tv = TextView::new(
        status_gid,
        TextBounds::GrowableFromTl(time_rect.tl(), time_rect.width() as _),
    );
    uptime_tv.untrusted = false;
    uptime_tv.style = GlyphStyle::Regular;
    uptime_tv.draw_border = false;
    uptime_tv.margin = Point::new(3, 0);
    write!(uptime_tv, "{}", t!("secnote.startup", xous::LANG)).expect("|status: couldn't init uptime text");
    gam.post_textview(&mut uptime_tv)
        .expect("|status: can't draw battery stats");
    log::debug!("|status: screensize as reported: {:?}", screensize);
    log::debug!("|status: uptime initialized to '{:?}'", uptime_tv);

    // build battstats text view: right half of status bar
    let mut battstats_tv = TextView::new(
        status_gid,
        TextBounds::GrowableFromTr(stats_rect.tr(), stats_rect.width() as _),
    );
    battstats_tv.style = GlyphStyle::Regular;
    battstats_tv.draw_border = false;
    battstats_tv.margin = Point::new(0, 0);
    gam.post_textview(&mut battstats_tv)
        .expect("|status: can't draw battery stats");

    // initialize to some "sane" mid-point defaults, so we don't trigger errors later on before the first real battstat reading comes
    let mut stats = BattStats {
        voltage: 3700,
        soc: 50,
        current: 0,
        remaining_capacity: 650,
    };

    log::debug!("initializing RTC...");
    let mut rtc = llio::Rtc::new(&xns);

    #[cfg(any(target_os = "none", target_os = "xous"))]
    rtc.clear_wakeup_alarm().unwrap(); // clear any wakeup alarm state, if it was set

    rtc.hook_rtc_callback(dt_callback).unwrap();
    let mut datetime: Option<llio::DateTime> = None;
    let llio = llio::Llio::new(&xns);

    log::debug!("usb unlock notice...");
    let (dl, _) = llio.debug_usb(None).unwrap();
    let mut debug_locked = dl;
    // build security status textview
    let mut security_tv = TextView::new(
        status_gid,
        TextBounds::BoundingBox(Rectangle::new(
            Point::new(0, screensize.y / 2 + 1),
            Point::new(screensize.x, screensize.y),
        )),
    );
    security_tv.style = GlyphStyle::Regular;
    security_tv.draw_border = false;
    security_tv.margin = Point::new(0, 0);
    security_tv.token = gam.claim_token(gam::STATUS_BAR_NAME).expect("couldn't request token"); // this is a shared magic word to identify this process
    security_tv.clear_area = true;
    security_tv.invert = true;
    write!(&mut security_tv, "{}", t!("secnote.startup", xous::LANG)).unwrap();
    gam.post_textview(&mut security_tv).unwrap();
    gam.draw_line(status_gid, Line::new_with_style(
        Point::new(0, screensize.y), screensize,
        DrawStyle::new(PixelColor::Light, PixelColor::Light, 1))).unwrap();
    log::trace!("status redraw## initial");
    gam.redraw().unwrap(); // initial boot redraw

    let sec_notes = Arc::new(Mutex::new(HashMap::new()));
    let mut last_sec_note_index = 0;
    let mut last_sec_note_size = 0;
    if !debug_locked {
        sec_notes.lock().unwrap().insert(
            "secnote.usb_unlock".to_string(),
            t!("secnote.usb_unlock", xous::LANG).to_string(),
        );
    }
    let keys = Arc::new(Mutex::new(
        root_keys::RootKeys::new(&xns, None)
            .expect("couldn't connect to root_keys to query initialization state"),
    ));
    if !keys.lock().unwrap().is_initialized().unwrap() {
        sec_notes.lock().unwrap().insert(
            "secnotes.no_keys".to_string(),
            t!("secnote.no_keys", xous::LANG).to_string(),
        );
    } else {
        log::info!("checking gateware signature...");
        thread::spawn({
            let clone = Arc::clone(&sec_notes);
            let keys = Arc::clone(&keys);
            move || {
                if let Some(pass) = keys
                    .lock()
                    .unwrap()
                    .check_gateware_signature()
                    .expect("couldn't issue gateware check call")
                {
                    if !pass {
                        let mut sn = clone.lock().unwrap();
                        sn.insert(
                            "secnotes.gateware_fail".to_string(),
                            t!("secnote.gateware_fail", xous::LANG).to_string(),
                        );
                    }
                } else {
                    let mut sn = clone.lock().unwrap();
                    sn.insert(
                        "secnotes.state_fail".to_string(),
                        t!("secnote.state_fail", xous::LANG).to_string(),
                    );
                }
            }
        });
    };
    sec_notes.lock().unwrap().insert("current_app".to_string(), format!("Running: Shellchat").to_string()); // this is the default app on boot

    let mut stats_phase: usize = 0;

    let dt_pump_interval = 15;
    let charger_pump_interval = 180;
    let batt_interval;
    let secnotes_interval;
    if cfg!(feature = "braille") {
        // lower the status output rate for braille mode - it's invisible anyways, this is mainly for the debug configuration
        batt_interval = 60;
        secnotes_interval = 30;
    } else {
        batt_interval = 4;
        secnotes_interval = 4;
    }
    let mut battstats_phase = true;
    let mut secnotes_force_redraw = false;

    // the EC gets reset by the Net crate on boot to ensure that the state machines are synced up
    // this takes a few seconds, so we have a dead-wait here. This is a good spot for it because
    // the status bar reads "booting up..." during this period.
    log::debug!("syncing with COM");
    let mut com = com::Com::new(&xns).expect("|status: can't connect to COM");
    com.ping(0).unwrap(); // this will block until the COM is ready to take events
    com.hook_batt_stats(battstats_cb)
        .expect("|status: couldn't hook callback for events from COM");
    // prime the loop
    com.req_batt_stats()
        .expect("Can't get battery stats from COM");

    log::debug!("starting main menu thread");
    create_main_menu(keys.clone(), xous::connect(status_sid).unwrap(), &com);
    create_app_menu(xous::connect(status_sid).unwrap());
    let kbd_mgr = xous::create_server().unwrap();
    let kbd_menumatic = create_kbd_menu(xous::connect(status_sid).unwrap(), kbd_mgr);
    let kbd = keyboard::Keyboard::new(&xns).unwrap();

    // some RTC UX structures
    let modals = modals::Modals::new(&xns).unwrap();
    let day_of_week_list = [
        t!("rtc.monday", xous::LANG),
        t!("rtc.tuesday", xous::LANG),
        t!("rtc.wednesday", xous::LANG),
        t!("rtc.thursday", xous::LANG),
        t!("rtc.friday", xous::LANG),
        t!("rtc.saturday", xous::LANG),
        t!("rtc.sunday", xous::LANG),
    ];
    log::debug!("subscribe to wifi updates");
    netmgr.wifi_state_subscribe(cb_cid, StatusOpcode::WifiStats.to_u32().unwrap()).unwrap();
    let mut wifi_status: WlanStatus = WlanStatus::from_ipc(WlanStatusIpc::default());

    #[cfg(feature="tts")]
    thread::spawn({
        move || {
            // indicator of boot-up for blind users
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            tt.sleep_ms(1500).unwrap();
            let xns = xous_names::XousNames::new().unwrap();
            let llio = llio::Llio::new(&xns);
            llio.vibe(llio::VibePattern::Double).unwrap();
        }
    });
    log::info!("|status: starting main loop"); // don't change this -- factory test looks for this exact string
    loop {
        let msg = xous::receive_message(status_sid).unwrap();
        log::trace!("|status: Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(StatusOpcode::BattStats) => msg_scalar_unpack!(msg, lo, hi, _, _, {
                stats = [lo, hi].into();
                battstats_tv.clear_str();
                // have to clear the entire rectangle area, because the SSID has a variable width and can be much wider or shorter than battstats
                gam.draw_rectangle(status_gid, stats_rect).ok();

                // 0xdddd and 0xffff are what are returned when the EC is too busy to respond/hung, or in reset, respectively
                if stats.current == -8739 /* 0xdddd */
                || stats.voltage == 0xdddd || stats.voltage == 0xffff
                || stats.soc == 0xdd || stats.soc == 0xff {
                    write!(&mut battstats_tv, "{}", t!("stats.measuring", xous::LANG)).unwrap();
                } else {
                    // toggle between two views of the data every time we have a status update
                    let mut wattage = stats.current as f32 / 1000.0 * stats.voltage as f32 / 1000.0;
                    let sign = if wattage > 0.005 {
                        '\u{2b06}' // up arrow
                    } else if wattage < -0.005 {
                        '\u{2b07}' // down arrow
                    } else {
                        '\u{1f50c}' // plugged in icon (e.g., fully charged, running on wall power now)
                    };
                    wattage = wattage.abs();
                    if battstats_phase {
                        write!(&mut battstats_tv, "{:.3}W{}{:.2}V {}%", wattage, sign, stats.voltage as f32 / 1000.0, stats.soc).unwrap();
                    } else {
                        if let Some(ssid) = wifi_status.ssid {
                            write!(
                                &mut battstats_tv,
                                "{} -{}dBm",
                                ssid.name.as_str().unwrap_or("UTF-8 Erorr"),
                                ssid.rssi,
                            ).unwrap();
                        } else {
                            write!(
                                &mut battstats_tv,
                                "{}",
                                t!("stats.disconnected", xous::LANG)
                            ).unwrap();
                        }
                    }
                }
                gam.post_textview(&mut battstats_tv)
                    .expect("|status: can't draw battery stats");
                if let Some(bounds) = battstats_tv.bounds_computed {
                    if bounds.height() as i16 > screensize.y / 2 + 1 {
                        // the clipping rectangle limits the bounds to the overall height of the status area, so
                        // the overlap between status and secnotes must be managed within this server
                        log::info!("Status text overstepped its intended bound. Forcing secnotes redraw.");
                        secnotes_force_redraw = true;
                    }
                }
                battstats_phase = !battstats_phase;
            }),
            Some(StatusOpcode::WifiStats) => {
                let buffer = unsafe {
                    xous_ipc::Buffer::from_memory_message(msg.body.memory_message().unwrap())
                };
                wifi_status = WlanStatus::from_ipc(buffer.to_original::<com::WlanStatusIpc, _>().unwrap());
            },
            Some(StatusOpcode::Pump) => {
                let elapsed_time = ticktimer.elapsed_ms();
                { // update the CPU load bar
                    let mut draw_list = GamObjectList::new(status_gid);
                    draw_list.push(GamObjectType::Rect(cpuload_rect)).unwrap();
                    let (latest_activity, period) = llio
                        .activity_instantaneous()
                        .expect("couldn't get CPU activity");
                    let activity_to_width = ((latest_activity as f32) / (period as f32)) * (cpuload_rect.width() - 4) as f32;
                    draw_list.push(GamObjectType::Rect(
                        Rectangle::new_coords_with_style(
                            cpuload_rect.tl().x + 2,
                            cpuload_rect.tl().y + 2,
                            cpuload_rect.tl().x + 2 + activity_to_width as i16,
                            cpuload_rect.br().y - 2,
                            DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 0))
                    )).unwrap();
                    gam.draw_list(draw_list).expect("couldn't draw object list");
                }

                // update the security status, if any
                let (is_locked, force_update) = llio.debug_usb(None).unwrap();
                if (debug_locked != is_locked)
                    || force_update || secnotes_force_redraw
                    || sec_notes.lock().unwrap().len() != last_sec_note_size
                    || /*(sec_notes.lock().unwrap().len() > 1) // force the redraw periodically to clean up any tb overflow from uptime
                        &&*/ ((stats_phase % secnotes_interval) == 0)
                {
                    log::debug!("updating lock state text");
                    if debug_locked != is_locked {
                        if is_locked {
                            sec_notes
                                .lock()
                                .unwrap()
                                .remove(&"secnote.usb_unlock".to_string()); // this is the key, not the value to remove
                        } else {
                            sec_notes.lock().unwrap().insert(
                                "secnotes.usb_unlock".to_string(),
                                t!("secnote.usb_unlock", xous::LANG).to_string(),
                            );
                        }
                        debug_locked = is_locked;
                    }

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
                                last_sec_note_index =
                                    (last_sec_note_index + 1) % last_sec_note_size;
                                break;
                            }
                        }
                    } else {
                        write!(&mut security_tv, "{}", t!("secnote.allclear", xous::LANG)).unwrap();
                    }

                    secnotes_force_redraw = false;
                    gam.post_textview(&mut security_tv).unwrap();
                    gam.draw_line(status_gid, Line::new_with_style(
                        Point::new(0, screensize.y), screensize,
                        DrawStyle::new(PixelColor::Light, PixelColor::Light, 1))).unwrap();
                }
                if (stats_phase % batt_interval) == (batt_interval - 1) {
                    com.req_batt_stats()
                        .expect("Can't get battery stats from COM");
                }
                if (stats_phase % charger_pump_interval) == 1 {
                    // stagger periodic tasks
                    // confirm that the charger is in the right state.
                    if stats.soc < 95 || stats.remaining_capacity < 1000 {
                        // only request if we aren't fully charged, either by SOC or capacity metrics
                        if (llio.adc_vbus().unwrap() as f64) * 0.005033 > 4.45 {
                            // 4.45V is our threshold for deciding if a cable is present
                            // charging cable is present
                            if !com.is_charging().expect("couldn't check charging state") {
                                // not charging, but cable is present
                                log::debug!("Charger present, but not currently charging. Automatically requesting charge start.");
                                com.request_charging()
                                    .expect("couldn't send charge request");
                            }
                        }
                    }
                }
                if (stats_phase % dt_pump_interval) == 2 {
                    #[cfg(any(target_os = "none", target_os = "xous"))]
                    rtc.request_datetime()
                        .expect("|status: can't request datetime from RTC");
                    #[cfg(not(any(target_os = "none", target_os = "xous")))]
                    {
                        log::trace!("hosted request of date time - short circuiting server call");
                        use chrono::prelude::*;
                        use llio::Weekday;
                        let now = Local::now();
                        let wday: Weekday = match now.weekday() {
                            chrono::Weekday::Mon => Weekday::Monday,
                            chrono::Weekday::Tue => Weekday::Tuesday,
                            chrono::Weekday::Wed => Weekday::Wednesday,
                            chrono::Weekday::Thu => Weekday::Thursday,
                            chrono::Weekday::Fri => Weekday::Friday,
                            chrono::Weekday::Sat => Weekday::Saturday,
                            chrono::Weekday::Sun => Weekday::Sunday,
                        };
                        datetime = Some(llio::DateTime {
                            seconds: now.second() as u8,
                            minutes: now.minute() as u8,
                            hours: now.hour() as u8,
                            months: now.month() as u8,
                            days: now.day() as u8,
                            years: (now.year() - 2000) as u8,
                            weekday: wday,
                        });
                    }
                }

                { // update the time field
                    // have to clear the entire rectangle area, because the text has a variable width and dirty text will remain if the text is shortened
                    gam.draw_rectangle(status_gid, time_rect).ok();
                    uptime_tv.clear_str();
                    if let Some(dt) = datetime {
                        write!(
                            &mut uptime_tv,
                            "{:02}:{:02} {}/{}",
                            dt.hours, dt.minutes, dt.months, dt.days
                        )
                        .unwrap();
                    } else {
                        write!(
                            &mut uptime_tv,
                            "Invalid RTC"
                        ).unwrap();
                    }
                    // use ticktimer, not stats_phase, because stats_phase encodes some phase drift due to task-switching overhead
                    write!(
                        &mut uptime_tv,
                        " {}{}:{:02}:{:02}",
                        t!("stats.uptime", xous::LANG),
                        (elapsed_time / 3_600_000),
                        (elapsed_time / 60_000) % 60,
                        (elapsed_time / 1000) % 60,
                    )
                    .expect("|status: can't write string");
                    gam.post_textview(&mut uptime_tv)
                        .expect("|status: can't draw uptime");
                    if let Some(bounds) = uptime_tv.bounds_computed {
                        if bounds.height() as i16 > screensize.y / 2 + 1 {
                            // the clipping rectangle limits the bounds to the overall height of the status area, so
                            // the overlap between status and secnotes must be managed within this server
                            log::info!("Status text overstepped its intended bound. Forcing secnotes redraw.");
                            secnotes_force_redraw = true;
                        }
                    }
                }
                log::trace!("status redraw## update");
                gam.redraw().expect("|status: couldn't redraw");

                stats_phase = stats_phase.wrapping_add(1);
            }
            Some(StatusOpcode::DateTime) => {
                //log::info!("got DateTime update");
                let buffer = unsafe {
                    xous_ipc::Buffer::from_memory_message(msg.body.memory_message().unwrap())
                };
                let dt = buffer.to_original::<llio::DateTime, _>().unwrap();
                datetime = Some(dt);
            }
            Some(StatusOpcode::UxSetTime) => msg_scalar_unpack!(msg, _, _, _, _, {
                pump_run.store(false, Ordering::Relaxed); // stop status updates while we do this
                let secs: u8;
                let mins: u8;
                let hours: u8;
                let months: u8;
                let days: u8;
                let years: u8;
                let weekday: Weekday;

                months = modals.get_text(
                    t!("rtc.month", xous::LANG),
                    Some(rtc_ux_validator), Some(ValidatorOp::UxMonth.to_u32().unwrap())
                ).expect("couldn't get month").as_str()
                .parse::<u8>().expect("pre-validated input failed to re-parse!");
                log::debug!("got months {}", months);

                days = modals.get_text(
                    t!("rtc.day", xous::LANG),
                    Some(rtc_ux_validator), Some(ValidatorOp::UxDay.to_u32().unwrap())
                ).expect("couldn't get month").as_str()
                .parse::<u8>().expect("pre-validated input failed to re-parse!");
                log::debug!("got days {}", days);

                years = modals.get_text(
                    t!("rtc.year", xous::LANG),
                    Some(rtc_ux_validator), Some(ValidatorOp::UxYear.to_u32().unwrap())
                ).expect("couldn't get month").as_str()
                .parse::<u8>().expect("pre-validated input failed to re-parse!");
                log::debug!("got years {}", years);

                for dow in day_of_week_list.iter() {
                    modals.add_list_item(dow).expect("couldn't build day of week list");
                }
                let payload = modals.get_radiobutton(t!("rtc.day_of_week", xous::LANG)).expect("couldn't get day of week");
                weekday =
                    if payload.as_str() == t!("rtc.monday", xous::LANG) {
                        Weekday::Monday
                    } else if payload.as_str() == t!("rtc.tuesday", xous::LANG) {
                        Weekday::Tuesday
                    } else if payload.as_str() == t!("rtc.wednesday", xous::LANG) {
                        Weekday::Wednesday
                    } else if payload.as_str() == t!("rtc.thursday", xous::LANG) {
                        Weekday::Thursday
                    } else if payload.as_str() == t!("rtc.friday", xous::LANG) {
                        Weekday::Friday
                    } else if payload.as_str() == t!("rtc.saturday", xous::LANG) {
                        Weekday::Saturday
                    } else {
                        Weekday::Sunday
                    };
                log::debug!("got weekday {:?}", weekday);

                hours = modals.get_text(
                    t!("rtc.hour", xous::LANG),
                    Some(rtc_ux_validator), Some(ValidatorOp::UxHour.to_u32().unwrap())
                ).expect("couldn't get hour").as_str()
                .parse::<u8>().expect("pre-validated input failed to re-parse!");
                log::debug!("got hours {}", hours);

                mins = modals.get_text(
                    t!("rtc.minute", xous::LANG),
                    Some(rtc_ux_validator), Some(ValidatorOp::UxMinute.to_u32().unwrap())
                ).expect("couldn't get minutes").as_str()
                .parse::<u8>().expect("pre-validated input failed to re-parse!");
                log::debug!("got minutes {}", mins);

                secs = modals.get_text(
                    t!("rtc.seconds", xous::LANG),
                    Some(rtc_ux_validator), Some(ValidatorOp::UxSeconds.to_u32().unwrap())
                ).expect("couldn't get seconds").as_str()
                .parse::<u8>().expect("pre-validated input failed to re-parse!");
                log::debug!("got seconds {}", secs);

                log::info!("Setting time: {}/{}/{} {}:{}:{} {:?}", months, days, years, hours, mins, secs, weekday);
                let dt = llio::DateTime {
                    seconds: secs,
                    minutes: mins,
                    hours,
                    days,
                    months,
                    years,
                    weekday
                };
                rtc.set_rtc(dt).expect("couldn't set the current time");
                pump_run.store(true, Ordering::Relaxed); // stop status updates while we do this
            }),
            Some(StatusOpcode::Reboot) => {
                if ((llio.adc_vbus().unwrap() as f64) * 0.005033) > 1.5 {
                    // power plugged in, do a reboot using a warm boot method
                    susres.reboot(true).expect("couldn't issue reboot command");
                } else {
                    // ensure the self-boosting facility is turned off, this interferes with a cold boot
                    com.set_boost(false).ok();
                    llio.boost_on(false).ok();
                    // do a full cold-boot if the power is cut. This will force a re-load of the SoC contents.
                    gam.shipmode_blank_request().ok();
                    ticktimer.sleep_ms(500).ok(); // screen redraw time after the blank request
                    rtc.set_wakeup_alarm(4).expect("couldn't set wakeup alarm");
                    llio.allow_ec_snoop(true).ok();
                    llio.allow_power_off(true).ok();
                    com.power_off_soc().ok();
                    ticktimer.sleep_ms(4000).ok();
                    panic!("system did not reboot");
                }
            }
            Some(StatusOpcode::SubmenuPddb) => {
                ticktimer.sleep_ms(100).ok(); // yield for a moment to allow the previous menu to close
                gam.raise_menu(gam::PDDB_MENU_NAME).expect("couldn't raise PDDB submenu");
            },
            Some(StatusOpcode::SubmenuApp) => {
                ticktimer.sleep_ms(100).ok(); // yield for a moment to allow the previous menu to close
                gam.raise_menu(gam::APP_MENU_NAME).expect("couldn't raise App submenu");
            },
            Some(StatusOpcode::SubmenuKbd) => {
                log::info!("getting keyboard map");
                let map = kbd.get_keymap().expect("couldn't get key mapping");
                log::info!("setting index to {:?}", map);
                kbd_menumatic.set_index(map.into());
                log::info!("raising keyboard menu");
                ticktimer.sleep_ms(100).ok(); // yield for a moment to allow the previous menu to close
                gam.raise_menu(gam::KBD_MENU_NAME).expect("couldn't raise keyboard layout submenu");
            },
            Some(StatusOpcode::SetKeyboard) => msg_scalar_unpack!(msg, code, _, _, _, {
                let map = keyboard::KeyMap::from(code);
                kbd.set_keymap(map).expect("couldn't set keyboard mapping");
            }),
            Some(StatusOpcode::SwitchToShellchat) => {
                ticktimer.sleep_ms(100).ok();
                sec_notes.lock().unwrap().remove(&"current_app".to_string());
                sec_notes.lock().unwrap().insert("current_app".to_string(), format!("Running: Shellchat").to_string());
                gam.switch_to_app(gam::APP_NAME_SHELLCHAT, security_tv.token.unwrap()).expect("couldn't raise shellchat");
                secnotes_force_redraw = true;
                send_message(
                    cb_cid,
                    Message::new_scalar(StatusOpcode::Pump.to_usize().unwrap(), 0, 0, 0, 0),
                ).expect("couldn't trigger status update");
            },
            Some(StatusOpcode::SwitchToApp) => msg_scalar_unpack!(msg, index, _, _, _, {
                ticktimer.sleep_ms(100).ok();
                let app_name = app_autogen::app_index_to_name(index).expect("app index not found");
                app_autogen::app_dispatch(&gam, security_tv.token.unwrap(), index).expect("cannot switch to app");
                sec_notes.lock().unwrap().remove(&"current_app".to_string());
                sec_notes.lock().unwrap().insert("current_app".to_string(), format!("Running: {}", app_name).to_string());
                secnotes_force_redraw = true;
                send_message(
                    cb_cid,
                    Message::new_scalar(StatusOpcode::Pump.to_usize().unwrap(), 0, 0, 0, 0),
                ).expect("couldn't trigger status update");
            }),
            Some(StatusOpcode::TrySuspend) => {
                if ((llio.adc_vbus().unwrap() as f64) * 0.005033) > 1.5 {
                    modals.show_notification(t!("mainmenu.cant_sleep", xous::LANG)).expect("couldn't notify that power is plugged in");
                } else {
                    susres.initiate_suspend().expect("couldn't initiate suspend op");
                }
            },
            Some(StatusOpcode::BatteryDisconnect) => {
                if ((llio.adc_vbus().unwrap() as f64) * 0.005033) > 1.5 {
                    modals.show_notification(t!("mainmenu.cant_sleep", xous::LANG)).expect("couldn't notify that power is plugged in");
                } else {
                    gam.shipmode_blank_request().ok();
                    ticktimer.sleep_ms(500).unwrap();
                    llio.allow_ec_snoop(true).unwrap();
                    llio.allow_power_off(true).unwrap();
                    com.ship_mode().unwrap();
                    com.power_off_soc().unwrap();
                }
            },
            Some(StatusOpcode::Quit) => {
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

// RTC helper functions
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum ValidatorOp {
    UxMonth,
    UxDay,
    UxYear,
    UxHour,
    UxMinute,
    UxSeconds,
}

fn rtc_ux_validator(input: TextEntryPayload, opcode: u32) -> Option<ValidatorErr> {
    let text_str = input.as_str();
    let input_int = match text_str.parse::<u32>() {
        Ok(input_int) => input_int,
        _ => return Some(ValidatorErr::from_str(t!("rtc.integer_err", xous::LANG))),
    };
    log::trace!("validating input {}, parsed as {} for opcode {}", text_str, input_int, opcode);
    match FromPrimitive::from_u32(opcode) {
        Some(ValidatorOp::UxMonth) => {
            if input_int < 1 || input_int > 12 {
                return Some(ValidatorErr::from_str(t!("rtc.range_err", xous::LANG)))
            }
        }
        Some(ValidatorOp::UxDay) => {
            if input_int < 1 || input_int > 31 {
                return Some(ValidatorErr::from_str(t!("rtc.range_err", xous::LANG)))
            }
        }
        Some(ValidatorOp::UxYear) => {
            if input_int > 99 {
                return Some(ValidatorErr::from_str(t!("rtc.range_err", xous::LANG)))
            }
        }
        Some(ValidatorOp::UxHour) => {
            if input_int > 23 {
                return Some(ValidatorErr::from_str(t!("rtc.range_err", xous::LANG)))
            }
        }
        Some(ValidatorOp::UxMinute) => {
            if input_int > 59 {
                return Some(ValidatorErr::from_str(t!("rtc.range_err", xous::LANG)))
            }
        }
        Some(ValidatorOp::UxSeconds) => {
            if input_int > 59 {
                return Some(ValidatorErr::from_str(t!("rtc.range_err", xous::LANG)))
            }
        }
        _ => {
            log::error!("internal error: invalid opcode was sent to validator: {:?}", opcode);
            panic!("internal error: invalid opcode was sent to validator");
        }
    }
    None
}
