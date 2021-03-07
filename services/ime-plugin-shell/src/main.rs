#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use ime_plugin_api::*;
use core::convert::TryFrom;

use log::{error, info};
use heapless::spsc::Queue;
use heapless::consts::U3;

use rkyv::Unarchive;
use core::pin::Pin;
use rkyv::{archived_value, archived_value_mut};

#[xous::xous_main]
fn xmain() -> ! {
    let debug1 = true;
    log_server::init_wait().unwrap();
    info!("IME_SH: my PID is {}", xous::process::id());

    let ime_sh_sid = xous_names::register_name(xous::names::SERVER_NAME_IME_PLUGIN_SHELL).expect("IME_SH: can't register server");
    if debug1{info!("IME_SH: registered with NS -- {:?}", ime_sh_sid);}

    let mut history: Queue<xous::String<4096>, U3> = Queue::new(); // this has 2^3 elements = 8
    let history_max = 8;

    let mytriggers = PredictionTriggers {
        newline: true,
        punctuation: false,
        whitespace: false,
    };

    info!("IME_SH: ready to accept requests");
    loop {
        let envelope = xous::receive_message(ime_sh_sid).unwrap();
        if debug1{info!("IME_SH: received message {:?}", envelope);}
        if let xous::Message::Borrow(m) = &envelope.body {
            let buf = unsafe { xous::XousBuffer::from_memory_message(m) };
            let bytes = Pin::new(buf.as_ref());
            let value = unsafe {
                archived_value::<Opcode>(&bytes, m.id as usize)
            };
            match &*value {
                rkyv::Archived::<Opcode>::Input(_rkyv_s) => {
                    // shell does nothing with the input, it only keeps track of
                    // the picked results
                },
                rkyv::Archived::<Opcode>::Picked(rkyv_s) => {
                    let s: xous::String<4096> = rkyv_s.unarchive();
                    if history.len() == history_max {
                        history.dequeue().expect("IME_SH: couldn't dequeue history");
                    }
                    history.enqueue(s).expect("IME_SH: couldn't store history");
                },
                _ => error!("IME_SH: unknown Borrow message")
            };
        } else if let xous::Message::MutableBorrow(m) = &envelope.body {
            let mut buf = unsafe { xous::XousBuffer::from_memory_message(m) };
            let value = unsafe {
                archived_value_mut::<Opcode>(Pin::new(buf.as_mut()), m.id as usize)
            };
            match &*value {
                rkyv::Archived::<Opcode>::Prediction(pred_r) => {
                    let mut prediction: Prediction = pred_r.unarchive();
                    if history.len() > 0 {
                        let mut index = prediction.index;
                        if index >= history.len() as u32 {
                            index = history.len() as u32 - 1;
                        }
                        let mut i = history.len() as u32;
                        let mut retstr = xous::String::new();
                        for &s in history.iter() {
                            // iterator is from oldest to newest, so do some math to go from newest to oldest
                            if (history.len() as u32 - i) == index {
                                retstr = s;
                                break;
                            }
                            i = i + 1;
                        }
                        prediction.string = retstr;
                    } else { // there is no history
                        // return the empty string
                        prediction.string = xous::String::new();
                    }
                },
                _ => error!("IME_SH: unknown MutableBorrow message"),
            }
        } else if let Ok(opcode) = Opcode::try_from(&envelope.body) {
            match opcode {
                Opcode::Unpick => {
                    if history.len() == 1 {
                        history.dequeue().expect("IME_SH: couldn't dequeue in Unpick (1)");
                    } else if history.len() > 1 {
                        // rotate everything around except the last entry
                        for _ in 0 .. history.len() - 1 {
                            let s = history.dequeue().expect("IME_SH: couldn't dequee in Unpick (>1)");
                            history.enqueue(s).expect("IME_SH: couldn't enqueue in Unpick");
                        }
                        // discard the last entry
                        history.dequeue().expect("IME_SH: couldn't dequeue in Unpick (>1 last)");
                    }
                    // in case of 0 length, do nothing
                },
                Opcode::GetPredictionTriggers => {
                    xous::return_scalar(envelope.sender, mytriggers.into()).expect("IME_SH: couldn't return GetPredictionTriggers");
                },
                _ => error!("IME_SH: unknown Opcode"),
            }
        } else {
            info!("IME_SH: received unknown message type {:?}", envelope);
            panic!("IME_SH: received unknown message type");
        }
    }
}
