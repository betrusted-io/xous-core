#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use blitstr_ref as blitstr;
use blitstr::{Cursor, GlyphStyle};
use core::fmt::Write;
use graphics_server::{DrawStyle, PixelColor, Point, Rectangle};

use log::{error, info};
use xous::{String, Message, ScalarMessage};

use core::convert::TryFrom;

pub enum Opcode {
    Start,
    Stop,
}

impl core::convert::TryFrom<& Message> for Opcode {
    type Error = &'static str;
    fn try_from(message: & Message) -> Result<Self, Self::Error> {
        match message {
            Message::Scalar(m) => match m.id {
                0 => Ok(Opcode::Start),
                1 => Ok(Opcode::Stop),
                _ => Err("BENCHMARK api: unknown Scalar ID"),
            },
            _ => Err("BENCHMARK api: unhandled message type"),
        }
    }
}

impl Into<Message> for Opcode {
    fn into(self) -> Message {
        match self {
            Opcode::Start => Message::Scalar(ScalarMessage {
                id: 0,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::Stop => Message::Scalar(ScalarMessage {
                id: 1,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
        }
    }
}

fn stopwatch_thread(_arg: xous::SID) {
    info!("BENCHMARK|stopwatch: starting");

    let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
    let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();

    let shell_conn = xous_names::request_connection_blocking(xous::names::SERVER_NAME_SHELL).expect("BENCHMARK|stopwatch: can't connect to main program");
    let mut last_time: u64 = ticktimer_server::elapsed_ms(ticktimer_conn).unwrap();
    let mut start_sent = false;
    loop {
        if let Ok(elapsed_time) = ticktimer_server::elapsed_ms(ticktimer_conn) {
            if elapsed_time - last_time > 500 && !start_sent {
                last_time = elapsed_time;
                xous::send_message(shell_conn, Opcode::Start.into()).expect("BENCHMARK|stopwatch: couldn't send Start message");
                start_sent = true;
            } else if elapsed_time - last_time > 10_000 && start_sent {
                last_time = elapsed_time;
                start_sent = false;
                xous::send_message(shell_conn, Opcode::Stop.into()).expect("BENCHMARK|stopwatch: couldn't send Stop message");
            }
        } else {
            error!("error requesting ticktimer!")
        }
        if false {
            // send a start loop message
            xous::send_message(shell_conn, Opcode::Start.into()).expect("BENCHMARK|stopwatch: couldn't send Start message");
            ticktimer_server::sleep_ms(ticktimer_conn, 10_000).expect("couldn't sleep");
            // send a stop loop message
            xous::send_message(shell_conn, Opcode::Stop.into()).expect("BENCHMARK|stopwatch: couldn't send Stop message");
            // give a moment for the result to update
            ticktimer_server::sleep_ms(ticktimer_conn, 500).expect("couldn't sleep");
        }
        xous::yield_slice();
    }
}

#[xous::xous_main]
fn shell_main() -> ! {
    log_server::init_wait().unwrap();

    info!("BENCHMARK: ticktimer");
    let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
    let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();

    let shell_server = xous_names::register_name(xous::names::SERVER_NAME_SHELL).expect("BENCHMARK: can't register server");

    let graphics_conn = xous_names::request_connection_blocking(xous::names::SERVER_NAME_GFX).expect("BENCHMARK: can't connect to COM");
    let target_conn = xous_names::request_connection_blocking(xous::names::SERVER_NAME_BENCHMARK).expect("BENCHMARK: can't connect to COM");

    xous::create_thread_simple(stopwatch_thread, shell_server).unwrap();
    info!("BENCHMARK: stopwatch thread started");

    let screensize = graphics_server::screen_size(graphics_conn).expect("Couldn't get screen size");

    let mut string_buffer = String::new(4096);

    graphics_server::set_glyph_style(graphics_conn, GlyphStyle::Small)
        .expect("unable to set glyph");
    let (_, font_h) = graphics_server::query_glyph(graphics_conn).expect("unable to query glyph");
    let status_clipregion =
        Rectangle::new_coords_with_style(4, 0, screensize.x, font_h as i16 * 4, DrawStyle::new(PixelColor::Light, PixelColor::Light, 1));

    graphics_server::draw_rectangle(graphics_conn, status_clipregion)
        .expect("unable to clear region");

    string_buffer.clear();
    write!(&mut string_buffer, "First pass, please wait...");
    let status_cursor = Cursor::from_top_left_of(status_clipregion.into());
    graphics_server::set_cursor(graphics_conn, status_cursor).expect("can't set cursor");
    graphics_server::draw_string(graphics_conn, &string_buffer).expect("unable to draw string");
    graphics_server::flush(graphics_conn).expect("unable to draw to screen");

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
                if let Ok(opcode) = Opcode::try_from(&envelope.body) {
                    match opcode {
                        Opcode::Start => {
                            start_time = ticktimer_server::elapsed_ms(ticktimer_conn).unwrap();
                        },
                        Opcode::Stop => {
                            stop_time = ticktimer_server::elapsed_ms(ticktimer_conn).unwrap();
                            update_result = true;
                        },
                    }
                } else {
                    error!("BENCHMARK: couldn't convert opcode");
                }
            }
            None => (), // don't yield, we are trying to run the loop as fast as we can...
        }

        // actual benchmark
        // get a scalar message
        if true {
            // measured at 1349 iterations per second in this loop
            count = benchmark_target::test_scalar(target_conn, count).expect("BENCHMARK: couldn't send test message");
            check_count = check_count + 1;
        } else {
            count = benchmark_target::test_memory(target_conn, count).expect("BENCHMARK: couldn't send test message");
            check_count = check_count + 1;
        }

        if update_result {
            update_result = false;

            graphics_server::draw_rectangle(graphics_conn, status_clipregion)
            .expect("unable to clear region");

            string_buffer.clear();
            write!(&mut string_buffer, "Elapsed: {}, count: {}, check: {}",
                stop_time - start_time, count, check_count).unwrap();
            let status_cursor = Cursor::from_top_left_of(status_clipregion.into());
            graphics_server::set_cursor(graphics_conn, status_cursor).expect("can't set cursor");
            graphics_server::draw_string(graphics_conn, &string_buffer).expect("unable to draw string");
            graphics_server::flush(graphics_conn).expect("unable to draw to screen");

            count = 0;
            check_count = 0;
        }
    }
}
