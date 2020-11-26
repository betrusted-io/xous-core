#![cfg_attr(baremetal, no_main)]
#![cfg_attr(baremetal, no_std)]

#[cfg(baremetal)]
#[macro_use]
mod debug;

mod logstr;
mod timer;
use core::fmt::Write;
use log::{error, info};
use xous::String;
use graphics_server::Point;
use graphics_server::GlyphSet;
use com::api::BattStats;
use com::*;

use core::convert::TryFrom;

// fn print_and_yield(index: *mut usize) -> ! {
//     let num = index as usize;
//     loop {
//         println!("THREAD {}", num);
//         xous::syscall::yield_slice();
//     }
// }

#[derive(Debug, Clone, Copy)]
pub struct Rectangle {
    /// Top left point of the rect
    pub top_left: Point,
    /// Bottom right point of the rect
    pub bottom_right: Point,
}

impl Rectangle {
    pub fn top_left(&self) -> Point {
        self.top_left
    }
    pub fn bottom_right(&self) -> Point {
        self.bottom_right
    }
    pub fn new(top_left: Point, bottom_right: Point) -> Self {
        Rectangle {
            top_left,
            bottom_right,
        }
    }
}

fn move_lfsr(mut lfsr: u32) -> u32 {
    lfsr ^= lfsr >> 7;
    lfsr ^= lfsr << 9;
    lfsr ^= lfsr >> 13;
    lfsr
}

pub struct Bounce {
    vector: Point,
    radius: u16,
    bounds: Rectangle,
    loc: Point,
    lfsr: u32,
}

impl Bounce {
    pub fn new(radius: u16, bounds: Rectangle) -> Bounce {
        Bounce {
            vector: Point::new(2,3),
            radius: radius,
            bounds: bounds,
            loc: Point::new((bounds.bottom_right.x - bounds.top_left.x)/2, (bounds.bottom_right.y - bounds.top_left.y)/2),
            lfsr: 0xace1u32,
        }
    }

    pub fn next_rand(&mut self) -> i16 {
        let mut ret = move_lfsr(self.lfsr);
        self.lfsr = ret;
        ret *= 2; // make the ball move faster

        (ret % 8) as i16
    }

    pub fn update(&mut self) -> &mut Self {
        let mut x: i16;
        let mut y: i16;
        // update the new ball location
        x = self.loc.x + self.vector.x; y = self.loc.y + self.vector.y;

        let r: i16 = self.radius as i16;
        if (x >= (self.bounds.bottom_right().x - r)) ||
           (x <= (self.bounds.top_left().x + r)) ||
           (y >= (self.bounds.bottom_right().y - r)) ||
           (y <= (self.bounds.top_left().y + r)) {
            if x >= (self.bounds.bottom_right().x - r - 1) {
                self.vector.x = -self.next_rand();
                x = self.bounds.bottom_right().x - r;
            }
            if x <= self.bounds.top_left().x + r + 1 {
                self.vector.x = self.next_rand();
                x = self.bounds.top_left().x + r;
            }
            if y >= (self.bounds.bottom_right().y - r - 1) {
                self.vector.y = -self.next_rand();
                y = self.bounds.bottom_right().y - r;
            }
            if y <= (self.bounds.top_left().y + r + 1) {
                self.vector.y = self.next_rand();
                y = self.bounds.top_left().y + r;
            }
        }

        self.loc.x = x;
        self.loc.y = y;

        self
    }
}

use core::sync::atomic::{AtomicU16, AtomicI16, AtomicU8, Ordering};

// need atomic global constants to pass data between threads
// as we do not yet have a "Mutex" in Xous
static BATT_STATS_VOLTAGE: AtomicU16 = AtomicU16::new(3700);
static BATT_STATS_CURRENT: AtomicI16 = AtomicI16::new(-150);
static BATT_STATS_SOC: AtomicU8 = AtomicU8::new(50);
static BATT_STATS_REMAINING: AtomicU16 = AtomicU16::new(750);

fn com_thread(_arg: Option<u32>) {
    let shell_server = xous::create_server(b"shell           ").expect("Couldn't create Shell server");
    info!("SHELL|com_thread: starting COM response handler thread");
    loop {
        let mut envelope = xous::syscall::receive_message(shell_server).expect("couldn't get address");
        info!("SHELL|com_thread: got message {:?}", envelope);
        if let Ok(opcode) = com::api::Opcode::try_from(&envelope.body) {
            match opcode {
                com::api::Opcode::BattStatsReturn(stats) => {
                    BATT_STATS_VOLTAGE.store(stats.voltage, Ordering::Relaxed);
                    BATT_STATS_CURRENT.store(stats.current, Ordering::Relaxed);
                    BATT_STATS_SOC.store(stats.soc, Ordering::Relaxed);
                    BATT_STATS_REMAINING.store(stats.remaining_capacity, Ordering::Relaxed);
                },
                _ => error!("shell received an opcode that wasn't expected")
            }
        } else {
            error!("couldn't convert opcode");
        }
    }
}

#[xous::xous_main]
fn shell_main() -> ! {
    timer::init();
    log_server::init_wait().unwrap();

    // let log_server_id = xous::SID::from_bytes(b"xous-logs-output").unwrap();
    let graphics_server_id = xous::SID::from_bytes(b"graphics-server ").unwrap();
    let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
    let log_server_id = xous::SID::from_bytes(b"xous-log-server ").unwrap();
    let com_id =        xous::SID::from_bytes(b"com             ").unwrap();

    println!("SHELL: Attempting to connect to servers...");
    let log_conn = xous::connect(log_server_id).unwrap();
    let graphics_conn = xous::connect(graphics_server_id).unwrap();
    let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();
    let com_conn = xous::connect(com_id).unwrap();

    println!(
        "SHELL: Connected to Log server: {}  Graphics server: {}  Ticktimer server: {} Com: {}",
        log_conn, graphics_conn, ticktimer_conn, com_conn,
    );

    assert_ne!(
        log_conn, graphics_conn,
        "SHELL: graphics and log connections are the same!"
    );

    assert_ne!(
        ticktimer_conn, graphics_conn,
        "SHELL: graphics and ticktimer connections are the same!"
    );

    // make a thread to catch responses from the COM
    xous::create_thread_simple(com_thread, None).unwrap();
    info!("SHELL: COM responder thread started");

    let screensize = graphics_server::screen_size(graphics_conn).expect("Couldn't get screen size");

    let dark = graphics_server::Color::from(0);
    let light = graphics_server::Color::from(!0);
    let mut bouncyball = Bounce::new(14,
        Rectangle::new(Point::new(0, 18 * 21),
        Point::new(screensize.x as _, screensize.y as i16 - 1)));
    bouncyball.update();

    let mut batt_stats: BattStats = BattStats::default();

    #[cfg(baremetal)]
    { // use this to select which UART to monitor in the main loop
        use utralib::generated::*;
        let gpio_base = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::gpio::HW_GPIO_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map GPIO CSR range");
        let mut gpio = CSR::new(gpio_base.as_mut_ptr() as *mut u32);
        gpio.wfo(utra::gpio::UARTSEL_UARTSEL, 1); // 0 = kernel, 1 = log, 2-3 are various servers
    }

    graphics_server::set_style(
        graphics_conn,
        1,
        dark,
        dark,
    )
    .expect("unable to draw to screen: {:?}");

    let mut last_time: u64 = 0;
    ticktimer_server::reset(ticktimer_conn).unwrap();
    let mut string_buffer = String::new(4096);
    loop {
        string_buffer.clear();
        write!(&mut string_buffer, "{}mV  Uptime: {:.2}s", BATT_STATS_VOLTAGE.load(Ordering::Relaxed), last_time as f32 / 1000f32).expect("Can't write");
        graphics_server::set_glyph(graphics_conn, GlyphSet::Small).expect("unable to set glyph");
        let (_, h) = graphics_server::query_glyph(graphics_conn).expect("unable to query glyph");
        graphics_server::clear_region(graphics_conn, 0, 0, screensize.x as usize - 1, h)
            .expect("unable to clear region");
        info!("drawing string: {}", string_buffer);
        graphics_server::draw_string(graphics_conn, &string_buffer).expect("unable to draw string");

        // ticktimer_server::sleep_ms(ticktimer_conn, 500).expect("couldn't sleep");

        // draw the ball
        bouncyball.update();
        graphics_server::clear_region(graphics_conn,
            bouncyball.bounds.top_left().x as _, bouncyball.bounds.top_left().y as _,
            bouncyball.bounds.bottom_right().x as _, bouncyball.bounds.bottom_right().y as usize + 1)
            .expect("unable to clear region");
        graphics_server::draw_circle(
            graphics_conn,
            bouncyball.loc,
            bouncyball.radius as u16,
        )
        .expect("unable to draw to screen");

        // Periodic tasks
        if let Ok(elapsed_time) = ticktimer_server::elapsed_ms(ticktimer_conn) {
            if elapsed_time - last_time > 500 {
                last_time = elapsed_time;
                info!("Requesting batt stats from COM");
                get_batt_stats_nb(com_conn).expect("Can't get battery stats from COM");
            }
        } else {
            error!("error requesting ticktimer!")
        }

        graphics_server::flush(graphics_conn).expect("unable to draw to screen");
    }
}
