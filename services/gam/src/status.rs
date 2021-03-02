use crate::api::*;

use log::{error, info};
use com::api::BattStats;
use graphics_server::*;

use core::fmt::Write;
use core::convert::TryFrom;

use blitstr_ref as blitstr;

pub fn status_thread(canvas_gid: [u32; 4]) {
    let debug1 = false;
    let status_gid: Gid = Gid::new(canvas_gid);

    info!("GAM|status: registering GAM|status thread");
    let status_sid = xous_names::register_name(xous::names::SERVER_NAME_STATUS).expect("GAM|status: can't register server");

    let ticktimer_conn = xous::connect(xous::SID::from_bytes(b"ticktimer-server").unwrap()).unwrap();
    let com_conn = xous_names::request_connection_blocking(xous::names::SERVER_NAME_COM).expect("GAM|status: can't connect to COM");
    let gfx_conn = xous_names::request_connection_blocking(xous::names::SERVER_NAME_GFX).expect("GAM|status: can't connect to COM");
    let gam_conn = xous_names::request_connection_blocking(xous::names::SERVER_NAME_GAM).expect("GAM|status: can't connect to GAM");

    info!("GAM|status: getting screen size");
    let screensize = gam::get_canvas_bounds(gam_conn, status_gid).expect("GAM|status: Couldn't get canvas size");
    //let screensize: Point = Point::new(0, 336);

    info!("GAM|status: building textview objects");
    // build uptime text view: left half of status bar
    let mut uptime_tv = TextView::new(status_gid, 0,
         TextBounds::BoundingBox(Rectangle::new(Point::new(0,0),
                 Point::new(screensize.x / 2, screensize.y - 1))));
    uptime_tv.untrusted = false;
    uptime_tv.style = blitstr::GlyphStyle::Small;
    uptime_tv.draw_border = false;
    uptime_tv.margin = Point::new(0, 0);
    write!(uptime_tv, "Booting up...").expect("GAM|status: couldn't init uptime text");
    info!("GAM|status: screensize as reported: {:?}", screensize);
    info!("GAM|status: uptime initialized to '{:?}'", uptime_tv);

    // build battstats text view: right half of status bar
    let mut battstats_tv = TextView::new(status_gid, 0,
        TextBounds::BoundingBox(Rectangle::new(Point::new(screensize.x / 2, 0),
               Point::new(screensize.x, screensize.y - 1))));
    battstats_tv.style = blitstr::GlyphStyle::Small;
    battstats_tv.draw_border = false;
    battstats_tv.margin = Point::new(0, 0);

    let mut stats: BattStats = BattStats::default();
    let mut last_time: u64 = ticktimer_server::elapsed_ms(ticktimer_conn).unwrap();
    let mut stats_phase: usize = 0;

    let style_dark = DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1);
    gam::draw_line(gam_conn, status_gid, Line::new_with_style(
        Point::new(0, screensize.y),
        Point::new(screensize.x, screensize.y),
        style_dark
    )).expect("GAM|status: Can't draw border line");

    com::request_battstat_events(xous::names::SERVER_NAME_STATUS, com_conn).expect("GAM|status: couldn't request events from COM");
    info!("GAM|status: starting main loop");
    loop {
        /*
        if debug1{info!("GAM|status: periodic tasks: updating uptime, requesting battstats");}
        let elapsed_time = ticktimer_server::elapsed_ms(ticktimer_conn).expect("GAM|status: error requesting uptime");
        com::get_batt_stats_nb(com_conn).expect("Can't get battery stats from COM");
        uptime_tv.clear_str();
        write!(&mut uptime_tv, "Up {:02}:{:02}:{:02}",
           (elapsed_time / 3_600_000), (elapsed_time / 60_000) % 60, (elapsed_time / 1000) % 60).expect("GAM|status: can't write string");
        if debug1{info!("GAM|status: requesting draw of '{}'", uptime_tv);}
        gam::post_textview(gam_conn, &mut uptime_tv).expect("GAM|status: can't draw uptime");*/

        let maybe_env = xous::try_receive_message(status_sid).unwrap();
        match maybe_env {
            Some(envelope) => {
                //let envelope = xous::receive_message(status_sid).unwrap();
                if debug1{info!("GAM|status: Message: {:?}", envelope);}
                if let Ok(opcode) = com::api::Opcode::try_from(&envelope.body) {
                    match opcode {
                        com::api::Opcode::BattStatsEvent(s) => {
                            stats = s.clone();
                            battstats_tv.clear_str();
                            // toggle between two views of the data; duration of toggle is set by the modulus and thresholds below
                            if stats_phase > 4 {
                                write!(&mut battstats_tv, "{}mV {}mA", stats.voltage, stats.current).expect("GAM|status: can't write string");
                            } else {
                                write!(&mut battstats_tv, "{}mAh {}%", stats.remaining_capacity, stats.soc).expect("GAM|status: can't write string");
                            }
                            stats_phase = (stats_phase + 1) % 8;
                            gam::post_textview(gam_conn, &mut battstats_tv).expect("GAM|status: can't draw battery stats");
                        },
                        _ => error!("GAM|status received COM event opcode that wasn't expected"),
                    }
                } else {
                    error!("GAM|status: couldn't convert opcode");
                }
            }
            _ => xous::yield_slice(),
        }

        //ticktimer_server::sleep_ms(ticktimer_conn, 1000).expect("couldn't sleep");

        if let Ok(elapsed_time) = ticktimer_server::elapsed_ms(ticktimer_conn) {
            if elapsed_time - last_time > 500 {
                //info!("GAM|status: size of TextView type: {} bytes", core::mem::size_of::<TextView>());
                if debug1{info!("GAM|status: periodic tasks: updating uptime, requesting battstats");}
                last_time = elapsed_time;
                com::get_batt_stats_nb(com_conn).expect("Can't get battery stats from COM");
                uptime_tv.clear_str();
                write!(&mut uptime_tv, "Up {:02}:{:02}:{:02}",
                   (elapsed_time / 3_600_000), (elapsed_time / 60_000) % 60, (elapsed_time / 1000) % 60).expect("GAM|status: can't write string");
                if debug1{info!("GAM|status: requesting draw of '{}'", uptime_tv);}
                gam::post_textview(gam_conn, &mut uptime_tv).expect("GAM|status: can't draw uptime");
            }
        } else {
            error!("error requesting ticktimer!")
        }
    }

}