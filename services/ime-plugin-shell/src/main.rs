#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use ime_plugin_api::*;
use core::convert::TryFrom;

use log::{error, info};
use heapless::spsc::Queue;
use heapless::consts::U4;

use rkyv::Unarchive;
use core::pin::Pin;
use rkyv::{archived_value, archived_value_mut};

#[xous::xous_main]
fn xmain() -> ! {
    let debug1 = false;
    log_server::init_wait().unwrap();
    info!("IME_SH: my PID is {}", xous::process::id());

    let ime_sh_sid = xous_names::register_name(xous::names::SERVER_NAME_IME_PLUGIN_SHELL).expect("IME_SH: can't register server");
    if debug1{info!("IME_SH: registered with NS -- {:?}", ime_sh_sid);}

    let mut history: Queue<xous::String<64>, U4> = Queue::new(); // this has 2^4 elements = 16??? or does it just have 4 elements.
    let history_max = 4;

    if true { // loads defaults into the predictor array to test things
        use core::fmt::Write as CoreWriter;
        let mut test1: xous::String::<64> = xous::String::new();
        write!(test1, "This〰should overflow the box").unwrap();
        history.enqueue(test1).unwrap();
        let mut test2: xous::String::<64> = xous::String::new();
        write!(test2, "Another string too long").unwrap();
        history.enqueue(test2).unwrap();
        let mut test3: xous::String::<64> = xous::String::new();
        write!(test3, "Mary had a little lamb").unwrap();
        history.enqueue(test3).unwrap();
    }

    let mytriggers = PredictionTriggers {
        newline: true,
        punctuation: false,
        whitespace: true,
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
                    let s: xous::String<4000> = rkyv_s.unarchive();
                    let mut local_s: xous::String<64> = xous::String::new();
                    use core::fmt::Write;
                    write!(local_s, "{:32}", s).expect("IME_SH: overflowed history variable");
                    if debug1{info!("IME_SH: storing history value | {}", s);}
                    if history.len() == history_max {
                        history.dequeue().expect("IME_SH: couldn't dequeue history");
                    }
                    history.enqueue(local_s).expect("IME_SH: couldn't store history");
                    if debug1{info!("IME_SH: history has length {}", history.len());}
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
                    if debug1{info!("IME_SH: querying prediction index {}", prediction.index);}
                    if debug1{info!("IME_SH: {:?}", prediction);}
                    if history.len() > 0 && ((prediction.index as usize) < history.len()) {
                        let mut index = prediction.index;
                        if index >= history.len() as u32 {
                            index = history.len() as u32 - 1;
                        }
                        let mut i = history.len() as u32;
                        for &s in history.iter() {
                            // iterator is from oldest to newest, so do some math to go from newest to oldest
                            if (history.len() as u32 - i) == index {
                                // decompose the string into a character-by-character sequence
                                // and then stuff byte-by-byte, as fits, into the return array
                                prediction.len = 0;
                                for ch in s.as_str().unwrap().chars() {
                                    match ch.len_utf8() {
                                        1 => {
                                            if prediction.len < prediction.string.len() as u32 {
                                                prediction.string[prediction.len as usize] = ch as u8;
                                                prediction.len += 1;
                                            } else {
                                                break;
                                            }
                                        },
                                        _ => {
                                            let mut data: [u8; 4] = [0; 4];
                                            let subslice = ch.encode_utf8(&mut data);
                                            if prediction.len + (subslice.len() as u32) < prediction.string.len() as u32 {
                                                for c in subslice.bytes() {
                                                    prediction.string[prediction.len as usize] = c;
                                                    prediction.len += 1;
                                                }
                                            } else {
                                                break;
                                            }
                                        },
                                    }
                                }
                                prediction.valid = true;
                                break;
                            }
                            i = i - 1;
                        }
                    } else { // there is no history
                        prediction.valid = false;
                        if debug1{info!("IME_SH: no prediction found");}
                    }
                    if debug1{info!("IME_SH: returning index {} string {:?}", prediction.index, prediction.string);}

                    // pack our data back into the buffer to return
                    use rkyv::Write;
                    let ret_op = Opcode::Prediction(prediction);
                    let mut writer = rkyv::ArchiveBuffer::new(buf);
                    let _pos = writer.archive(&ret_op).expect("IME_SH: Prediction query couldn't re-archive return value");
                    if debug1{info!("IME_SH: archived with pos {}", _pos);}
                },
                _ => error!("IME_SH: Got invalid MutableBorrow")
            };
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
