#![cfg_attr(not(target_os = "none"), allow(dead_code))]

use std::sync::{Arc, Mutex, mpsc};

use minifb::{Key, Window, WindowOptions};
use ux_api::minigfx::{ColorNative, FrameBuffer, PixelColor, Point};
use ux_api::platform::*;

const LCD_WORDS_PER_LINE: usize = FB_WIDTH_WORDS;
const LCD_PX_PER_LINE: isize = WIDTH as isize;
const LCD_LINES: isize = FB_LINES as isize;

const MAX_FPS: usize = 60;
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
    #[allow(unreachable_code)]
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

    pub fn screen_size(&self) -> Point { Point::new(WIDTH as isize, HEIGHT as isize) }

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
            native_buffer.chunks_mut(WIDTH as _).zip(self.emulated_buffer.chunks(FB_WIDTH_WORDS as _))
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

impl FrameBuffer for XousDisplay {
    /// Puts a pixel of ColorNative at x, y. (0, 0) is defined as the lower left corner.
    fn put_pixel(&mut self, p: Point, color: ColorNative) {
        let fb: &mut [u32] = &mut self.emulated_buffer;
        let clip_y: usize;
        if p.y >= LCD_LINES {
            clip_y = LCD_LINES as usize - 1;
        } else if p.y < 0 {
            clip_y = 0;
        } else {
            clip_y = p.y as usize;
        }

        let clip_x: usize;
        if p.x >= LCD_PX_PER_LINE {
            clip_x = LCD_PX_PER_LINE as usize - 1;
        } else if p.x < 0 {
            clip_x = 0;
        } else {
            clip_x = p.x as usize;
        }

        let pc = PixelColor::from(color.0);

        if pc == PixelColor::Light {
            fb[(clip_x + clip_y * LCD_WORDS_PER_LINE * 32) / 32] |= 1 << (clip_x % 32)
        } else {
            fb[(clip_x + clip_y * LCD_WORDS_PER_LINE * 32) / 32] &= !(1 << (clip_x % 32))
        }
        // set the dirty bit on the line that contains the pixel
        fb[clip_y * LCD_WORDS_PER_LINE + (LCD_WORDS_PER_LINE - 1)] |= 0x1_0000;
    }

    /// Wrapper for compatibility sake
    unsafe fn put_pixel_unchecked(&mut self, p: Point, color: ColorNative) { self.put_pixel(p, color); }

    /// Retrieves a pixel value from the frame buffer; returns None if the point is out of bounds.
    ///
    /// Note: this has not be carefully tested as this API is not used by the legacy code base.
    /// Anyone using this API for the first time may benefit in checking that it is correct.
    fn get_pixel(&self, p: Point) -> Option<ColorNative> {
        let fb: &[u32] = &self.emulated_buffer;
        let clip_y: usize;
        if p.y >= LCD_LINES {
            return None;
        } else if p.y < 0 {
            return None;
        } else {
            clip_y = p.y as usize;
        }

        let clip_x: usize;
        if p.x >= LCD_PX_PER_LINE {
            return None;
        } else if p.x < 0 {
            return None;
        } else {
            clip_x = p.x as usize;
        }
        Some(ColorNative::from(
            (fb[(clip_x + clip_y * LCD_WORDS_PER_LINE * 32) / 32] & (1 << (clip_x % 32))) as usize,
        ));
        todo!("Check that this API is implemented correctly before using it!");
    }

    /// XORs a pixel to what is in the existing frame buffer. The exact definition of "XOR" is somewhat
    /// ambiguous for full color systems but is generally meant to imply a light/dark swap of foreground
    /// and background colors for a color theme.
    fn xor_pixel(&mut self, p: Point) {
        let fb: &mut [u32] = &mut self.emulated_buffer;
        let clip_y: usize;
        if p.y >= LCD_LINES {
            return;
        } else if p.y < 0 {
            return;
        } else {
            clip_y = p.y as usize;
        }

        let clip_x: usize;
        if p.x >= LCD_PX_PER_LINE {
            return;
        } else if p.x < 0 {
            return;
        } else {
            clip_x = p.x as usize;
        }

        fb[(clip_x + clip_y * LCD_WORDS_PER_LINE * 32) / 32] ^= 1 << (clip_x % 32);
        // set the dirty bit on the line that contains the pixel
        fb[clip_y * LCD_WORDS_PER_LINE + (LCD_WORDS_PER_LINE - 1)] |= 0x1_0000;
    }

    /// Swaps the drawable buffer to the screen and sends it to the hardware
    fn draw(&mut self) { self.redraw(); }

    /// Clears the drawable buffer
    fn clear(&mut self) {
        let fb: &mut [u32] = &mut self.emulated_buffer;
        fb.fill(0xFFFF_FFFF);
    }

    /// Returns the size of the frame buffer as a Point
    fn dimensions(&self) -> Point { self.screen_size() }

    /// Returns a raw pointer to the frame buffer
    unsafe fn raw_mut(&mut self) -> &mut ux_api::platform::FbRaw { self.native_buffer() }
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
        window.set_target_fps(MAX_FPS);

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
