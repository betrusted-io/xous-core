use embedded_graphics::{drawable::Pixel, geometry::Size, pixelcolor::BinaryColor, DrawTarget};
use minifb::{Key, Window, WindowOptions};

const WIDTH: usize = 336;
const HEIGHT: usize = 536;
const MAX_FPS: u64 = 15;
const DARK_COLOUR: u32 = 0xB5B5AD;
const LIGHT_COLOUR: u32 = 0x1B1B19;

pub struct XousDisplay {
    buffer: Vec<u32>, //[u32; WIDTH * HEIGHT],
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

        let buffer = vec![DARK_COLOUR; WIDTH * HEIGHT];
        window.update_with_buffer(&buffer, WIDTH, HEIGHT).unwrap();

        XousDisplay { buffer, window }
    }

    pub fn redraw(&mut self) {
        self.window
            .update_with_buffer(&self.buffer, WIDTH, HEIGHT)
            .unwrap();
    }

    pub fn update(&mut self) {
        self.window.update();
        if !self.window.is_open() || self.window.is_key_down(Key::Escape) {
            std::process::exit(0);
        }
    }
}

impl DrawTarget<BinaryColor> for XousDisplay {
    type Error = core::convert::Infallible;

    /// Draw a `Pixel` that has a color defined as `Gray8`.
    fn draw_pixel(&mut self, pixel: Pixel<BinaryColor>) -> Result<(), Self::Error> {
        let Pixel(point, color) = pixel;
        let w = WIDTH as _;
        let h = HEIGHT as _;
        if point.x < w && point.y < h && point.x >= 0 && point.y >= 0 {
            self.buffer[(point.y * w + point.x) as usize] = if color == BinaryColor::On {
                LIGHT_COLOUR
            } else {
                DARK_COLOUR
            };
        }
        Ok(())
    }

    fn size(&self) -> Size {
        Size::new(WIDTH as _, HEIGHT as _)
    }
}
