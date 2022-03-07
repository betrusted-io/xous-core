#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use xous_ipc::Buffer;
use xous::{msg_scalar_unpack, Message};
use num_traits::*;
use codec::{ZERO_PCM, VolumeOps, FrameRing};
use xous_tts_backend::*;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::VecDeque;

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum WaveOp {
    Return,
    Quit,
}

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let tts_sid = xns.register_name(api::SERVER_NAME_TTS, None).expect("can't register server");
    let tts_cid = xous::connect(tts_sid).unwrap();
    log::trace!("registered with NS -- {:?}", tts_sid);

    let tts_be = TtsBackend::new(&xns).unwrap();

    let tt = ticktimer_server::Ticktimer::new().unwrap();
    let mut codec = codec::Codec::new(&xns).unwrap();
    codec.setup_8k_stream().expect("couldn't setup stream");
    tt.sleep_ms(50).unwrap();
    codec.set_speaker_volume(VolumeOps::Set, Some(0.0)).unwrap();
    codec.set_headphone_volume(VolumeOps::RestoreDefault, None).unwrap();
    codec.hook_frame_callback(Opcode::CodecCb.to_u32().unwrap(), tts_cid).unwrap();
    let mut frame_count = 0;

    let wav_sid = xous::create_server().unwrap();
    let wav_cid = xous::connect(wav_sid).unwrap();
    let wavbuf = Arc::new(Mutex::new(VecDeque::<u16>::new()));
    let synth_done = Arc::new(AtomicBool::new(false));
    std::thread::spawn({
        let wav_sid = wav_sid.clone();
        let wavbuf = wavbuf.clone();
        let tts_cid = tts_cid.clone();
        let synth_done = synth_done.clone();
        move || {
            loop {
                let msg = xous::receive_message(wav_sid).unwrap();
                match FromPrimitive::from_usize(msg.body.id()) {
                    Some(WaveOp::Return) => {
                        let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let wavdat = buffer.to_original::<TtsBackendData, _>().unwrap();
                        let mut buf = wavbuf.lock().unwrap();
                        for &d in wavdat.data[..wavdat.len as usize].iter() {
                            buf.push_back(d);
                        }
                        match wavdat.control {
                            Some(TtsBeControl::End) => {
                                synth_done.store(true, Ordering::SeqCst);
                                /*
                                xous::send_message(tts_cid,
                                    Message::new_scalar(Opcode::CodecStop.to_usize().unwrap(), 0, 0, 0, 0)
                                ).expect("couldn't stop playback");
                                */
                            }
                            None => {
                                // do nothing
                            }
                        }
                    },
                    Some(WaveOp::Quit) => {
                        xous::return_scalar(msg.sender, 1).unwrap();
                        break;
                    },
                    _ => {
                        log::warn!("message unknown: {:?}", msg);
                    }
                }
            }
        }
    });
    tts_be.tts_config(wav_sid.to_array(), WaveOp::Return.to_u32().unwrap(), None).unwrap();

    loop {
        let msg = xous::receive_message(tts_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::TextToSpeech) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let msg = buffer.to_original::<TtsFrontendMsg, _>().unwrap();
                log::info!("tts front end got string {}", msg.text.as_str().unwrap());
                synth_done.store(false, Ordering::SeqCst);
                tts_be.tts_simple(msg.text.as_str().unwrap()).unwrap();
                codec.resume().unwrap();
            },
            Some(Opcode::CodecCb) => msg_scalar_unpack!(msg, free_play, available_rec, _, routing_id, {
                if routing_id == codec::AUDIO_CB_ROUTING_ID {
                    let mut frames: FrameRing = FrameRing::new();
                    let frames_to_push = if frames.writeable_count() < free_play {
                        frames.writeable_count()
                    } else {
                        free_play
                    };
                    frame_count += frames_to_push as u32;

                    log::debug!("f{} p{}", frame_count, frames_to_push);
                    let mut locked_buf = wavbuf.lock().unwrap();
                    for _ in 0..frames_to_push {
                        let mut frame: [u32; codec::FIFO_DEPTH] = [ZERO_PCM as u32 | (ZERO_PCM as u32) << 16; codec::FIFO_DEPTH];
                        for sample in frame.iter_mut() {
                            let samp = locked_buf.pop_front().unwrap_or(ZERO_PCM);
                            let left = samp as u16;
                            let right = samp as u16;
                            *sample = right as u32 | (left as u32) << 16;
                        }
                        frames.nq_frame(frame).unwrap();
                    }
                    codec.swap_frames(&mut frames).unwrap();
                    // detect if the buffer is empty and the synthesizer has indicated it's finished
                    if (locked_buf.len() == 0) && synth_done.load(Ordering::SeqCst) {
                        codec.pause().unwrap();
                    }
                } else {
                    codec.pause().unwrap(); // this should stop callbacks from occurring too.
                }
            }),
            Some(Opcode::CodecStop) => {
                log::info!("stop called");
                codec.pause().unwrap();
            }
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
