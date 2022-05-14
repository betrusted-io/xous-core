#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use num_traits::*;

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
enum KbbOps {
    Keypress,
    TurnLightsOff,
}

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let kbb_sid = xns
        .register_name("_Keyboard backlight_", Some(1))
        .expect("can't register server");

    // connect to com
    let com = com::Com::new(&xns).expect("cannot connect to com");
    
    // connect to keyboard
    let keyboard = keyboard::Keyboard::new(&xns).expect("cannot connect to keyboard server");
    keyboard.register_observer("_Keyboard backlight_", KbbOps::Keypress.to_u32().unwrap() as usize);

    //let backlight_period_on =

    loop {
        let mut msg = xous::receive_message(kbb_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(KbbOps::Keypress) => {},
            Some(KbbOps::TurnLightsOff) => {
                com.set_backlight(0, 0).expect("cannot set backlight off");
            },
            _ => {},
        }
    }
}

fn turn_lights_on(com: &com::Com) {
    com.set_backlight(255, 128).expect("cannot set backlight on");
    std::thread::sleep(std::time::Duration::from_secs(7));
    com.set_backlight(0, 0).expect("cannot set backlight off");
}