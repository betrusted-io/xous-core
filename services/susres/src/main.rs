#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;

use num_traits::FromPrimitive;

use log::info;

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let susres_sid = xns.register_name(api::SERVER_NAME_SUSRES).expect("can't register server");
    log::trace!("registered with NS -- {:?}", susres_sid);

    loop {
        let msg = xous::receive_message(susres_sid).unwrap();
        /*
        match FromPrimitive::from_usize(msg.body.id()) {
            None => {
                log::error!("couldn't convert opcode");
                break
            }
        }*/
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(susres_sid).unwrap();
    xous::destroy_server(susres_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(); loop {}
}
