#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use ime_plugin_api::*;

use log::{error, info};
use heapless::spsc::Queue;
use heapless::consts::*;

use xous_ipc::{String, Buffer};
use num_traits::FromPrimitive;

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // one connection only, should be the GAM
    let ime_sh_sid = xns.register_name(xous::names::SERVER_NAME_IME_PLUGIN_SHELL, Some(1)).expect("can't register server");
    log::trace!("registered with NS -- {:?}", ime_sh_sid);

    let mut history: Queue<String<64>, U4> = Queue::new(); // this has 2^4 elements = 16??? or does it just have 4 elements.
    let history_max = 4;

    if false { // loads defaults into the predictor array to test things
        use core::fmt::Write as CoreWriter;
        let mut test1: String::<64> = String::new();
        write!(test1, "This〰should overflow the box").unwrap();
        history.enqueue(test1).unwrap();
        let mut test2: String::<64> = String::new();
        write!(test2, "Another string too long").unwrap();
        history.enqueue(test2).unwrap();
        let mut test3: String::<64> = String::new();
        write!(test3, "未雨绸缪").unwrap();
        history.enqueue(test3).unwrap();
    }

    let mytriggers = PredictionTriggers {
        newline: true,
        punctuation: false,
        whitespace: false,
    };

    info!("ready to accept requests");
    loop {
        let mut msg = xous::receive_message(ime_sh_sid).unwrap();
        log::trace!("received message {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::Input) => {
                // shell does nothing with the input, it only keeps track of
                // the picked results
            }
            Some(Opcode::Picked) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s = buffer.as_flat::<String::<4000>, _>().unwrap();
                // the API allows for large picked feedback, but this implementation only keeps the first 64 characters
                let mut local_s: String<64> = String::new();
                use core::fmt::Write;
                write!(local_s, "{}", s.as_str()).expect("overflowed history variable");
                log::trace!("storing history value | {}", s.as_str());
                if history.len() == history_max {
                    history.dequeue().expect("couldn't dequeue history");
                }
                history.enqueue(local_s).expect("couldn't store history");
                log::trace!("history has length {}", history.len());
            }
            Some(Opcode::Prediction) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut prediction: Prediction = buffer.to_original::<Prediction, _>().unwrap();
                log::trace!("querying prediction index {}", prediction.index);
                log::trace!("{:?}", prediction);
                if history.len() > 0 && ((prediction.index as usize) < history.len()) {
                    let mut index = prediction.index;
                    if index >= history.len() as u32 {
                        index = history.len() as u32 - 1;
                    }
                    let mut i = 1;
                    for &s in history.iter() {
                        // iterator is from oldest to newest, so do some math to go from newest to oldest
                        // TIL: there is a .rev() feature in iterators. Next time maybe use that instead.
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
                } else { // there is no history
                    prediction.valid = false;
                    log::trace!("no prediction found");
                }
                log::trace!("returning index {} string {:?}", prediction.index, prediction.string);

                // pack our data back into the buffer to return
                buffer.replace(Return::Prediction(prediction)).expect("couldn't return Prediction");
            }
            Some(Opcode::Unpick) => {
                if history.len() == 1 {
                    history.dequeue().expect("couldn't dequeue in Unpick (1)");
                } else if history.len() > 1 {
                    // rotate everything around except the last entry
                    for _ in 0 .. history.len() - 1 {
                        let s = history.dequeue().expect("couldn't dequee in Unpick (>1)");
                        history.enqueue(s).expect("couldn't enqueue in Unpick");
                    }
                    // discard the last entry
                    history.dequeue().expect("couldn't dequeue in Unpick (>1 last)");
                }
                // in case of 0 length, do nothing
            }
            Some(Opcode::GetPredictionTriggers) => {
                xous::return_scalar(msg.sender, mytriggers.into()).expect("couldn't return GetPredictionTriggers");
            }
            None => {error!("unknown Opcode"); break}
        }
    }
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(ime_sh_sid).unwrap();
    xous::destroy_server(ime_sh_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
