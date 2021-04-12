#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;

use num_traits::FromPrimitive;

use log::info;

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let trng_sid = xns.register_name(api::SERVER_NAME_TRNG).expect("can't register server");
    log::trace!("registered with NS -- {:?}", trng_sid);

    #[cfg(target_os = "none")]
    let trng = Trng::new();

    #[cfg(not(target_os = "none"))]
    let mut trng = Trng::new();

    // pump the TRNG hardware to clear the first number out, sometimes it is 0 due to clock-sync issues on the fifo
    trng.get_trng(2);
    log::trace!("ready to accept requests");

    loop {
        let msg = xous::receive_message(trng_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(api::Opcode::GetTrng) => xous::msg_blocking_scalar_unpack!(msg, count, _, _, _, {
                let val: [u32; 2] = trng.get_trng(count);
                xous::return_scalar2(msg.sender, val[0] as _, val[1] as _)
                    .expect("couldn't return GetTrng request");
            }),
            None => {
                log::error!("couldn't convert opcode");
                break
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(trng_sid).unwrap();
    xous::destroy_server(trng_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(); loop {}
}
