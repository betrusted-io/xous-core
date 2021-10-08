#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;

use num_traits::*;

#[xous::xous_main]
fn xmain() -> ! {
    use crate::implementation::Pddb;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let pddb_sid = xns.register_name(api::SERVER_NAME_PDDB, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", pddb_sid);

    let mut pddb = Pddb::new();

    log::trace!("ready to accept requests");

    // register a suspend/resume listener
    let sr_cid = xous::connect(pddb_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(&xns, api::Opcode::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");

    loop {
        let msg = xous::receive_message(pddb_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(api::Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                pddb.suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                pddb.resume();
            }),
            None => {
                log::error!("couldn't convert opcode");
                break
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(pddb_sid).unwrap();
    xous::destroy_server(pddb_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
