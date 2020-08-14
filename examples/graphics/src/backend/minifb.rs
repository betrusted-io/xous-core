use minifb::{Key, Window, WindowOptions};

const WIDTH: usize = 336;
const HEIGHT: usize = 536;
const MAX_FPS: u64 = 15;
const LIGHT_COLOUR: u32 = 0xB5B5AD;
const DARK_COLOUR: u32 = 0x1B1B19;

pub fn run() {
    let mut buffer: Vec<u32> = vec![0; WIDTH * HEIGHT];

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

    let mut lfsr = 0xace1u32;
    while window.is_open() && !window.is_key_down(Key::Escape) {
        for i in buffer.iter_mut() {
            lfsr ^= lfsr >> 7;
            lfsr ^= lfsr << 9;
            lfsr ^= lfsr >> 13;
            *i = if lfsr & 1 == 0 { LIGHT_COLOUR } else { DARK_COLOUR };
        }

        // We unwrap here as we want this code to exit if it fails.
        // Real applications may want to handle this in a different way
        window.update_with_buffer(&buffer, WIDTH, HEIGHT).unwrap();
    }
    std::process::exit(0);
}
