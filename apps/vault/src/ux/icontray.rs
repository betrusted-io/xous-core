use ime_plugin_api::*;

use xous_ipc::{String, Buffer};
use num_traits::FromPrimitive;

pub(crate) const SERVER_NAME_ICONTRAY: &'static str = "_vault icon tray plugin_";

const ICONS: [&'static str; 4] = [
    "\t FIDO",
    "\t⏳1234",
    "\t🔐****",
    "\t🧾🛠",
];

pub(crate) fn icontray_server() {
    let xns = xous_names::XousNames::new().unwrap();
    // one connection only, should be the GAM
    // however, because the predictor is connected only on demand -- we leave this as open-ended, which
    // means anyone could send something to this server if they knew the name of it.

    // TODO:: FIX THIS
    let ime_sh_sid = xns.register_name(SERVER_NAME_ICONTRAY, None /*Some(1)*/).expect("can't register server");

    let mytriggers = PredictionTriggers {
        newline: false,
        punctuation: false,
        whitespace: false,
    };

    loop {
        let mut msg = xous::receive_message(ime_sh_sid).unwrap();
        log::trace!("received message {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::Input) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s = buffer.as_flat::<String::<4000>, _>().unwrap();
                // input is dynamically updated here
                log::info!("Input: {}", s.as_str());
            }
            Some(Opcode::Picked) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s = buffer.as_flat::<String::<4000>, _>().unwrap();
                // input is dynamically updated here
                log::info!("Picked: {}", s.as_str());
            }
            Some(Opcode::Prediction) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut prediction: Prediction = buffer.to_original::<Prediction, _>().unwrap();
                // every key press, the four slots get queried
                prediction.string.clear();
                if prediction.index < ICONS.len() as u32 {
                    prediction.string.append(ICONS[prediction.index as usize]).ok();
                    prediction.valid = true;
                } else {
                    prediction.valid = false;
                }
                // pack our data back into the buffer to return
                buffer.replace(Return::Prediction(prediction)).expect("couldn't return Prediction");
            }
            Some(Opcode::Unpick) => {
                // ignore
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
