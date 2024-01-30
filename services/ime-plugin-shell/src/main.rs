#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use std::collections::HashMap;

use ime_plugin_api::*;
use log::{error, info};
use num_traits::FromPrimitive;
use xous::msg_scalar_unpack;
use xous_ipc::{Buffer, String};

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // one connection only, should be the GAM
    let ime_sh_sid = xns
        .register_name(ime_plugin_shell::SERVER_NAME_IME_PLUGIN_SHELL, None)
        .expect("can't register server");
    log::trace!("registered with NS -- {:?}", ime_sh_sid);

    let mut history_store: HashMap<[u32; 4], Vec<String<64>>> = HashMap::new();
    let mut active_history: Option<([u32; 4], Vec<String<64>>)> = None;
    let history_max = 4;

    /*
        use core::fmt::Write as CoreWriter;
        let mut test1: String::<64> = String::new();
        write!(test1, "This〰should overflow the box").unwrap();
        history.push(test1);
        let mut test2: String::<64> = String::new();
        write!(test2, "Another string too long").unwrap();
        history.push(test2);
        let mut test3: String::<64> = String::new();
        write!(test3, "未雨绸缪").unwrap();
        history.push(test3);
    */

    let mytriggers = PredictionTriggers { newline: true, punctuation: false, whitespace: false };

    loop {
        let mut msg = xous::receive_message(ime_sh_sid).unwrap();
        log::trace!("received message {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::Acquire) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut ret = buffer.to_original::<AcquirePredictor, _>().unwrap();
                if active_history.is_none() {
                    if let Some(token) = ret.token {
                        if let Some(h) = history_store.remove(&token) {
                            active_history = Some((token, h));
                            ret.token = Some(token);
                        } else {
                            ret.token = None;
                            log::warn!("invalid history token");
                        }
                    } else {
                        let new_token = xous::create_server_id().unwrap().to_array();
                        active_history = Some((new_token, Vec::new()));
                        ret.token = Some(new_token);
                    }
                } else {
                    ret.token = None;
                    log::warn!("attempt to acquire lock on a predictor that was already locked");
                }
                buffer.replace(ret).unwrap();
            }
            Some(Opcode::Release) => msg_scalar_unpack!(msg, t0, t1, t2, t3, {
                let token = [t0 as u32, t1 as u32, t2 as u32, t3 as u32];
                if let Some((t, h)) = active_history.take() {
                    if t == token {
                        history_store.insert(token, h);
                    } else {
                        log::warn!("Release had inconsistent api token!");
                    }
                } else {
                    log::warn!("Release called on a predictor that was in a released state");
                }
            }),
            Some(Opcode::Input) => {
                // shell does nothing with the input, it only keeps track of
                // the picked results
            }
            Some(Opcode::Picked) => {
                if let Some((_token, history)) = &mut active_history {
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let s = buffer.as_flat::<String<4000>, _>().unwrap();
                    // the API allows for large picked feedback, but this implementation only keeps the first
                    // 64 characters
                    let mut local_s: String<64> = String::new();
                    use core::fmt::Write;
                    write!(local_s, "{}", s.as_str()).expect("overflowed history variable");
                    log::trace!("storing history value | {}", s.as_str());
                    if history.len() == history_max {
                        history.remove(0);
                    }
                    history.push(local_s);
                    log::trace!("history has length {}", history.len());
                } else {
                    log::warn!("predictor not acquired, ignoring");
                }
            }
            Some(Opcode::Prediction) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut prediction: Prediction = buffer.to_original::<Prediction, _>().unwrap();
                if let Some((token, history)) = &mut active_history {
                    if *token == prediction.api_token {
                        log::trace!("querying prediction index {}", prediction.index);
                        log::trace!("{:?}", prediction);
                        if history.len() > 0 && ((prediction.index as usize) < history.len()) {
                            let mut index = prediction.index;
                            if index >= history.len() as u32 {
                                index = history.len() as u32 - 1;
                            }
                            let mut i = 1;
                            for &s in history.iter() {
                                // iterator is from oldest to newest, so do some math to go from newest to
                                // oldest TIL: there is a .rev() feature in
                                // iterators. Next time maybe use that instead.
                                if (history.len() as u32 - i) == index {
                                    // decompose the string into a character-by-character sequence
                                    // and then stuff byte-by-byte, as fits, into the return array
                                    prediction.string.clear();
                                    for ch in s.as_str().unwrap().chars() {
                                        if let Ok(_) = prediction.string.push(ch) {
                                            // it's ok, carry on.
                                        } else {
                                            // we ran out of space, stop copying
                                            break;
                                        }
                                    }
                                    prediction.valid = true;
                                    break;
                                }
                                i = i + 1;
                            }
                        } else {
                            // there is no history
                            prediction.valid = false;
                            log::trace!("no prediction found");
                        }
                        log::trace!("returning index {} string {:?}", prediction.index, prediction.string);
                    } else {
                        prediction.valid = false;
                        log::warn!("api token mismatch, ignoring");
                    }
                } else {
                    prediction.valid = false;
                    log::warn!("predictor not acquired, ignoring");
                }
                // pack our data back into the buffer to return
                buffer.replace(Return::Prediction(prediction)).expect("couldn't return Prediction");
            }
            Some(Opcode::Unpick) => {
                if let Some((_token, history)) = &mut active_history {
                    if history.len() == 1 {
                        let _ = history.remove(0);
                    } else if history.len() > 1 {
                        let _ = history.pop(); // discard the last entry
                    }
                    // in case of 0 length, do nothing
                } else {
                    log::warn!("predictor not acquired, ignoring");
                }
            }
            Some(Opcode::GetPredictionTriggers) => {
                xous::return_scalar(msg.sender, mytriggers.into())
                    .expect("couldn't return GetPredictionTriggers");
            }
            Some(Opcode::Quit) => {
                if active_history.is_some() {
                    error!("received quit, goodbye!");
                    break;
                }
            }
            None => {
                error!("unknown Opcode");
            }
        }
    }
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(ime_sh_sid).unwrap();
    xous::destroy_server(ime_sh_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
