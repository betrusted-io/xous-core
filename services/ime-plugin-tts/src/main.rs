#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use ime_plugin_api::*;
use num_traits::FromPrimitive;
use tts_frontend::*;
use xous::msg_scalar_unpack;
use xous_ipc::Buffer;

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // one connection only, should be the GAM
    let ime_sh_sid =
        xns.register_name(ime_plugin_tts::SERVER_NAME_IME_PLUGIN_TTS, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", ime_sh_sid);
    let tts = TtsFrontend::new(&xns).unwrap();

    let mytriggers = PredictionTriggers { newline: true, punctuation: true, whitespace: true };

    log::trace!("ready to accept requests");
    let mut api_token: Option<[u32; 4]> = None;
    loop {
        let mut msg = xous::receive_message(ime_sh_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::Acquire) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut ret = buffer.to_original::<AcquirePredictor, _>().unwrap();
                if api_token.is_none() {
                    if let Some(token) = ret.token {
                        api_token = Some(token);
                    } else {
                        let new_token = xous::create_server_id().unwrap().to_array();
                        ret.token = Some(new_token);
                        api_token = Some(new_token);
                    }
                } else {
                    ret.token = None;
                    log::warn!("attempt to acquire lock on a predictor that was already locked");
                }
                buffer.replace(ret).unwrap();
            }
            Some(Opcode::Release) => msg_scalar_unpack!(msg, t0, t1, t2, t3, {
                let token = [t0 as u32, t1 as u32, t2 as u32, t3 as u32];
                if let Some(t) = api_token {
                    if t == token {
                        api_token.take();
                    } else {
                        log::warn!("Release called with an invalid token");
                    }
                } else {
                    log::warn!("Release called on a predictor that was in a released state");
                }
            }),
            Some(Opcode::Input) => {}
            Some(Opcode::Picked) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s = buffer.as_flat::<String, _>().unwrap();
                tts.tts_simple(s.as_str()).unwrap();
            }
            Some(Opcode::Prediction) => {
                // we don't check the API token, because we always return `false`
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut prediction: Prediction = buffer.to_original::<Prediction, _>().unwrap();
                prediction.valid = false;
                buffer.replace(Return::Prediction(prediction)).expect("couldn't return Prediction");
            }
            Some(Opcode::Unpick) => {}
            Some(Opcode::GetPredictionTriggers) => {
                xous::return_scalar(msg.sender, mytriggers.into())
                    .expect("couldn't return GetPredictionTriggers");
            }
            Some(Opcode::Quit) => {
                if api_token.is_some() {
                    log::error!("received quit, goodbye!");
                    break;
                }
            }
            None => {
                log::error!("unknown Opcode");
            }
        }
    }
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(ime_sh_sid).unwrap();
    xous::destroy_server(ime_sh_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
