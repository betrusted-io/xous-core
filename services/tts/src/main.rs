#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use xous_ipc::Buffer;

use num_traits::*;
use xous_tts_backend::*;

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let tts_sid = xns.register_name(api::SERVER_NAME_TTS, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", tts_sid);

    let tts_be = TtsBackend::new(&xns).unwrap();

    loop {
        let msg = xous::receive_message(tts_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::TextToSpeech) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let msg = buffer.to_original::<TtsFrontendMsg, _>().unwrap();
                log::info!("tts front end got string {}", msg.text.as_str().unwrap());
                tts_be.tts_simple(msg.text.as_str().unwrap()).unwrap();
            },
            Some(Opcode::CodecCb) => {
                // tbd
            },
            Some(Opcode::Quit) => {
                log::warn!("Quit received, goodbye world!");
                break;
            },
            None => {
                log::error!("couldn't convert opcode: {:?}", msg);
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(tts_sid).unwrap();
    xous::destroy_server(tts_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
