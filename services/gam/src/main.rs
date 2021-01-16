#![cfg_attr(baremetal, no_main)]
#![cfg_attr(baremetal, no_std)]

mod api;
use api::*;
mod status;
use status::*;
mod canvas;
use canvas::*;

use blitstr::{Cursor, GlyphStyle};
use com::*;
use core::fmt::Write;
use graphics_server::{Circle, DrawStyle, Line, PixelColor, Point, Rectangle};

use log::{error, info};
use xous::String;

use core::convert::TryFrom;

use heapless::binary_heap::{BinaryHeap, Max};
use heapless::FnvIndexMap;
use heapless::consts::*;

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();

    info!("GAM: starting up...");
    let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
    let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();

    let gfx_conn = xous_names::request_connection_blocking(xous::names::SERVER_NAME_GFX).expect("GAM: can't connect to COM");
    let trng_conn = xous_names::request_connection_blocking(xous::names::SERVER_NAME_TRNG).expect("GAM: can't connect to TRNG");

    let screensize = graphics_server::screen_size(gfx_conn).expect("GAM: Couldn't get screen size");

    graphics_server::set_glyph_style(gfx_conn, GlyphStyle::Small).expect("GAM: couldn't set glyph style");
    let (_, small_height_tuple) = graphics_server::query_glyph(gfx_conn).expect("GAM: couldn't get glyph height");
    let small_height: i16 = small_height_tuple as i16;
    graphics_server::set_glyph_style(gfx_conn, GlyphStyle::Regular).expect("GAM: couldn't set glyph style");
    let (_, regular_height_tuple) = graphics_server::query_glyph(gfx_conn).expect("GAM: couldn't get glyph height");
    let regular_height: i16 = regular_height_tuple as i16;

    let mut canvases: BinaryHeap<Canvas, U32, Max> = BinaryHeap::new();
    let mut status_canvas = Canvas::new(
        Rectangle::new_coords(0, 0, screensize.x, small_height),
        0, trng_conn, None
    ).expect("GAM: couldn't create status canvas");
    canvases.push(status_canvas).expect("GAM: can't store status canvus");

    let mut password_canvas = Canvas::new(
        Rectangle::new_coords(-1, -1, -1, -1),
        0, trng_conn, None
    ).expect("GAM: couldn't create password canvas");
    canvases.push(password_canvas).expect("GAM: can't store password canvas");

    let mut predictive_canvas = Canvas::new(
        Rectangle::new_coords(0, screensize.y - regular_height, screensize.x, screensize.y),
        1,
        trng_conn, None
    ).expect("GAM: couldn't create predictive text canvas");
    canvases.push(predictive_canvas).expect("GAM: couldn't store predictive canvas");

    let mut input_canvas = Canvas::new(
        Rectangle::new_v_stack(predictive_canvas.clip_rect(), -regular_height),
        1, trng_conn, None
    ).expect("GAM: couldn't create input text canvas");
    canvases.push(input_canvas).expect("GAM: couldn't store input canvas");

    let mut content_canvas = Canvas::new(
        Rectangle::new_v_span(status_canvas.clip_rect(), input_canvas.clip_rect()),
        2, trng_conn, None
    ).expect("GAM: couldn't create content canvas");
    canvases.push(content_canvas).expect("GAM: can't store content canvas");

    let mut canvas_map: FnvIndexMap<[u32;4], Canvas, U32>;
    // this is broken into two steps because of https://github.com/rust-lang/rust/issues/71126
    let (c_tuple, m_tuple) = recompute_canvases(canvases, Rectangle::new(Point::new(0, 0), screensize));
    canvases = c_tuple; canvas_map = m_tuple;

    // make a thread to manage the status bar
    xous::create_thread_simple(status_thread, status_canvas.gid()).expect("GAM: couldn't create status thread");

    let gam_sid = xous_names::register_name(xous::names::SERVER_NAME_GAM).expect("GAM: can't register server");

    info!("GAM: entering main loop");
    loop {
        let maybe_env = xous::try_receive_message(gam_sid).unwrap();
        match maybe_env {
            Some(envelope) => {
                info!("GAM: Message: {:?}", envelope);
                if let Ok(opcode) = Opcode::try_from(&envelope.body) {
                    match opcode {
                        Opcode::ClearCanvas(gid) => {
                            match canvas_map.get(&gid) {
                                Some(c) => {
                                    let mut rect = c.clip_rect();
                                    rect.style = DrawStyle {
                                        fill_color: Some(PixelColor::Light),
                                        stroke_color: Some(PixelColor::Light),
                                        stroke_width: 1,
                                    };
                                    graphics_server::draw_rectangle(gfx_conn, rect).expect("GAM: can't clear canvas");
                                },
                                None => info!("GAM: attempt to clear bogus canvas, ignored."),
                            }
                        },
                        _ => todo!("GAM: opcode not yet implemented"),
                    }
                }
            }
            _ => xous::yield_slice(),
        }

        graphics_server::flush(gfx_conn).expect("GAM: couldn't flush buffer to screen");
    }
}
