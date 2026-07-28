//! The app's F-key legend ("icontray").
//!
//! In Xous the four labels shown at the bottom of the screen come from the
//! app's registered IME *predictor*: the GAM queries four "prediction" slots
//! and draws them under the F1-F4 keys. This is a minimal predictor server
//! that always returns four fixed strings -- our menu labels -- exactly the
//! way apps/vault (`ux/icontray.rs`) and the chat library do it.
//!
//! We run our own (registered under a unique name and pointed at by our
//! `UxRegistration.predictor` in `main.rs`) so we control the labels; the
//! chat library hard-codes "F1".."F4" and offers no hook to change them.
//! Actual F-key *presses* are delivered separately, as raw keystrokes (see
//! `MailOp::Rawkeys` in main.rs) -- the predictor only supplies the labels.

use ime_plugin_api::*;
use num_traits::*;
use xous::msg_scalar_unpack;
use xous_ipc::Buffer;

/// Unique server name for this predictor (must not collide with the chat
/// library's `_chat icon tray plugin_`).
pub(crate) const SERVER_NAME_ICONTRAY: &str = "_mail icon tray plugin_";

/// The four F-key labels, F1..F4 left to right. Kept short to fit the narrow
/// slots. These mirror the actions wired up in `main.rs`:
/// F1=inbox, F2=compose, F3=settings, F4=reply.
const ICONS: [&str; 4] = ["INBOX", "WRITE", "CONFIG", "REPLY"];

pub(crate) fn icontray_server() {
    let xns = xous_names::XousNames::new().unwrap();
    // Open-ended connection count: the predictor is connected on demand by
    // the GAM when the app is focused (same note as the chat lib's icontray).
    let sid = xns.register_name(SERVER_NAME_ICONTRAY, None).expect("can't register icontray server");

    let mytriggers = PredictionTriggers { newline: false, punctuation: false, whitespace: false };
    let mut api_token: Option<[u32; 4]> = None;

    loop {
        let mut msg = xous::receive_message(sid).unwrap();
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
                    log::warn!("attempt to acquire a predictor lock that was already held");
                }
                buffer.replace(ret).unwrap();
            }
            Some(Opcode::Release) => msg_scalar_unpack!(msg, t0, t1, t2, t3, {
                let token = [t0 as u32, t1 as u32, t2 as u32, t3 as u32];
                if api_token == Some(token) {
                    api_token.take();
                } else {
                    log::warn!("Release called with an invalid or absent token");
                }
            }),
            Some(Opcode::Input) => {} // no free-text input in the mail shell
            Some(Opcode::Picked) => {} // ignored
            Some(Opcode::Prediction) => {
                // The GAM queries all four slots on every keypress; we always
                // answer with our fixed labels.
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut prediction: Prediction = buffer.to_original::<Prediction, _>().unwrap();
                prediction.string.clear();
                if prediction.index < ICONS.len() as u32 {
                    prediction.string.push_str(ICONS[prediction.index as usize]);
                    prediction.valid = true;
                } else {
                    prediction.valid = false;
                }
                buffer.replace(Return::Prediction(prediction)).expect("couldn't return Prediction");
            }
            Some(Opcode::Unpick) => {}
            Some(Opcode::GetPredictionTriggers) => {
                xous::return_scalar(msg.sender, mytriggers.into())
                    .expect("couldn't return GetPredictionTriggers");
            }
            Some(Opcode::Quit) => {
                if api_token.is_some() {
                    log::error!("icontray received quit");
                    break;
                }
            }
            None => log::error!("icontray got unknown opcode"),
        }
    }
    xns.unregister_server(sid).unwrap();
    xous::destroy_server(sid).unwrap();
}
