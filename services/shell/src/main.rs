#![cfg_attr(baremetal, no_main)]
#![cfg_attr(baremetal, no_std)]

use blitstr_ref as blitstr;
use blitstr::{Cursor, GlyphStyle};
use com::*;
use core::fmt::Write;
use graphics_server::{Circle, DrawStyle, Line, PixelColor, Point, Rectangle};

use log::{error, info};
use xous::String;

use core::convert::TryFrom;

pub struct Bounce {
    vector: Point,
    radius: u16,
    bounds: Rectangle,
    loc: Point,
}

impl Bounce {
    pub fn new(radius: u16, bounds: Rectangle) -> Bounce {
        Bounce {
            vector: Point::new(2, 3),
            radius: radius,
            bounds: bounds,
            loc: Point::new(
                (bounds.br.x - bounds.tl.x) / 2,
                (bounds.br.y - bounds.tl.y) / 2,
            ),
        }
    }

    pub fn ball_center(&self) -> Point {
        self.loc
    }
    pub fn radius(&self) -> u16 {
        self.radius
    }
    pub fn bounds(&self) -> Rectangle {
        self.bounds
    }

    pub fn next_rand(&mut self, trng_conn: xous::CID) -> i16 {
        let ret = trng::get_u32(trng_conn).expect("SHELL: can't get TRNG") * 3;

        (ret % 12) as i16
    }

    pub fn update(&mut self, trng_conn: xous::CID) -> &mut Self {
        let mut x: i16;
        let mut y: i16;
        // update the new ball location
        x = self.loc.x + self.vector.x;
        y = self.loc.y + self.vector.y;

        let r: i16 = self.radius as i16;
        if (x >= (self.bounds.br.x - r))
            || (x <= (self.bounds.tl.x + r))
            || (y >= (self.bounds.br.y - r))
            || (y <= (self.bounds.tl.y + r))
        {
            if x >= (self.bounds.br.x - r - 1) {
                self.vector.x = -self.next_rand(trng_conn);
                x = self.bounds.br.x - r;
            }
            if x <= self.bounds.tl.x + r + 1 {
                self.vector.x = self.next_rand(trng_conn);
                x = self.bounds.tl.x + r;
            }
            if y >= (self.bounds.br.y - r - 1) {
                self.vector.y = -self.next_rand(trng_conn);
                y = self.bounds.br.y - r;
            }
            if y <= (self.bounds.tl.y + r + 1) {
                self.vector.y = self.next_rand(trng_conn);
                y = self.bounds.tl.y + r;
            }
        }

        self.loc.x = x;
        self.loc.y = y;

        self
    }
}

use core::sync::atomic::{AtomicI16, AtomicU16, AtomicU8, Ordering, AtomicU32, AtomicBool};
use heapless::Vec;
use heapless::consts::U64;

// need atomic global constants to pass data between threads
// as we do not yet have a "Mutex" in Xous
static BATT_STATS_VOLTAGE: AtomicU16 = AtomicU16::new(3700);
static BATT_STATS_CURRENT: AtomicI16 = AtomicI16::new(-150);
static BATT_STATS_SOC: AtomicU8 = AtomicU8::new(50);
static BATT_STATS_REMAINING: AtomicU16 = AtomicU16::new(750);
static INCOMING_CHAR: AtomicU32 = AtomicU32::new(0);
static INCOMING_FRESH: AtomicBool = AtomicBool::new(false);

fn event_thread(_arg: usize) {
    info!("SHELL|event_thread: registering shell SID");
    let xns = xous_names::XousNames::new().unwrap();
    let shell_server = xns.register_name(xous::names::SERVER_NAME_SHELL).expect("SHELL: can't register server");

    let kbd_conn = xns.request_connection_blocking(xous::names::SERVER_NAME_KBD).expect("SHELL|event_thread: can't connect to KBD");
    keyboard::request_events(xous::names::SERVER_NAME_SHELL, kbd_conn).expect("SHELL|event_thread: couldn't request events from keyboard");

    let com_conn = xns.request_connection_blocking(xous::names::SERVER_NAME_COM).expect("SHELL|event_thread: can't connect to COM");
    com::request_battstat_events(xous::names::SERVER_NAME_SHELL, com_conn).expect("SHELL|event_thread: couldn't request events from COM");

    info!("SHELL|event_thread: starting COM response handler thread");
    let mut key_queue: Vec<char, U64> = Vec::new();
    loop {
        if key_queue.len() > 0 {
            for i in 0..key_queue.len() {
                INCOMING_CHAR.store(key_queue[i] as u32, Ordering::Relaxed);
                INCOMING_FRESH.store(true, Ordering::Relaxed);
                while INCOMING_FRESH.load(Ordering::Relaxed) {
                    xous::yield_slice();
                }
            }
            key_queue.clear();
        }
        let envelope = xous::syscall::receive_message(shell_server).expect("couldn't get address");
        // info!("SHELL|event_thread: got message {:?}", envelope);
        if let Ok(opcode) = com::api::Opcode::try_from(&envelope.body) {
            match opcode {
                com::api::Opcode::BattStatsEvent(stats) => {
                    BATT_STATS_VOLTAGE.store(stats.voltage, Ordering::Relaxed);
                    BATT_STATS_CURRENT.store(stats.current, Ordering::Relaxed);
                    BATT_STATS_SOC.store(stats.soc, Ordering::Relaxed);
                    BATT_STATS_REMAINING.store(stats.remaining_capacity, Ordering::Relaxed);
                },
                _ => error!("shell received COM event opcode that wasn't expected"),
            }
        } else if let Ok(opcode) = keyboard::api::Opcode::try_from(&envelope.body) {
            match opcode {
                keyboard::api::Opcode::KeyboardEvent(keys) => {
                    for &k in keys.iter() {
                        if k != '\u{0000}' {
                            key_queue.push(k).unwrap();
                            // info!("SHELL:event_thread: got key '{}'", k);
                        }
                    }
                },
                _ => error!("shell received KBD event opcode that wasn't expected"),
            }
        } else {
            error!("couldn't convert opcode");
        }
    }
}

#[xous::xous_main]
fn shell_main() -> ! {
    log_server::init_wait().unwrap();
    info!("SHELL: my PID is {}", xous::process::id());

    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");

    let graphics_conn = xns.request_connection_blocking(xous::names::SERVER_NAME_GFX).expect("SHELL: can't connect to COM");

    info!(
        "SHELL: Connected to Graphics server: {}  Ticktimer server: {}",
        graphics_conn, ticktimer_conn,
    );

    let com_conn = xns.request_connection_blocking(xous::names::SERVER_NAME_COM).expect("SHELL: can't connect to COM");
    info!("SHELL: connected to COM: {:?}", com_conn);

    let trng_conn = xns.request_connection_blocking(xous::names::SERVER_NAME_TRNG).expect("SHELL: can't connect to TRNG");

    // make a thread to catch responses from the COM
    xous::create_thread_simple(event_thread, 0).unwrap();
    info!("SHELL: COM responder thread started");
    let start_time: u64 = ticktimer.elapsed_ms();
    while ticktimer.elapsed_ms() - start_time < 1000 {
        xous::yield_slice();
    }

    let screensize = graphics_server::screen_size(graphics_conn).expect("Couldn't get screen size");

    let mut bouncyball = Bounce::new(
        14,
        Rectangle::new(
            Point::new(0, 18 * 21),
            Point::new(screensize.x as _, screensize.y as i16 - 1),
        ),
    );
    bouncyball.update(trng_conn);

    let style_dark = DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1);
    let style_light = DrawStyle::new(PixelColor::Light, PixelColor::Light, 1);

    let mut string_buffer = String::new();
    let mut input_buf = String::new();
    graphics_server::set_glyph_style(graphics_conn, GlyphStyle::Small)
        .expect("unable to set glyph");
    let (_, font_h) = graphics_server::query_glyph(graphics_conn).expect("unable to query glyph");
    let status_clipregion =
        Rectangle::new_coords_with_style(4, 0, screensize.x, font_h as i16 * 2, style_light);
    let mut status_cursor;
    let small_font_h = font_h;

    graphics_server::set_glyph_style(graphics_conn, GlyphStyle::Regular)
        .expect("unable to set glyph");
    let (_, font_h) = graphics_server::query_glyph(graphics_conn).expect("unable to query glyph");
    let mut work_clipregion = Rectangle::new_coords_with_style(
        4,
        small_font_h as i16 * 2,
        screensize.x,
        font_h as i16 * 8 + 18,
        style_light,
    );
    let mut work_cursor: Cursor;
    graphics_server::draw_rectangle(graphics_conn, work_clipregion)
        .expect("unable to clear region");

    let mut last_time: u64 = ticktimer.elapsed_ms();
    let mut first_time = true;
    loop {
        //////////////// status bar
        graphics_server::set_glyph_style(graphics_conn, GlyphStyle::Small)
            .expect("unable to set glyph");

        graphics_server::draw_rectangle(graphics_conn, status_clipregion)
            .expect("unable to clear region");
        graphics_server::set_string_clipping(graphics_conn, status_clipregion.into())
            .expect("unable to set string clip region");
        string_buffer.clear();
        write!(
            &mut string_buffer,
            "{}mV",
            BATT_STATS_VOLTAGE.load(Ordering::Relaxed)
        )
        .expect("Can't write");
        status_cursor = Cursor::from_top_left_of(status_clipregion.into());
        graphics_server::set_cursor(graphics_conn, status_cursor).expect("can't set cursor");
        //info!("SHELL: debug0");
        graphics_server::draw_string(graphics_conn, &string_buffer).expect("unable to draw string");
        //info!("SHELL: debug1");
        status_cursor.pt.x = 95;
        string_buffer.clear();
        write!(
            &mut string_buffer,
            "{}mA",
            BATT_STATS_CURRENT.load(Ordering::Relaxed)
        )
        .expect("Can't write");
        graphics_server::set_cursor(graphics_conn, status_cursor).expect("can't set cursor");
        //info!("SHELL: debug2");
        graphics_server::draw_string(graphics_conn, &string_buffer).expect("unable to draw string");
        //info!("SHELL: debug3");
        status_cursor.pt.x = 190;
        string_buffer.clear();
        write!(
            &mut string_buffer,
            "{}mA",
            BATT_STATS_REMAINING.load(Ordering::Relaxed)
        )
        .expect("Can't write");
        graphics_server::set_cursor(graphics_conn, status_cursor).expect("can't set cursor");
        graphics_server::draw_string(graphics_conn, &string_buffer).expect("unable to draw string");
        status_cursor.pt.x = 280;
        string_buffer.clear();
        write!(
            &mut string_buffer,
            "{}%",
            BATT_STATS_SOC.load(Ordering::Relaxed)
        )
        .expect("Can't write");
        graphics_server::set_cursor(graphics_conn, status_cursor).expect("can't set cursor");
        graphics_server::draw_string(graphics_conn, &string_buffer).expect("unable to draw string");

        //////////////// uptime
        string_buffer.clear();
        write!(
            &mut string_buffer,
            "Uptime: {:.2}s\n\n",
            last_time as f32 / 1000f32
        )
        .expect("Can't write");
        status_cursor.pt.x = 4; status_cursor.pt.y = small_font_h as u32;
        graphics_server::draw_string(graphics_conn, &string_buffer).expect("unable to draw string");

        // a line under the status area
        graphics_server::draw_line(
            graphics_conn,
            Line::new_with_style(
                Point::new(0, status_clipregion.br.y + 2),
                Point::new(screensize.x as _, status_clipregion.br.y + 2),
                style_dark,
            ),
        )
        .expect("can't draw line");



        //////////////// work area
        if INCOMING_FRESH.load(Ordering::Relaxed) {
            INCOMING_FRESH.store(false, Ordering::Relaxed);
            if INCOMING_CHAR.load(Ordering::Relaxed) == 0x14 {
                power_off_soc(com_conn).expect("SHELL: can't power down");
                #[cfg(baremetal)]
                {
                    use utralib::generated::*;
                    let power_base = xous::syscall::map_memory(
                        xous::MemoryAddress::new(utra::power::HW_POWER_BASE),
                        None,
                        4096,
                        xous::MemoryFlags::R | xous::MemoryFlags::W,
                    )
                    .expect("couldn't map POWER CSR range");
                    let mut power = CSR::new(power_base.as_mut_ptr() as *mut u32);
                    power.wo(utra::power::POWER, 0);
                }
            }
            write!(&mut input_buf, "{}", core::char::from_u32(INCOMING_CHAR.load(Ordering::Relaxed)).unwrap()).expect("unable to copy to Xous string");

            graphics_server::set_glyph_style(graphics_conn, GlyphStyle::Regular)
            .expect("unable to set glyph");

            // define the text area
            work_clipregion.tl = Point::new(4, font_h as i16 * 2);
            work_clipregion.br = Point::new(screensize.x, bouncyball.bounds.tl.y);
            work_cursor = Cursor::from_top_left_of(work_clipregion.into());

            // clear the text area, set string clipping and cursor
            if first_time {
                info!("SHELL: first time clear of work area");
                graphics_server::draw_rectangle(graphics_conn, work_clipregion)
                   .expect("unable to clear region");
                first_time = false;
            }
            graphics_server::set_string_clipping(graphics_conn, work_clipregion.into())
                .expect("unable to set string clip region");
            graphics_server::set_cursor(graphics_conn, work_cursor).expect("can't set cursor");

            // info!("SHELL: attempting to render {}", input_buf);
            graphics_server::draw_string(graphics_conn, &input_buf).expect("unable to draw string");
        }


        //////////////// draw the ball
        graphics_server::draw_rectangle(
            graphics_conn,
            Rectangle::new_with_style(
                Point::new(
                    bouncyball.ball_center().x - bouncyball.radius() as i16 - 1,
                    bouncyball.ball_center().y - bouncyball.radius() as i16 - 1,
                ),
                Point::new(
                    bouncyball.ball_center().x + bouncyball.radius() as i16 + 1,
                    bouncyball.ball_center().y + bouncyball.radius() as i16 + 1,
                ),
                style_light,
            ),
        )
        .expect("unable to clear ball region");
        bouncyball.update(trng_conn);

        // draw the top line that contains the ball
        graphics_server::draw_line(
            graphics_conn,
            Line::new_with_style(
                Point::new(0, bouncyball.bounds.tl.y - 1),
                Point::new(screensize.x, bouncyball.bounds.tl.y - 1),
                style_dark,
            ),
        )
        .expect("can't draw border");
        // draw the ball
        graphics_server::draw_circle(
            graphics_conn,
            Circle::new_with_style(bouncyball.loc, bouncyball.radius as i16, style_dark),
        )
        .expect("unable to draw to screen");

        // Periodic tasks
        if let Ok(elapsed_time) = ticktimer.elapsed_ms() {
            if elapsed_time - last_time > 500 {
                last_time = elapsed_time;
                get_batt_stats_nb(com_conn).expect("Can't get battery stats from COM");
            }
        } else {
            error!("error requesting ticktimer!")
        }

        graphics_server::flush(graphics_conn).expect("unable to draw to screen");

        // rate limit graphics
        //ticktimer.sleep_ms(ticktimer_conn, 500).expect("couldn't sleep");
    }
}
