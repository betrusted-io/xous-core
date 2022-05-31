#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

/***************
NOTE: this assumes that you turn off the watchdog timer. This code does not include enough sleeps to reset the WDT
Do this by removing the watchdog feature in the ticktimer-server Cargo.toml crate
****************/
use blitstr::GlyphStyle;
use blitstr_ref as blitstr;
use core::fmt::Write;
use graphics_server::{DrawStyle, PixelColor, Point, Rectangle, TextBounds, TextOp, TextView};

use log::{error, info};
use num_traits::{FromPrimitive, ToPrimitive};

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum Opcode {
    Start,
    Stop,
}

const SERVER_NAME_SHELL: &str = "_Shell_";

fn stopwatch_thread() {
    info!("BENCHMARK|stopwatch: starting");

    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");

    let xns = xous_names::XousNames::new().unwrap();
    let shell_conn = xns
        .request_connection_blocking(SERVER_NAME_SHELL)
        .expect("BENCHMARK|stopwatch: can't connect to main program");
    let mut last_time: u64 = ticktimer.elapsed_ms();
    let mut start_sent = false;
    loop {
        let elapsed_time = ticktimer.elapsed_ms();
        if false {
            if elapsed_time - last_time > 500 && !start_sent {
                last_time = elapsed_time;
                xous::send_message(
                    shell_conn,
                    xous::Message::new_scalar(Opcode::Start.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .expect("BENCHMARK|stopwatch: couldn't send Start message");
                start_sent = true;
            } else if elapsed_time - last_time > 10_000 && start_sent {
                last_time = elapsed_time;
                start_sent = false;
                xous::send_message(
                    shell_conn,
                    xous::Message::new_scalar(Opcode::Stop.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .expect("BENCHMARK|stopwatch: couldn't send Start message");
            }
        } else {
            // send a start loop message
            xous::send_message(
                shell_conn,
                xous::Message::new_scalar(Opcode::Start.to_usize().unwrap(), 0, 0, 0, 0),
            )
            .expect("BENCHMARK|stopwatch: couldn't send Start message");
            ticktimer.sleep_ms(10_000).expect("couldn't sleep");
            // send a stop loop message
            xous::send_message(
                shell_conn,
                xous::Message::new_scalar(Opcode::Stop.to_usize().unwrap(), 0, 0, 0, 0),
            )
            .expect("BENCHMARK|stopwatch: couldn't send Start message");
            // give a moment for the result to update
            ticktimer.sleep_ms(500).expect("couldn't sleep");
        }
        xous::yield_slice();
    }
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    info!("BENCHMARK: my PID is {}", xous::process::id());

    info!("BENCHMARK: ticktimer");
    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");

    let xns = xous_names::XousNames::new().unwrap();
    let shell_server = xns
        .register_name(SERVER_NAME_SHELL, None)
        .expect("BENCHMARK: can't register server");

    let gfx = graphics_server::Gfx::new(&xns).unwrap();
    let target_conn = xns
        .request_connection_blocking(benchmark_target::api::SERVER_NAME_BENCHMARK)
        .expect("BENCHMARK: can't connect to COM");

    xous::create_thread_0(stopwatch_thread).unwrap();
    info!("BENCHMARK: stopwatch thread started");

    let screensize = gfx.screen_size().expect("Couldn't get screen size");

    let font_h: i16 = gfx
        .glyph_height_hint(GlyphStyle::Regular)
        .expect("couldn't get glyph height") as i16;

    let status_clipregion = Rectangle::new_coords_with_style(
        4,
        0,
        screensize.x,
        font_h as i16 * 4,
        DrawStyle::new(PixelColor::Light, PixelColor::Light, 1),
    );

    gfx.draw_rectangle(Rectangle::new_with_style(
        Point::new(0, 0),
        screensize,
        DrawStyle::new(PixelColor::Light, PixelColor::Light, 0),
    ))
    .expect("unable to clear region");

    let mut result_tv = TextView::new(
        graphics_server::Gid::new([0, 0, 0, 0]),
        TextBounds::BoundingBox(Rectangle::new(
            Point::new(0, 0),
            Point::new(screensize.x, screensize.y - 1),
        )),
    );
    result_tv.set_op(TextOp::Render);
    result_tv.clip_rect = Some(status_clipregion.into());
    result_tv.untrusted = false;
    result_tv.style = blitstr::GlyphStyle::Regular;
    result_tv.draw_border = false;
    result_tv.margin = Point::new(3, 0);
    write!(result_tv, "Initializing...").expect("couldn't init text");
    gfx.draw_textview(&mut result_tv).unwrap();

    gfx.flush().expect("unable to draw to screen");

    let mut start_time: u64 = 0;
    let mut stop_time: u64 = 0;
    let mut update_result: bool = false;
    let mut count: u32 = 0;
    let mut check_count: u32 = 0;
    loop {
        let maybe_env = xous::try_receive_message(shell_server).unwrap();
        match maybe_env {
            Some(envelope) => {
                info!("BENCHMARK: Message: {:?}", envelope);
                match FromPrimitive::from_usize(envelope.body.id()) {
                    Some(Opcode::Start) => {
                        start_time = ticktimer.elapsed_ms();
                    }
                    Some(Opcode::Stop) => {
                        stop_time = ticktimer.elapsed_ms();
                        update_result = true;
                    }
                    None => {
                        error!("BENCHMARK: couldn't convert opcode");
                    }
                }
            }
            None => (), // don't yield, we are trying to run the loop as fast as we can...
        }

        // actual benchmark
        // get a scalar message
        if false {
            // measured at 1479.2 iterations per second in this loop (hardware); 55/s (hosted)

            // xous v0.8
            // 29729 per 10s = 2972.9/s (hardware)
            // 485 per 10s = 48.5/s (hosted)
            count = benchmark_target::test_scalar(target_conn, count)
                .expect("BENCHMARK: couldn't send test message");
            check_count = check_count + 1;
        } else {
            if false {
                // works on hosted mode, 35/s (hosted)
                // measured at 762.6 iterations per second (hardware)

                // xous v0.8
                // 9,928 per 10s = 992.8/s (hardware)
                // 243 per 10s = 24.3/s (hosted)
                count = benchmark_target::test_memory(target_conn, count)
                    .expect("BENCHMARK: couldn't send test message");
                check_count = check_count + 1;
            } else {
                // simple send benchmark, instead of lend
                count = benchmark_target::test_memory_send(target_conn, count)
                    .expect("BENCHMARK: couldn't send test message");
                check_count = check_count + 1;
            }
        }

        if update_result {
            update_result = false;

            gfx.draw_rectangle(status_clipregion)
                .expect("unable to clear region");

            result_tv.clear_str();
            write!(
                &mut result_tv,
                "Elapsed: {}, count: {}, check: {}",
                stop_time - start_time,
                count,
                check_count
            )
            .unwrap();
            gfx.draw_textview(&mut result_tv).unwrap();
            gfx.flush().expect("unable to draw to screen");

            count = 0;
            check_count = 0;
        }
    }
}
