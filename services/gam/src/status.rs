use log::info;
use com::api::BattStats;
use graphics_server::*;

use core::fmt::Write;

use blitstr_ref as blitstr;

use xous::{send_message, CID, Message, msg_scalar_unpack};
use num_traits::{ToPrimitive, FromPrimitive};

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
enum StatusOpcode {
    // for passing battstats on to the main thread from the callback
    BattStats,

    // for passing DateTime
    DateTime,

    // indicates time for periodic update of the status bar
    Pump,
}

static mut CB_TO_MAIN_CONN: Option<CID> = None;
fn battstats_cb(stats: BattStats) {
    if let Some(cb_to_main_conn) = unsafe{CB_TO_MAIN_CONN} {
        let rawstats: [usize; 2] = stats.into();
        send_message(cb_to_main_conn,
            xous::Message::new_scalar(StatusOpcode::BattStats.to_usize().unwrap(),
            rawstats[0], rawstats[1], 0, 0
        )).unwrap();
    }
}

pub fn dt_callback(dt: rtc::DateTime) {
    //log::info!("dt_callback received with {:?}", dt);
    if let Some(cb_to_main_conn) = unsafe{CB_TO_MAIN_CONN} {
        let buf = xous_ipc::Buffer::into_buf(dt).or(Err(xous::Error::InternalError)).unwrap();
        buf.send(cb_to_main_conn, StatusOpcode::DateTime.to_u32().unwrap()).unwrap();
    }
}

pub fn pump_thread(conn: usize) {
    let ticktimer = ticktimer_server::Ticktimer::new().unwrap();
    loop {
        match send_message(conn as u32,
            Message::new_scalar(StatusOpcode::Pump.to_usize().unwrap(), 0, 0, 0, 0)
        ) {
            Err(xous::Error::ServerNotFound) => break,
            Ok(xous::Result::Ok) => {},
            _ => panic!("unhandled error in status pump thread")
        }
        ticktimer.sleep_ms(1000).unwrap();
    }
}

const SERVER_NAME_STATUS: &str   = "_Status bar manager_";
pub fn status_thread(canvas_gid_0: usize, canvas_gid_1: usize, canvas_gid_2: usize, canvas_gid_3: usize) {
    let canvas_gid = [canvas_gid_0 as u32, canvas_gid_1 as u32, canvas_gid_2 as u32, canvas_gid_3 as u32];

    let status_gid: Gid = Gid::new(canvas_gid);
    log::trace!("|status: my canvas {:?}", status_gid);

    log::trace!("|status: registering GAM|status thread");
    let xns = xous_names::XousNames::new().unwrap();
    // should be only one connection here, from the status main loop
    let status_sid = xns.register_name(SERVER_NAME_STATUS, Some(1)).expect("|status: can't register server");
    // create a connection for callback hooks
    unsafe{CB_TO_MAIN_CONN = Some(xous::connect(status_sid).unwrap())};
    let pump_conn = xous::connect(status_sid).unwrap();
    xous::create_thread_1(pump_thread, pump_conn as _).expect("couldn't create pump thread");

    let gam = gam::Gam::new(&xns).expect("|status: can't connect to GAM");
    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");
    let mut com = com::Com::new(&xns).expect("|status: can't connect to COM");

    log::trace!("|status: getting screen size");
    let screensize = gam.get_canvas_bounds(status_gid).expect("|status: Couldn't get canvas size");
    //let screensize: Point = Point::new(0, 336);

    log::trace!("|status: building textview objects");
    // build uptime text view: left half of status bar
    let mut uptime_tv = TextView::new(status_gid,
         TextBounds::BoundingBox(Rectangle::new(Point::new(0,0),
                 Point::new(screensize.x / 2, screensize.y / 2 - 1))));
    uptime_tv.untrusted = false;
    uptime_tv.style = blitstr::GlyphStyle::Small;
    uptime_tv.draw_border = false;
    uptime_tv.margin = Point::new(3, 0);
    write!(uptime_tv, "Booting up...").expect("|status: couldn't init uptime text");
    gam.post_textview(&mut uptime_tv).expect("|status: can't draw battery stats");
    log::trace!("|status: screensize as reported: {:?}", screensize);
    log::trace!("|status: uptime initialized to '{:?}'", uptime_tv);

    // build battstats text view: right half of status bar
    let mut battstats_tv = TextView::new(status_gid,
        TextBounds::BoundingBox(Rectangle::new(Point::new(screensize.x / 2, 0),
               Point::new(screensize.x, screensize.y / 2 - 1))));
    battstats_tv.style = blitstr::GlyphStyle::Small;
    battstats_tv.draw_border = false;
    battstats_tv.margin = Point::new(0, 0);
    gam.post_textview(&mut battstats_tv).expect("|status: can't draw battery stats");

    // initialize to some "sane" mid-point defaults, so we don't trigger errors later on before the first real battstat reading comes
    let mut stats = BattStats {
        voltage: 3700,
        soc: 50,
        current: 0,
        remaining_capacity: 650,
    };

    let style_dark = DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1);
    gam.draw_line(status_gid, Line::new_with_style(
        Point::new(0, screensize.y),
        Point::new(screensize.x, screensize.y),
        style_dark
    )).expect("|status: Can't draw border line");

    com.hook_batt_stats(battstats_cb).expect("|status: couldn't hook callback for events from COM");
    // prime the loop
    com.req_batt_stats().expect("Can't get battery stats from COM");

    log::debug!("initializing RTC...");
    let mut rtc = rtc::Rtc::new(&xns).unwrap();

    #[cfg(target_os = "none")]
    rtc.clear_wakeup_alarm().unwrap(); // clear any wakeup alarm state, if it was set

    rtc.hook_rtc_callback(dt_callback).unwrap();
    let mut datetime: Option<rtc::DateTime> = None;
    let llio = llio::Llio::new(&xns).unwrap();

    log::debug!("usb unlock notice...");
    let (dl, _) = llio.debug_usb(None).unwrap();
    let mut debug_locked = dl;
    // build security status textview
    let mut security_tv = TextView::new(status_gid,
        TextBounds::BoundingBox(Rectangle::new(Point::new(0,screensize.y / 2),
                Point::new(screensize.x, screensize.y - 1))));
    security_tv.style = blitstr::GlyphStyle::Small;
    security_tv.draw_border = false;
    security_tv.margin = Point::new(0, 0);
    security_tv.token = gam.claim_token("status").expect("couldn't request token"); // this is a shared magic word to identify this process
    security_tv.clear_area = true;
    security_tv.invert = true;
    write!(&mut security_tv, " USB unlocked").unwrap();
    gam.post_textview(&mut security_tv).unwrap();
    gam.redraw().unwrap();  // initial boot redraw

    let mut stats_phase: usize = 0;

    let dt_pump_interval = 15;
    let charger_pump_interval = 60;
    let stats_interval;
    let batt_interval;
    if cfg!(feature = "slowstatus") {
        // lower the status output rate for braille mode, debugging, etc.
        stats_interval = 30;
        batt_interval = 60;
    } else {
        stats_interval = 4;
        batt_interval = 4;
    }
    let mut battstats_phase = true;
    let mut needs_redraw = false;

    info!("|status: starting main loop");
    loop {
        let msg = xous::receive_message(status_sid).unwrap();
        log::trace!("|status: Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(StatusOpcode::BattStats) => msg_scalar_unpack!(msg, lo, hi, _, _, {
                stats = [lo, hi].into();
                battstats_tv.clear_str();
                // toggle between two views of the data every time we have a status update
                if battstats_phase {
                    write!(&mut battstats_tv, "{}mV {}mA", stats.voltage, stats.current).expect("|status: can't write string");
                } else {
                    write!(&mut battstats_tv, "{}mAh {}%", stats.remaining_capacity, stats.soc).expect("|status: can't write string");
                }
                gam.post_textview(&mut battstats_tv).expect("|status: can't draw battery stats");
                battstats_phase = !battstats_phase;
                needs_redraw = true;
            }),
            Some(StatusOpcode::Pump) => {
                let (is_locked, force_update) = llio.debug_usb(None).unwrap();
                if (debug_locked != is_locked) || force_update {
                    debug_locked = is_locked;
                    security_tv.clear_str();
                    if debug_locked {
                        write!(&mut security_tv, " USB secured").unwrap();
                    } else {
                        write!(&mut security_tv, " USB unlocked").unwrap();
                    }
                    // only post the view if something has actually changed
                    gam.post_textview(&mut security_tv).unwrap();
                    needs_redraw = true;
                }
                if (stats_phase % batt_interval) == (batt_interval - 1) {
                    com.req_batt_stats().expect("Can't get battery stats from COM");
                }

                if (stats_phase % charger_pump_interval) == 1 { // stagger periodic tasks
                    // once a minute confirm that the charger is in the right state.
                    if stats.soc < 95 || stats.remaining_capacity < 1000 { // only request if we aren't fully charged, either by SOC or capacity metrics
                        if (llio.adc_vbus().unwrap() as f64) * 0.005033 > 4.45 { // 4.45V is our threshold for deciding if a cable is present
                            // charging cable is present
                            if !com.is_charging().expect("couldn't check charging state") {
                                // not charging, but cable is present
                                log::info!("Charger present, but not currently charging. Automatically requesting charge start.");
                                com.request_charging().expect("couldn't send charge request");
                            }
                        }
                    }
                }
                if (stats_phase % dt_pump_interval) == 2 {
                    #[cfg(target_os = "none")]
                    rtc.request_datetime().expect("|status: can't request datetime from RTC");
                    #[cfg(not(target_os = "none"))]
                    {
                        log::trace!("hosted request of date time - short circuiting server call");
                        use chrono::prelude::*;
                        use rtc::Weekday;
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
                        datetime = Some(rtc::DateTime {
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
                // date/time only redraws once every stats_interval period; but if needs_redraw is triggered (e.g. due to resume), force a redraw
                if (needs_redraw || ((stats_phase % stats_interval == 0) && ((stats_phase % (stats_interval * 2)) == stats_interval))) && datetime.is_some() {
                    let dt = datetime.unwrap();
                    let day = match dt.weekday {
                        rtc::Weekday::Monday => "Mon",
                        rtc::Weekday::Tuesday => "Tue",
                        rtc::Weekday::Wednesday => "Wed",
                        rtc::Weekday::Thursday => "Thu",
                        rtc::Weekday::Friday => "Fri",
                        rtc::Weekday::Saturday => "Sat",
                        rtc::Weekday::Sunday => "Sun",
                    };
                    uptime_tv.clear_str();
                    write!(&mut uptime_tv, "{:02}:{:02} {} {}/{}", dt.hours, dt.minutes, day, dt.months, dt.days).unwrap();
                    gam.post_textview(&mut uptime_tv).expect("|status: can't draw uptime");
                    needs_redraw = true;
                } else if (stats_phase % (stats_interval * 2)) < stats_interval {
                    uptime_tv.clear_str();
                    let (latest_activity, period) = llio.activity_instantaneous().expect("couldn't get CPU activity");
                    // use ticktimer, not stats_phase, because stats_phase encodes some phase drift due to task-switching overhead
                    let elapsed_time = ticktimer.elapsed_ms();
                    write!(&mut uptime_tv, "Up {}:{:02}:{:02} {:.0}%",
                        (elapsed_time / 3_600_000), (elapsed_time / 60_000) % 60, (elapsed_time / 1000) % 60,
                        ((latest_activity as f32) / (period as f32)) * 100.0
                    ).expect("|status: can't write string");
                    gam.post_textview(&mut uptime_tv).expect("|status: can't draw uptime");
                    needs_redraw = true;
                }
                if needs_redraw {
                    gam.redraw().expect("|status: couldn't redraw");
                    needs_redraw = false;
                }

                stats_phase = stats_phase + 1;
            }
            Some(StatusOpcode::DateTime) => {
                //log::info!("got DateTime update");
                let buffer = unsafe { xous_ipc::Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let dt = buffer.to_original::<rtc::DateTime, _>().unwrap();
                datetime = Some(dt);
            }
            None => {log::error!("|status: received unknown Opcode"); break}
        }
    }
    log::trace!("status thread exit, destroying servers");
    unsafe{
        if let Some(cb)= CB_TO_MAIN_CONN {
            xous::disconnect(cb).unwrap();
        }
    }
    unsafe{xous::disconnect(pump_conn).unwrap();}
    xns.unregister_server(status_sid).unwrap();
    xous::destroy_server(status_sid).unwrap();
    log::trace!("status thread quitting");
}
