use minifb::{Key, Window, WindowOptions};

const WIDTH: usize = 336;
const HEIGHT: usize = 536;

/// Width of the screen in 32-bit words
const WIDTH_WORDS: usize = 11;
const FB_SIZE: usize = WIDTH_WORDS * HEIGHT; // 44 bytes by 536 lines

const MAX_FPS: u64 = 15;
const DARK_COLOUR: u32 = 0xB5B5AD;
const LIGHT_COLOUR: u32 = 0x1B1B19;

pub struct XousDisplay {
    native_buffer: Vec<u32>, //[u32; WIDTH * HEIGHT],
    emulated_buffer: [u32; FB_SIZE],
    window: Window,
}

impl XousDisplay {
    pub fn new() -> XousDisplay {
        let mut window = Window::new(
            "Betrusted",
            WIDTH,
            HEIGHT,
            WindowOptions {
                scale_mode: minifb::ScaleMode::AspectRatioStretch,
                resize: true,
                ..WindowOptions::default()
            },
        )
        .unwrap_or_else(|e| {
            panic!("{}", e);
        });

        // Limit the maximum refresh rate
        window.limit_update_rate(Some(std::time::Duration::from_micros(
            1000 * 1000 / MAX_FPS,
        )));

        let native_buffer = vec![DARK_COLOUR; WIDTH * HEIGHT];
        window
            .update_with_buffer(&native_buffer, WIDTH, HEIGHT)
            .unwrap();

        XousDisplay {
            native_buffer,
            window,
            emulated_buffer: [0u32; FB_SIZE],
        }
    }

    pub fn blit_screen(&mut self, bmp: [u32; FB_SIZE]) {
        for (dest, src) in self.emulated_buffer.iter_mut().zip(bmp.iter()) {
            *dest = *src;
        }
    }

    pub fn native_buffer(&mut self) -> &mut [u32; FB_SIZE] {
        &mut self.emulated_buffer
    }

    pub fn redraw(&mut self) {
        self.emulated_to_native();
        self.window
            .update_with_buffer(&self.native_buffer, WIDTH, HEIGHT)
            .unwrap();
    }

    pub fn update(&mut self) {
        self.emulated_to_native();
        self.window.update();
        if !self.window.is_open() || self.window.is_key_down(Key::Escape) {
            std::process::exit(0);
        }
    }

    fn emulated_to_native(&mut self) {
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                // print!("({}, {}): {} @ {}: ", x, y, (x + y * 44 * 8) / 8, self.emulated_buffer.len());
                // println!("{:08x}", self.emulated_buffer[(x + y * 44 * 8) / 8]);
                if ((x + y * 11 * 32) / 32) >= self.emulated_buffer.len() {
                    panic!(
                        "Value exceeds src buffer ({}, {}) @ {}",
                        x,
                        y,
                        self.emulated_buffer.len()
                    );
                }
                if (x + y * WIDTH) > self.native_buffer.len() {
                    panic!("Value exceeds dest buffer");
                }
                self.native_buffer[x + y * WIDTH] =
                    if ((self.emulated_buffer[(x + y * 11 * 32) / 32] >> (x % 32)) & 1) > 0 {
                        DARK_COLOUR
                    } else {
                        LIGHT_COLOUR
                    };
            }
        }
    }
}
