use std::sync::{Arc, Mutex, mpsc};

use cramium_api::*;
use minifb::{Key, Window, WindowOptions};
use ux_api::minigfx::{ColorNative, FrameBuffer, Point};
use ux_api::platform::*;

pub const COLUMN: isize = WIDTH;
pub const ROW: isize = LINES;
pub const PAGE: u8 = ROW as u8 / 8;

const MAX_FPS: usize = 60;
const DARK_COLOUR: u32 = 0x161616;
const LIGHT_COLOUR: u32 = 0xC5C5BD;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MonoColor(ColorNative);
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mono {
    Black,
    White,
}
impl From<ColorNative> for Mono {
    fn from(value: ColorNative) -> Self {
        match value.0 {
            1 => Mono::Black,
            _ => Mono::White,
        }
    }
}
impl Into<ColorNative> for Mono {
    fn into(self) -> ColorNative {
        match self {
            Mono::Black => ColorNative::from(1),
            Mono::White => ColorNative::from(0),
        }
    }
}

/// The channel for the backend to communicate back to the main thread that it
/// has claimed.
pub struct MainThreadToken(mpsc::SyncSender<MinifbThread>);

/// A substitute for the native never type (`!`), which is still unstable on
/// `Fn` bounds.
pub enum Never {}

/// Claim the calling thread (which must be a main thread) for use by the
/// backend and call the specified closure on a new thread.
pub fn claim_main_thread(f: impl FnOnce(MainThreadToken) -> Never + Send + 'static) -> ! {
    // Some operating systems and GUI frameworks, in particular Cocoa, don't
    // allow creating an event loop from a thread other than the main thread
    // (TID 1) and will abort a program on violation (see issue #373), hence we
    // need to claim the main thread for use by the backend.
    let (send, recv) = mpsc::sync_channel(0);

    // Call the closure on a fake main thread
    let fake_main_thread = std::thread::Builder::new()
        .name("wrapped_main".into())
        .spawn(move || f(MainThreadToken(send)))
        .unwrap();

    // Process up to one request (that's the maximum because
    // `MainThreadToken: !Clone`)
    match recv.recv() {
        Ok(thread_params) => {
            // Run a GUI event loop. Abort if the fake main thread panics
            thread_params.run_while(|| !fake_main_thread.is_finished());
        }
        Err(mpsc::RecvError) => {}
    }

    // Join on the fake main thread
    match fake_main_thread.join() {
        Ok(x) => match x {},
        // The default panic handler should have already outputted the panic
        // message, so we can just call `abort` here
        Err(_) => std::process::abort(),
    }
}

/// Encapsulates the data passed to the thread handling minifb screen updates
/// and input events.
struct MinifbThread {
    native_buffer: Arc<Mutex<Vec<u32>>>,
}

struct XousKeyboardHandler {
    kbd: cramium_api::keyboard::Keyboard,
    left_shift: bool,
    right_shift: bool,
}

pub struct Oled128x128 {
    native_buffer: Arc<Mutex<Vec<u32>>>, //[u32; WIDTH * HEIGHT],
    // front and back buffers
    buffer: [u32; WIDTH as usize * HEIGHT as usize / (core::mem::size_of::<u32>() * 8)],
    stash: [u32; WIDTH as usize * HEIGHT as usize / (core::mem::size_of::<u32>() * 8)],
}

impl<'a> Oled128x128 {
    pub fn new<T>(
        main_thread_token: MainThreadToken,
        _perclk_freq: u32,
        _iox: &'a T,
        _udma_global: &'a dyn UdmaGlobalConfig,
    ) -> Self {
        let native_buffer = vec![DARK_COLOUR; COLUMN as usize * ROW as usize];
        let native_buffer = Arc::new(Mutex::new(native_buffer));

        // Start a GUI event loop on the main thread
        let thread_params = MinifbThread { native_buffer: Arc::clone(&native_buffer) };
        main_thread_token.0.send(thread_params).unwrap();

        Self {
            native_buffer,
            buffer: [0u32; WIDTH as usize * HEIGHT as usize / (core::mem::size_of::<u32>() * 8)],
            stash: [0u32; WIDTH as usize * HEIGHT as usize / (core::mem::size_of::<u32>() * 8)],
        }
    }

    pub fn buffer_mut(&mut self) -> &mut ux_api::platform::FbRaw { &mut self.buffer }

    pub fn buffer(&self) -> &ux_api::platform::FbRaw { &self.buffer }

    pub fn screen_size(&self) -> Point { Point::new(WIDTH, LINES) }

    pub fn redraw(&mut self) { self.draw(); }

    pub fn blit_screen(&mut self, bmp: &[u32]) { self.buffer.copy_from_slice(bmp); }

    pub fn set_devboot(&mut self, _ena: bool) {
        unimplemented!("devboot feature does not exist on this platform");
    }

    pub fn stash(&mut self) { self.stash.copy_from_slice(&self.buffer); }

    pub fn pop(&mut self) {
        self.buffer.copy_from_slice(&self.stash);
        self.redraw();
    }

    pub fn send_command<'b, U>(&'b mut self, _cmd: U)
    where
        U: IntoIterator<Item = u8> + 'b,
    {
    }

    pub fn init(&mut self) {}
}

impl FrameBuffer for Oled128x128 {
    fn draw(&mut self) {
        for (index, pixel) in self.native_buffer.lock().unwrap().iter_mut().enumerate() {
            *pixel =
                if (self.buffer[index / 32] & (1 << (index % 32))) != 0 { DARK_COLOUR } else { LIGHT_COLOUR }
        }
    }

    fn clear(&mut self) { self.buffer_mut().fill(0xFFFF_FFFF); }

    fn put_pixel(&mut self, p: Point, on: ColorNative) {
        if p.x >= COLUMN || p.y >= ROW || p.x < 0 || p.y < 0 {
            return;
        }
        let bitnum = (p.x + p.y * COLUMN) as usize;
        if on.0 != 0 {
            self.buffer[bitnum / 32] |= 1 << (bitnum % 32);
        } else {
            self.buffer[bitnum / 32] &= !(1 << (bitnum % 32));
        }
    }

    fn dimensions(&self) -> Point { Point::new(COLUMN, ROW) }

    fn get_pixel(&self, p: Point) -> Option<ColorNative> {
        if p.x >= COLUMN || p.y >= ROW || p.x < 0 || p.y < 0 {
            return None;
        }
        let bitnum = (p.x + p.y * COLUMN) as usize;
        if self.buffer[bitnum / 32] & 1 << (bitnum % 32) != 0 { Some(1.into()) } else { Some(0.into()) }
    }

    fn xor_pixel(&mut self, p: Point) {
        if let Some(px) = self.get_pixel(p) {
            let mono_px: Mono = px.into();
            self.put_pixel(
                p,
                match mono_px {
                    Mono::Black => Mono::White,
                    Mono::White => Mono::Black,
                }
                .into(),
            );
        }
    }

    /// In this architecture, it's actually totally safe to do this, but the trait
    /// is marked unsafe because in some other displays it may require some tomfoolery
    /// to get reference types to match up.
    unsafe fn raw_mut(&mut self) -> &mut ux_api::platform::FbRaw { self.buffer_mut() }
}

impl MinifbThread {
    pub fn run_while(self, mut predicate: impl FnMut() -> bool) {
        let mut window = Window::new(
            "baosec",
            COLUMN as usize,
            ROW as usize,
            WindowOptions {
                scale_mode: minifb::ScaleMode::AspectRatioStretch,
                resize: true,
                ..WindowOptions::default()
            },
        )
        .unwrap_or_else(|e| {
            log::error!("{e:?}");
            std::process::abort();
        });

        // Limit the maximum update rate
        window.set_target_fps(MAX_FPS);

        let xns = xous_names::XousNames::new().unwrap();
        let kbd = cramium_api::keyboard::Keyboard::new(&xns).unwrap();
        let keyboard_handler = Box::new(XousKeyboardHandler { kbd, left_shift: false, right_shift: false });
        window.set_input_callback(keyboard_handler);

        let mut native_buffer = Vec::new();

        while predicate() {
            // Copy the contents of `native_buffer`. Release the lock
            // immediately so as not to starve the server thread.
            native_buffer.clear();
            native_buffer.extend_from_slice(&self.native_buffer.lock().unwrap());

            // Render the contents of the minifb window and handle input events.
            // This may block to regulate the update rate.
            window.update_with_buffer(&native_buffer, COLUMN as usize, ROW as usize).unwrap();
            if !window.is_open() || window.is_key_down(Key::Escape) {
                std::process::exit(0);
            }
        }
    }
}

impl XousKeyboardHandler {
    fn decode_key(&mut self, k: Key) -> char {
        let shift = self.left_shift || self.right_shift;
        let base: char = if shift == false {
            match k {
                // key maps are commented out so we can use the add_char routine for all the characters
                // natively handled by mini_fb this allows us to apply the native keyboard map
                // to all the typed characters, while still passing through the special
                // keys needed to emulate the special buttons on the device.
                /* Key::A => 'a',
                Key::B => 'b',
                Key::C => 'c',
                Key::D => 'd',
                Key::E => 'e',
                Key::F => 'f',
                Key::G => 'g',
                Key::H => 'h',
                Key::I => 'i',
                Key::J => 'j',
                Key::K => 'k',
                Key::L => 'l',
                Key::M => 'm',
                Key::N => 'n',
                Key::O => 'o',
                Key::P => 'p',
                Key::Q => 'q',
                Key::R => 'r',
                Key::S => 's',
                Key::T => 't',
                Key::U => 'u',
                Key::V => 'v',
                Key::W => 'w',
                Key::X => 'x',
                Key::Y => 'y',
                Key::Z => 'z',
                Key::Key0 => '0',
                Key::Key1 => '1',
                Key::Key2 => '2',
                Key::Key3 => '3',
                Key::Key4 => '4',
                Key::Key5 => '5',
                Key::Key6 => '6',
                Key::Key7 => '7',
                Key::Key8 => '8',
                Key::Key9 => '9',*/
                Key::Left => '←',
                Key::Right => '→',
                Key::Up => '↑',
                Key::Down => '↓',
                Key::Home => '∴',
                Key::Backspace => '\u{0008}',
                Key::Delete => '\u{0008}',
                Key::Enter => 0xd_u8.into(),
                //Key::Space => ' ',
                //Key::Comma => ',',
                //Key::Period => '.',
                Key::F1 => 0x11_u8.into(),
                Key::F2 => 0x12_u8.into(),
                Key::F3 => 0x13_u8.into(),
                Key::F4 => 0x14_u8.into(),
                Key::F5 => '😊',
                Key::F6 => '福',
                _ => '\u{0000}',
            }
        } else {
            match k {
                /* Key::A => 'A',
                Key::B => 'B',
                Key::C => 'C',
                Key::D => 'D',
                Key::E => 'E',
                Key::F => 'F',
                Key::G => 'G',
                Key::H => 'H',
                Key::I => 'I',
                Key::J => 'J',
                Key::K => 'K',
                Key::L => 'L',
                Key::M => 'M',
                Key::N => 'N',
                Key::O => 'O',
                Key::P => 'P',
                Key::Q => 'Q',
                Key::R => 'R',
                Key::S => 'S',
                Key::T => 'T',
                Key::U => 'U',
                Key::V => 'V',
                Key::W => 'W',
                Key::X => 'X',
                Key::Y => 'Y',
                Key::Z => 'Z',
                Key::Key0 => ')',
                Key::Key1 => '!',
                Key::Key2 => '@',
                Key::Key3 => '#',
                Key::Key4 => '$',
                Key::Key5 => '%',
                Key::Key6 => '^',
                Key::Key7 => '&',
                Key::Key8 => '*',
                Key::Key9 => '(', */
                Key::Left => '←',
                Key::Right => '→',
                Key::Up => '↑',
                Key::Down => '↓',
                Key::Home => '∴',
                Key::Backspace => '\u{0008}',
                Key::Delete => '\u{0008}',
                //Key::Space => ' ',
                //Key::Comma => '<',
                //Key::Period => '>',
                _ => '\u{0000}',
            }
        };
        base
    }
}

impl minifb::InputCallback for XousKeyboardHandler {
    fn add_char(&mut self, uni_char: u32) {
        let c = char::from_u32(uni_char).unwrap_or('\u{0000}');
        if c != '\u{0008}' && c != '\u{000d}' && c != '\u{007f}' {
            self.kbd.inject_key(c);
        }
    }

    fn set_key_state(&mut self, key: minifb::Key, state: bool) {
        if key == Key::LeftShift {
            self.left_shift = state;
            return;
        }
        if key == Key::RightShift {
            self.right_shift = state;
            return;
        }
        if !state {
            return;
        }

        log::debug!("GFX|hosted: sending key {:?}", key);
        let c = self.decode_key(key);
        if c != '\u{0000}' {
            self.kbd.inject_key(c);
        }
    }
}
