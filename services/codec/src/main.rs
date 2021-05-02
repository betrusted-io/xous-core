#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
mod backend;
use backend::Codec;

use num_traits::FromPrimitive;

use log::info;

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let codec_sid = xns.register_name(api::SERVER_NAME_CODEC).expect("can't register server");
    log::trace!("registered with NS -- {:?}", codec_sid);

    let mut codec = Codec::new();

    log::trace!("ready to accept requests");

    // register a suspend/resume listener
    let sr_cid = xous::connect(codec_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(&xns, api::Opcode::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");

    loop {
        let msg = xous::receive_message(codec_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(api::Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                codec.suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                codec.resume();
            }),
            None => {
                log::error!("couldn't convert opcode");
                break
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(codec_sid).unwrap();
    xous::destroy_server(codec_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(); loop {}
}
