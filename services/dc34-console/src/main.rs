use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
mod bio;
mod cmds;
mod repl;
mod shell;
use cmds::*;
// mod fxcore;
mod leds;
mod power;

// .\baosign.ps1 -Config baosec-lite -Target bunnie@10.0.245.164:code/testjig/images/

fn main() {
    // first thing: initialize the WDT
    let mut wdt = bao1x_hal::wdt::Wdt::new();
    // set for nominally 20 seconds to WDT reset - assuming 50 MHz pclk on boot
    // this is "properly" set later on once the system has fully booted and
    // the clock manager is queryable
    wdt.enable((50_000_000 / 2) * 20, true);

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());
    #[cfg(feature = "duart-debug-hal")]
    bao1x_hal::claim_duart();

    let tt = ticktimer::Ticktimer::new().unwrap();
    tt.sleep_ms(500).ok(); // pause for the system to startup

    shell::start_shell();

    tt.sleep_ms(500).ok();
    leds::start_leds();

    let run_led_fade = Arc::new(AtomicBool::new(false));
    let plugged_in = Arc::new(AtomicBool::new(false));

    let usb = usb_bao1x::UsbHid::new();
    usb.serial_console_input_injection();

    std::thread::spawn({
        let run_led_fade = run_led_fade.clone();
        let plugged_in = plugged_in.clone();
        move || {
            let xns = xous_names::XousNames::new().unwrap();
            let gfx = ux_api::service::gfx::Gfx::new(&xns).unwrap();

            const MIN: u8 = 0;
            const MAX: u8 = bao1x_hal::sh1107::DEFAULT_BRIGHTNESS;
            // Number of steps across the full fade - tune this alongside the sleep duration
            // to control the overall fade speed.
            const STEPS: u8 = MAX;
            const INC: u8 = 2;
            // Gamma value: 2.2 is standard sRGB. Increase for a longer "dark" phase;
            // decrease toward 1.0 to flatten back toward linear.
            const GAMMA: f32 = 2.4;

            // Precompute the lookup table at startup rather than doing powf() every tick.
            let lut: Vec<u8> = (0..=STEPS)
                .map(|i| {
                    let t = i as f32 / STEPS as f32; // 0.0 ..= 1.0 linear
                    let corrected = t.powf(GAMMA); // apply gamma
                    (corrected * MAX as f32).round() as u8 // scale to hardware range
                })
                .collect();

            let mut up = false;
            let mut t: u8 = 0; // linear animation parameter, 0 ..= STEPS

            let mut was_fading = run_led_fade.load(Ordering::SeqCst);
            loop {
                let do_fade = run_led_fade.load(Ordering::SeqCst);
                let is_plugged = plugged_in.load(Ordering::SeqCst);
                if do_fade && !is_plugged {
                    std::thread::sleep(std::time::Duration::from_millis(80));

                    if up {
                        t = t.saturating_add(INC).min(STEPS);
                        if t == STEPS {
                            up = false;
                        }
                    } else {
                        t = t.saturating_sub(INC).max(MIN);
                        if t == MIN {
                            up = true;
                        }
                    }

                    gfx.brightness_nonblocking(lut[t as usize]);
                } else {
                    if was_fading || is_plugged {
                        gfx.brightness_nonblocking(MAX);
                        t = MAX;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
                was_fading = do_fade;
            }
        }
    });

    // attempting to see if threading the power manager improves stability
    // safety: the wdt object is "retired" after this call, so there are no concurrency
    // issues, and the wdt memory address range is guaranteed to be correct and defined
    let wdt_addr = unsafe { wdt.to_raw() };
    std::thread::spawn({
        move || {
            power::power_manager(run_led_fade, plugged_in, wdt_addr);
        }
    });

    // idle forever, maybe turn this into a full blocking server that just parks and ends
    let dummy_sid = xous::create_server().unwrap();
    loop {
        // this just blocks forever, since the server ID is never passed to anyone else, effectively
        // parking the main thread
        let _ = xous::receive_message(dummy_sid);
    }
}
