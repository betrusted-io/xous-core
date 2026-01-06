use arbitrary_int::u5;

use crate::cmds::ShellCmdApi;

const DEFAULT_TOUCH_PIN: u8 = 1;

pub struct Touch {
    pin: u5,
}
impl Touch {
    pub fn new() -> Self { Touch { pin: u5::new(DEFAULT_TOUCH_PIN) } }
}

impl<'a> ShellCmdApi<'a> for Touch {
    cmd_api!(touch);

    fn process(&mut self, args: String, env: &mut super::CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();
        let helpstring = "touch [pin] [monitor] [wait-touch] [wait-release] [touch-release]";

        let mut tokens = args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "pin" => {
                    if let Some(pin_str) = tokens.next() {
                        let pin = u8::from_str_radix(pin_str, 10).unwrap_or(DEFAULT_TOUCH_PIN);
                        log::info!("Setting input pin to BIO pin {}", pin);
                        self.pin = arbitrary_int::u5::new(pin);
                    }
                }
                "monitor" => {
                    let mut captouch = bio_lib::captouch::Captouch::new(self.pin, None).unwrap();
                    captouch.calibrate(None).unwrap();
                    loop {
                        log::info!("{}", captouch.raw_status());
                        env.ticktimer.sleep_ms(100).ok();
                    }
                }
                "wait-touch" => {
                    let mut captouch = bio_lib::captouch::Captouch::new(self.pin, None).unwrap();
                    captouch.calibrate(None).unwrap();
                    log::info!("waiting for touch");
                    captouch.wait_touch(None).unwrap();
                    log::info!("touch!");
                }
                "wait-release" => {
                    let mut captouch = bio_lib::captouch::Captouch::new(self.pin, None).unwrap();
                    captouch.calibrate(None).unwrap();
                    log::info!("Touch and hold...");
                    captouch.wait_touch(None).unwrap();
                    env.ticktimer.sleep_ms(100).ok();
                    log::info!("...now waiting for release");
                    captouch.wait_release(None).unwrap();
                    log::info!("release!");
                }
                "touch-release" => {
                    let mut captouch = bio_lib::captouch::Captouch::new(self.pin, None).unwrap();
                    captouch.calibrate(None).unwrap();
                    log::info!("touch and release...");
                    captouch.wait_touch_and_release(None).unwrap();
                    log::info!("touched & released!");
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
