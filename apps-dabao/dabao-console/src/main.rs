mod cmds;
#[cfg(feature = "ctap-bringup")]
mod ctap;
mod repl;
mod shell;
use cmds::*;
use usb_bao1x::UsbHid;

fn main() {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());
    let tt = ticktimer::Ticktimer::new().unwrap();

    #[cfg(feature = "usb")]
    {
        let usb = UsbHid::new();
        usb.serial_console_input_injection();
    }

    // spawn the shell thread
    shell::start_shell();

    #[cfg(feature = "ctap-bringup")]
    {
        tt.sleep_ms(4000).ok();
        crate::ctap::ctap_test();
    }

    // idle the main thread, all children are spawned
    loop {
        tt.sleep_ms(2_000).ok();
    }
}
