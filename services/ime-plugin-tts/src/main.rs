#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use ime_plugin_api::*;

use xous_ipc::{String, Buffer};
use num_traits::FromPrimitive;
use tts_frontend::*;

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // one connection only, should be the GAM
    let ime_sh_sid = xns.register_name(ime_plugin_tts::SERVER_NAME_IME_PLUGIN_TTS, Some(1)).expect("can't register server");
    log::trace!("registered with NS -- {:?}", ime_sh_sid);
    let tts = TtsFrontend::new(&xns).unwrap();

    let mytriggers = PredictionTriggers {
        newline: true,
        punctuation: true,
        whitespace: true,
    };

    log::trace!("ready to accept requests");
    loop {
        let mut msg = xous::receive_message(ime_sh_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::Input) => {
            }
            Some(Opcode::Picked) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s = buffer.as_flat::<String::<4000>, _>().unwrap();
                tts.tts_simple(s.as_str()).unwrap();
            }
            Some(Opcode::Prediction) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut prediction: Prediction = buffer.to_original::<Prediction, _>().unwrap();
                prediction.valid = false;
                buffer.replace(Return::Prediction(prediction)).expect("couldn't return Prediction");
            }
            Some(Opcode::Unpick) => {
            }
            Some(Opcode::GetPredictionTriggers) => {
                xous::return_scalar(msg.sender, mytriggers.into()).expect("couldn't return GetPredictionTriggers");
            }
            Some(Opcode::Quit) => {
                log::error!("received quit, goodbye!"); break;
            }
            None => {log::error!("unknown Opcode");}
        }
    }
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(ime_sh_sid).unwrap();
    xous::destroy_server(ime_sh_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
