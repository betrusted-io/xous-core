use ime_plugin_api::*;

use num_traits::*;
use std::thread;

use xous::{msg_scalar_unpack, CID};
use xous_ipc::Buffer;

pub(crate) const SERVER_NAME_ICONTRAY: &'static str = "_chat icon tray plugin_";

pub struct Icontray {
    cid: Option<CID>,
}

impl Icontray {
    pub fn new(cid: Option<CID>, icons: [&'static str; 4]) -> Self {
        log::info!("Starting icontray handler server",);
        let _ = thread::spawn({
            move || {
                server(cid, icons);
            }
        });
        Icontray { cid }
    }
}

pub(crate) fn server(_cid: Option<CID>, icons: [&str; 4]) {
    let xns = xous_names::XousNames::new().unwrap();
    // one connection only, should be the GAM
    // however, because the predictor is connected only on demand -- we leave this as open-ended, which
    // means anyone could send something to this server if they knew the name of it.

    let ime_sh_sid = xns
        .register_name(SERVER_NAME_ICONTRAY, None)
        .expect("can't register server");

    let mytriggers = PredictionTriggers {
        newline: false,
        punctuation: false,
        whitespace: false,
    };

    let mut api_token: Option<[u32; 4]> = None;
    loop {
        let mut msg = xous::receive_message(ime_sh_sid).unwrap();
        log::trace!("received message {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::Acquire) => {
                let mut buffer = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
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
            Some(Opcode::Input) => {
                log::trace!("got chat icontray Opcode::Input");
            }
            Some(Opcode::Picked) => {
                log::trace!("got chat icontray Opcode::Picked");
            }
            Some(Opcode::Prediction) => {
                // we don't check the API token here, because our "predictions" are just the four menu slots
                let mut buffer = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let mut prediction: Prediction = buffer.to_original::<Prediction, _>().unwrap();
                // every key press, the four slots get queried
                prediction.string.clear();
                if prediction.index < icons.len() as u32 {
                    prediction
                        .string
                        .append(icons[prediction.index as usize])
                        .ok();
                    prediction.valid = true;
                } else {
                    prediction.valid = false;
                }
                // pack our data back into the buffer to return
                buffer
                    .replace(Return::Prediction(prediction))
                    .expect("couldn't return Prediction");
            }
            Some(Opcode::Unpick) => {
                // ignore
            }
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
