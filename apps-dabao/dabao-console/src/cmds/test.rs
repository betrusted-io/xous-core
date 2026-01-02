use String;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Test {}

impl<'a> ShellCmdApi<'a> for Test {
    cmd_api!(test);

    // inserts boilerplate for command API

    fn process(&mut self, args: String, _env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();

        #[allow(unused_variables)]
        let helpstring = "Test commands. See code for options.";

        let mut tokens = args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "ws2812b" => {
                    let mut ws2812 = bio_lib::ws2812::Ws2812::new(
                        bio_lib::ws2812::LedVariant::B,
                        arbitrary_int::u5::new(5),
                    )
                    .unwrap();

                    let mut hues: Vec<f32> = Vec::new();
                    for i in 0..6 {
                        hues.push(i as f32 * 30.0);
                    }
                    let mut strip = [0u32; 6];
                    loop {
                        // convert and update values
                        for (i, led) in hues.iter_mut().enumerate() {
                            let (r, g, b) = hsv_to_rgb(*led, 0.8, 0.05);
                            // Pack into 24-bit RGB value
                            let rgb_value: u32 = bio_lib::ws2812::Ws2812::rgb_to_u32(r, g, b);
                            strip[i] = rgb_value;
                            *led += 1.0;
                            if *led >= 360.0 {
                                *led = 0.0;
                            }
                        }
                        // send
                        ws2812.send(&strip);
                        _env.ticktimer.sleep_ms(10).ok();
                    }
                }
                "timer" => {
                    let start = _env.ticktimer.elapsed_ms();
                    log::info!("Starting test");
                    let mut seconds = 0;
                    loop {
                        let elapsed = _env.ticktimer.elapsed_ms() - start;
                        if elapsed > seconds * 1000 {
                            log::info!("{} s", seconds);
                            seconds += 1;
                        }
                    }
                }
                "env" => {
                    log::info!("{:?}", std::env::vars());
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
