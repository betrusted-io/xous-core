use std::time::{Duration, Instant};

use bao1x_api::bio::IoConfigMode;
use bio_lib::ws2812::rgb_to_u32;

use crate::cmds::ShellCmdApi;

const LED_BIO_PIN: u8 = 5;
const INIT_STRIP_LENGTH: usize = 6;
const CAPTOUCH_PIN: u8 = 1;

pub struct Ws2812 {
    len: usize,
    pin: arbitrary_int::u5,
    captouch_pin: arbitrary_int::u5,
}
impl Ws2812 {
    pub fn new() -> Self {
        Ws2812 {
            len: INIT_STRIP_LENGTH,
            pin: arbitrary_int::u5::new(LED_BIO_PIN),
            captouch_pin: arbitrary_int::u5::new(CAPTOUCH_PIN),
        }
    }
}

impl<'a> ShellCmdApi<'a> for Ws2812 {
    cmd_api!(ws2812);

    fn process(&mut self, args: String, env: &mut super::CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();
        let helpstring = "ws2812 [rainbow [<duration>]] [length <pixels>] [hexcolor <#rrggbb>]";

        let mut tokens = args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "pin" => {
                    if let Some(pin_str) = tokens.next() {
                        let pin = u8::from_str_radix(pin_str, 10).unwrap_or(LED_BIO_PIN);
                        log::info!("Setting output pin to BIO pin {}", pin);
                        self.pin = arbitrary_int::u5::new(pin);
                    }
                }
                "captouch-pin" => {
                    if let Some(pin_str) = tokens.next() {
                        let pin = u8::from_str_radix(pin_str, 10).unwrap_or(LED_BIO_PIN);
                        log::info!("Setting input pin to BIO pin {}", pin);
                        self.captouch_pin = arbitrary_int::u5::new(pin);
                    }
                }
                "rainbow" => {
                    let mut ws2812 =
                        bio_lib::ws2812::Ws2812::new(bio_lib::ws2812::LedVariant::B, self.pin, None).unwrap();

                    let duration = Duration::from_secs(if let Some(duration) = tokens.next() {
                        u64::from_str_radix(duration, 10).unwrap_or(u64::MAX)
                    } else {
                        u64::MAX
                    });

                    let mut hues: Vec<f32> = Vec::new();
                    for i in 0..self.len {
                        hues.push((i as f32 * 30.0) % 360.0);
                    }
                    let mut strip = vec![0u32; self.len];

                    let start_time = Instant::now();
                    while Instant::now().duration_since(start_time).lt(&duration) {
                        // convert and update values
                        for (i, led) in hues.iter_mut().enumerate() {
                            let (r, g, b) = hsv_to_rgb(*led, 0.8, 0.05);
                            // Pack into 24-bit RGB value
                            let rgb_value: u32 = rgb_to_u32(r, g, b);
                            strip[i] = rgb_value;
                            *led += 1.0;
                            if *led >= 360.0 {
                                *led = 0.0;
                            }
                        }
                        // send
                        ws2812.send(&strip);
                        env.ticktimer.sleep_ms(10).ok();
                    }
                }
                "touch" => {
                    let mut captouch =
                        bio_lib::captouch::Captouch::new(self.captouch_pin, Some(IoConfigMode::Overwrite))
                            .unwrap();
                    let cal = captouch.calibrate(None);
                    log::info!("touch cal: {:?}", cal);

                    let duration = Duration::from_secs(if let Some(duration) = tokens.next() {
                        u64::from_str_radix(duration, 10).unwrap_or(u64::MAX)
                    } else {
                        u64::MAX
                    });

                    let mut hues: Vec<f32> = Vec::new();
                    for i in 0..self.len {
                        hues.push((i as f32 * 30.0) % 360.0);
                    }
                    let mut strip = vec![0u32; self.len];

                    let mut saturation = 0.8;
                    let mut touch_state = false;

                    let mut ws2812 = bio_lib::ws2812::Ws2812::new(
                        bio_lib::ws2812::LedVariant::B,
                        self.pin,
                        Some(IoConfigMode::SetOnly),
                    )
                    .unwrap();

                    let start_time = Instant::now();
                    while Instant::now().duration_since(start_time).lt(&duration) {
                        // convert and update values
                        for (i, led) in hues.iter_mut().enumerate() {
                            let (r, g, b) = hsv_to_rgb(*led, saturation, 0.05);
                            // Pack into 24-bit RGB value
                            let rgb_value: u32 = rgb_to_u32(r, g, b);
                            strip[i] = rgb_value;
                            *led += 1.0;
                            if *led >= 360.0 {
                                *led = 0.0;
                            }
                        }
                        // send
                        ws2812.send(&strip);

                        env.ticktimer.sleep_ms(20).ok();
                        if !touch_state & captouch.is_touched() {
                            saturation = saturation + 0.2;
                            if saturation > 1.0 {
                                saturation = 0.0;
                            }
                        }
                        touch_state = captouch.is_touched();
                    }
                }
                "length" => {
                    self.len = if let Some(len_str) = tokens.next() {
                        usize::from_str_radix(len_str, 10).unwrap_or(INIT_STRIP_LENGTH)
                    } else {
                        log::info!("Usage: length [led strip length]");
                        INIT_STRIP_LENGTH
                    };
                }
                "hexcolor" => {
                    if let Some(hexcolor) = tokens.next() {
                        let mut ws2812 =
                            bio_lib::ws2812::Ws2812::new(bio_lib::ws2812::LedVariant::B, self.pin, None)
                                .unwrap();
                        if hexcolor.len() != 7 || &hexcolor[0..1] != "#" {
                            write!(ret, "Usage: 'hexcolor #rrggbb'; the # is mandatory").unwrap();
                            return Ok(Some(ret));
                        }
                        let r = u8::from_str_radix(&hexcolor[1..3], 16).unwrap_or(0);
                        let g = u8::from_str_radix(&hexcolor[3..5], 16).unwrap_or(0);
                        let b = u8::from_str_radix(&hexcolor[5..7], 16).unwrap_or(0);
                        let strip = vec![rgb_to_u32(r, g, b); self.len];
                        ws2812.send(&strip);
                    } else {
                        write!(ret, "Usage: 'hexcolor #rrggbb'; the # is mandatory").unwrap();
                    }
                }
                _ => {
                    write!(ret, "{}", helpstring).unwrap();
                }
            }
        } else {
            write!(ret, "{}", helpstring).unwrap();
        }
        Ok(Some(ret))
    }
}

fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> (u8, u8, u8) {
    let c = value * saturation;
    let h = hue / 60.0;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let m = value - c;

    let (r, g, b) = if h < 1.0 {
        (c, x, 0.0)
    } else if h < 2.0 {
        (x, c, 0.0)
    } else if h < 3.0 {
        (0.0, c, x)
    } else if h < 4.0 {
        (0.0, x, c)
    } else if h < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    (((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8)
}
