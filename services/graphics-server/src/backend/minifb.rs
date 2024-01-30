#![cfg_attr(not(target_os = "none"), allow(dead_code))]

use std::sync::{mpsc, Arc, Mutex};

use minifb::{Key, Window, WindowOptions};

use crate::api::Point;
use crate::api::{LINES, WIDTH};

const HEIGHT: i16 = LINES;

/// Width of the screen in 32-bit words
const WIDTH_WORDS: usize = 11;
pub const FB_WIDTH_WORDS: usize = WIDTH_WORDS;
pub const FB_WIDTH_PIXELS: usize = WIDTH as usize;
pub const FB_LINES: usize = HEIGHT as usize;
pub const FB_SIZE: usize = WIDTH_WORDS * HEIGHT as usize; // 44 bytes by 536 lines

const MAX_FPS: u64 = 60;
const DARK_COLOUR: u32 = 0xB5B5AD;
const LIGHT_COLOUR: u32 = 0x1B1B19;

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

pub struct XousDisplay {
    native_buffer: Arc<Mutex<Vec<u32>>>, //[u32; WIDTH * HEIGHT],
    emulated_buffer: [u32; FB_SIZE],
    srfb: [u32; FB_SIZE],
    devboot: bool,
}

/// Encapsulates the data passed to the thread handling minifb screen updates
/// and input events.
struct MinifbThread {
    native_buffer: Arc<Mutex<Vec<u32>>>,
}

struct XousKeyboardHandler {
    kbd: keyboard::Keyboard,
    left_shift: bool,
    right_shift: bool,
}

impl XousDisplay {
    pub fn new(main_thread_token: MainThreadToken) -> XousDisplay {
        let native_buffer = vec![DARK_COLOUR; WIDTH as usize * HEIGHT as usize];
        let native_buffer = Arc::new(Mutex::new(native_buffer));

        // Start a GUI event loop on the main thread
        let thread_params = MinifbThread { native_buffer: Arc::clone(&native_buffer) };
        main_thread_token.0.send(thread_params).unwrap();

        XousDisplay { native_buffer, emulated_buffer: [0u32; FB_SIZE], srfb: [0u32; FB_SIZE], devboot: true }
    }

    pub fn set_devboot(&mut self, ena: bool) {
        if ena {
            self.devboot = true;
        }
        // ignore attempts to turn off devboot
    }

    pub fn suspend(&self) {}

    pub fn resume(&self) {}

    pub fn stash(&mut self) { self.srfb.copy_from_slice(&self.emulated_buffer); }

    pub fn pop(&mut self) {
        self.emulated_buffer[FB_WIDTH_WORDS * 32..].copy_from_slice(&self.srfb[FB_WIDTH_WORDS * 32..]);
        self.redraw();
    }

    pub fn screen_size(&self) -> Point { Point::new(WIDTH as i16, HEIGHT as i16) }

    pub fn blit_screen(&mut self, bmp: &[u32]) {
        for (dest, src) in self.emulated_buffer.iter_mut().zip(bmp.iter()) {
            *dest = *src;
        }
        self.emulated_to_native();
    }

    pub fn as_slice(&self) -> &[u32] { &self.emulated_buffer }

    pub fn native_buffer(&mut self) -> &mut [u32; FB_SIZE] { &mut self.emulated_buffer }

    pub fn redraw(&mut self) { self.emulated_to_native(); }

    fn emulated_to_native(&mut self) {
        const DEVBOOT_LINE: usize = 7;
        let mut native_buffer = self.native_buffer.lock().unwrap();
        let mut row = 0;
        for (dest_row, src_row) in
            native_buffer.chunks_mut(WIDTH as _).zip(self.emulated_buffer.chunks(WIDTH_WORDS as _))
        {
            for (dest_cell, src_cell) in dest_row.chunks_mut(32).zip(src_row) {
                for (bit, dest) in dest_cell.iter_mut().enumerate() {
                    if self.devboot && ((bit >> 1) % 2) == 0 && (row == DEVBOOT_LINE) {
                        // try to render the devboot defile somewhat accurately
                        *dest = LIGHT_COLOUR
                    } else {
                        *dest = if src_cell & (1 << bit) != 0 { DARK_COLOUR } else { LIGHT_COLOUR };
                    }
                }
            }
            row += 1;
        }
    }
}

impl MinifbThread {
    pub fn run_while(self, mut predicate: impl FnMut() -> bool) {
        let mut window = Window::new(
            "Precursor",
            WIDTH as usize,
            HEIGHT as usize,
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
        window.limit_update_rate(Some(std::time::Duration::from_micros(1000 * 1000 / MAX_FPS)));

        let xns = xous_names::XousNames::new().unwrap();
        let kbd = keyboard::Keyboard::new(&xns).expect("GFX|hosted can't connect to KBD for emulation");
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
            window.update_with_buffer(&native_buffer, WIDTH as usize, HEIGHT as usize).unwrap();
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
            self.kbd.hostmode_inject_key(c);
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
            self.kbd.hostmode_inject_key(c);
        }
    }
}
