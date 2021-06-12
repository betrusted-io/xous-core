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
        ticktimer.sleep_ms(250).unwrap();
    }
}

const SERVER_NAME_STATUS: &str   = "_Status bar manager_";
pub fn status_thread(canvas_gid_0: usize, canvas_gid_1: usize, canvas_gid_2: usize, canvas_gid_3: usize) {
    let canvas_gid = [canvas_gid_0 as u32, canvas_gid_1 as u32, canvas_gid_2 as u32, canvas_gid_3 as u32];

    let status_gid: Gid = Gid::new(canvas_gid);
    log::trace!("|status: my canvas {:?}", status_gid);

    log::trace!("|status: registering GAM|status thread");
    let xns = xous_names::XousNames::new().unwrap();
    let status_sid = xns.register_name(SERVER_NAME_STATUS).expect("|status: can't register server");
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
    log::trace!("|status: screensize as reported: {:?}", screensize);
    log::trace!("|status: uptime initialized to '{:?}'", uptime_tv);

    // build battstats text view: right half of status bar
    let mut battstats_tv = TextView::new(status_gid,
        TextBounds::BoundingBox(Rectangle::new(Point::new(screensize.x / 2, 0),
               Point::new(screensize.x, screensize.y / 2 - 1))));
    battstats_tv.style = blitstr::GlyphStyle::Small;
    battstats_tv.draw_border = false;
    battstats_tv.margin = Point::new(0, 0);

    // initialize to some "sane" mid-point defaults, so we don't trigger errors later on before the first real battstat reading comes
    let mut stats = BattStats {
        voltage: 3700,
        soc: 50,
        current: 0,
        remaining_capacity: 650,
    };
    let mut last_time: u64 = ticktimer.elapsed_ms();
    let mut stats_phase: usize = 0;
    let mut last_seconds: usize = ((last_time / 1000) % 60) as usize;

    let style_dark = DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1);
    gam.draw_line(status_gid, Line::new_with_style(
        Point::new(0, screensize.y),
        Point::new(screensize.x, screensize.y),
        style_dark
    )).expect("|status: Can't draw border line");

    com.hook_batt_stats(battstats_cb).expect("|status: couldn't hook callback for events from COM");
    // prime the loop
    com.req_batt_stats().expect("Can't get battery stats from COM");

    let mut rtc = rtc::Rtc::new(&xns).unwrap();

    #[cfg(target_os = "none")]
    rtc.clear_wakeup_alarm().unwrap(); // clear any wakeup alarm state, if it was set

    rtc.hook_rtc_callback(dt_callback).unwrap();
    let mut datetime: Option<rtc::DateTime> = None;
    let mut dt_pump_modulus = 15;

    let mut charger_pump_modulus = 0;
    let llio = llio::Llio::new(&xns).unwrap();

    let mut debug_unlocked = llio.debug_usb_unlocked().unwrap();
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

    let secs_interval;
    let batt_interval;
    if cfg!(feature = "slowstatus") {
        // lower the status output rate for braille mode, debugging, etc.
        secs_interval = 10;
        batt_interval = 5000;
    } else {
        secs_interval = 1;
        batt_interval = 500;
    }

    last_seconds = last_seconds - 1; // this will force the uptime to redraw
    info!("|status: starting main loop");
    loop {
        let msg = xous::receive_message(status_sid).unwrap();
        //let msg = xous::receive_message(status_sid).unwrap();
        log::trace!("|status: Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(StatusOpcode::BattStats) => msg_scalar_unpack!(msg, lo, hi, _, _, {
                stats = [lo, hi].into();
                battstats_tv.clear_str();
                // toggle between two views of the data; duration of toggle is set by the modulus and thresholds below
                if stats_phase > 3 {
                    write!(&mut battstats_tv, "{}mV {}mA", stats.voltage, stats.current).expect("|status: can't write string");
                } else {
                    write!(&mut battstats_tv, "{}mAh {}%", stats.remaining_capacity, stats.soc).expect("|status: can't write string");
                }
            }),
            Some(StatusOpcode::Pump) => {
                if debug_unlocked != llio.debug_usb_unlocked().unwrap() {
                    debug_unlocked = llio.debug_usb_unlocked().unwrap();
                    security_tv.clear_str();
                    if debug_unlocked {
                        write!(&mut security_tv, " USB unlocked").unwrap();
                    } else {
                        write!(&mut security_tv, " USB secured").unwrap();
                    }
                }
                let elapsed_time = ticktimer.elapsed_ms();
                let now_seconds: usize = ((elapsed_time / 1000) % 60) as usize;
                if (now_seconds / secs_interval) != (last_seconds / secs_interval) {
                    charger_pump_modulus += 1;
                    if charger_pump_modulus > 60 {
                        // once a minute confirm that the charger is in the right state.
                        charger_pump_modulus = 0;
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
                    dt_pump_modulus += 1;
                    if dt_pump_modulus > 15 {
                        dt_pump_modulus = 0;
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
                    last_seconds = now_seconds;
                    uptime_tv.clear_str();
                    if (stats_phase > 3) && datetime.is_some() {
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
                        write!(&mut uptime_tv, "{:02}:{:02} {} {}/{}", dt.hours, dt.minutes, day, dt.months, dt.days).unwrap();
                    } else {
                        let (latest_activity, period) = llio.activity_instantaneous().expect("couldn't get CPU activity");
                        write!(&mut uptime_tv, "Up {}:{:02}:{:02} {:.0}%",
                            (elapsed_time / 3_600_000), (elapsed_time / 60_000) % 60, now_seconds,
                            ((latest_activity as f32) / (period as f32)) * 100.0
                        ).expect("|status: can't write string");
                    }
                    log::trace!("|status: requesting draw of '{}'", uptime_tv);
                    gam.post_textview(&mut uptime_tv).expect("|status: can't draw uptime");
                    gam.post_textview(&mut battstats_tv).expect("|status: can't draw battery stats");
                    gam.post_textview(&mut security_tv).unwrap();
                    gam.redraw().expect("|status: couldn't redraw");
                    stats_phase = (stats_phase + 1) % 8;
                }
                if elapsed_time - last_time > batt_interval {
                    //info!("|status: size of TextView type: {} bytes", core::mem::size_of::<TextView>());
                    log::trace!("|status: periodic tasks: updating uptime, requesting battstats");
                    last_time = elapsed_time;
                    com.req_batt_stats().expect("Can't get battery stats from COM");
                }
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
