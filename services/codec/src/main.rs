#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
mod backend;
use backend::Codec;

use num_traits::{ToPrimitive, FromPrimitive};
use xous_ipc::Buffer;
use xous::{CID, msg_scalar_unpack};

use log::info;

use api::*;

#[derive(Copy, Clone, Debug)]
struct ScalarCallback {
    server_to_cb_cid: CID,
    cb_to_client_cid: CID,
    cb_to_client_id: u32,
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // unlimited connections allowed; authentication via token is used
    let codec_sid = xns.register_name(api::SERVER_NAME_CODEC, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", codec_sid);

    let codec_conn = xous::connect(codec_sid).expect("couldn't make connection for the codec implementation");
    let mut codec = Box::new(Codec::new(codec_conn, &xns));

    let ticktimer = ticktimer_server::Ticktimer::new().unwrap();
    log::trace!("ready to accept requests");

    // register a suspend/resume listener
    let sr_cid = xous::connect(codec_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(Some(susres::SuspendOrder::Normal), &xns, api::Opcode::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");
    /*
    let trng = trng::Trng::new(&xns).unwrap();
    let mut noise: [u32; codec::FIFO_DEPTH] = [0; codec::FIFO_DEPTH];
    for i in 0..noise.len() {
        noise[i] = trng.get_u32().unwrap();
    }
    */

    let mut speaker_analog_gain_db: f32 = -6.0;
    let mut headphone_analog_gain_db: f32 = -15.0;
    let mut audio_cb_conns: [Option<ScalarCallback>; 32] = [None; 32];
    loop {
        let mut msg = xous::receive_message(codec_sid).unwrap();
        //log::trace!("got message {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(api::Opcode::SuspendResume) => msg_scalar_unpack!(msg, token, _, _, _, {
                codec.suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                codec.resume();
            }),
            Some(api::Opcode::PowerOff) => xous::msg_scalar_unpack!(msg, _, _, _, _, {
                codec.power(false);
            }),
            Some(api::Opcode::Setup8kStereo) => xous::msg_scalar_unpack!(msg, _, _, _, _, {
                log::trace!("turning on codec power");
                codec.power(true);
                log::trace!("waiting for power up");
                ticktimer.sleep_ms(2).unwrap();
                log::trace!("initializing codec");
                codec.init();
            }),
            Some(api::Opcode::ResumeStream) => xous::msg_scalar_unpack!(msg, _, _, _, _, {
                if codec.is_on() && codec.is_init() {
                    codec.audio_i2s_start();
                } else {
                    log::error!("attempted to resume a stream on an unitialized codec, ignoring!")
                }
            }),
            Some(api::Opcode::PauseStream) => xous::msg_scalar_unpack!(msg, _, _, _, _, {
                if codec.is_on() && codec.is_init() && codec.is_live() {
                    codec.drain(); // this will suppress any future callbacks from firing
                    while codec.can_play() {
                        xous::yield_slice();
                    }
                    codec.audio_i2s_stop();
                } else {
                    log::error!("attempted to pause a stream on an uninitialized codec, ignoring!")
                }
            }),
            Some(api::Opcode::AbortStream) => xous::msg_scalar_unpack!(msg, _, _, _, _, {
                if codec.is_on() && codec.is_init() && codec.is_live() {
                    codec.audio_i2s_stop();
                } else {
                    log::error!("attempted to abort a stream on an uninitialized codec, ignoring!")
                }
            }),
            Some(api::Opcode::IsLive) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let ret = if codec.is_live() {
                    1
                } else {
                    0
                };
                xous::return_scalar(msg.sender, ret).expect("couldn't return if codec is live");
            }),
            Some(api::Opcode::FreeFrames) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let play_free = codec.free_play_frames();
                let rec_avail = codec.available_rec_frames();
                xous::return_scalar2(msg.sender, play_free, rec_avail).expect("couldn't return FreeFrames");
            }),
            Some(api::Opcode::SwapFrames) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut framering = buffer.to_original::<codec::api::FrameRing, _>().unwrap();

                loop {
                    if let Some(frame) = framering.dq_frame() {
                        let mut printed = false;
                        while codec.free_play_frames() == 0 {
                            if !printed {
                                log::debug!("swap overrun");
                                printed = true;
                            }
                            xous::yield_slice();
                            if !codec.is_live() {
                                // handle the case that play stopped while we're trying to run the swap
                                break;
                            }
                        }
                        if codec.free_play_frames() > 0 {
                            codec.nq_play_frame(frame).unwrap(); // throw away the result because we know this must succeed
                        } else {
                            // TODO: need to define a behavior when we have a play overrun. Do we:
                            // - wait until we can play the frame?
                            // - throw away the frame?
                        }
                    } else {
                        break;
                    }
                }

                framering.reset_ptrs();
                loop {
                    if let Some(frame) = codec.dq_rec_frame() {
                        if !framering.is_full() {
                            framering.nq_frame(frame).unwrap(); // always succeeds because we checked if we're full first
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }

                buffer.replace(framering).unwrap();
            },
            Some(api::Opcode::AudioStreamSubscribe) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let hookdata = buffer.to_original::<ScalarHook, _>().unwrap();
                log::trace!("hooking {:?}", hookdata);
                do_hook(hookdata, &mut audio_cb_conns);
                log::trace!("hook done, {:?}", audio_cb_conns);
            }
            Some(api::Opcode::AnotherFrame) => xous::msg_scalar_unpack!(msg, _rdcount, _wrcount, _, _, {
                //log::trace!("A rd {} wr {}", rdcount, wrcount);
                send_event(&audio_cb_conns, codec.free_play_frames(), codec.available_rec_frames());
            }),
            Some(api::Opcode::SetSpeakerVolume) => xous::msg_scalar_unpack!(msg, op, gain_code, _, _, {
                match FromPrimitive::from_usize(op) {
                    Some(VolumeOps::Set) => {
                        speaker_analog_gain_db = -(gain_code as f32) / 10.0;
                    },
                    Some(VolumeOps::Mute) => {
                        speaker_analog_gain_db = -80.0;
                    },
                    Some(VolumeOps::RestoreDefault) => {
                        speaker_analog_gain_db = -6.0;
                    },
                    Some(VolumeOps::UpOne) => {
                        speaker_analog_gain_db += 3.0;
                        if speaker_analog_gain_db >= 0.0 {
                            speaker_analog_gain_db = 0.0;
                        }
                    },
                    Some(VolumeOps::DownOne) => {
                        speaker_analog_gain_db -= 3.0;
                        if speaker_analog_gain_db <= -80.0 {
                            speaker_analog_gain_db = -80.0;
                        }
                    },
                    _ => log::error!("got speaker volume primitive that we don't recognize, ignoring!"),
                };
                codec.set_speaker_gain_db(speaker_analog_gain_db);
            }),
            Some(api::Opcode::SetHeadphoneVolume) => xous::msg_scalar_unpack!(msg, op, gain_code, _, _, {
                match FromPrimitive::from_usize(op) {
                    Some(VolumeOps::Set) => {
                        headphone_analog_gain_db = -(gain_code as f32) / 10.0;
                    },
                    Some(VolumeOps::Mute) => {
                        headphone_analog_gain_db = -80.0;
                    },
                    Some(VolumeOps::RestoreDefault) => {
                        headphone_analog_gain_db = -6.0;
                    },
                    Some(VolumeOps::UpOne) => {
                        headphone_analog_gain_db += 3.0;
                        if headphone_analog_gain_db >= 0.0 {
                            headphone_analog_gain_db = 0.0;
                        }
                    },
                    Some(VolumeOps::DownOne) => {
                        headphone_analog_gain_db -= 3.0;
                        if headphone_analog_gain_db <= -80.0 {
                            headphone_analog_gain_db = -80.0;
                        }
                    },
                    _ => log::error!("got speaker volume primitive that we don't recognize, ignoring!"),
                };
                codec.set_headphone_gain_db(headphone_analog_gain_db, headphone_analog_gain_db);
            }),
            Some(api::Opcode::GetHeadphoneCode) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if codec.is_init() && codec.is_on() {
                    let hp_code = codec.get_headset_code();
                    if hp_code & 0x80 != 0x80 {
                        log::warn!("Headphone detection polled, but detection is not enabled in hardware!");
                    }
                    log::debug!("headset code: 0x{:x}", hp_code);
                    let code = if hp_code == 0xff { // kind of a hack, we could also check codec power state
                        HeadphoneState::CodecOff
                    } else {
                        match (hp_code >> 5) & 0x3 {
                            0b00 => HeadphoneState::NotPresent,
                            0b01 => HeadphoneState::PresentWithoutMic,
                            0b10 => HeadphoneState::Reserved,
                            0b11 => HeadphoneState::PresentWithMic,
                            _ => HeadphoneState::Reserved,
                        }
                    };
                    xous::return_scalar(msg.sender, code as usize).ok();
                } else {
                    xous::return_scalar(msg.sender, HeadphoneState::CodecOff as usize).ok();
                }
            }),
            None => {
                log::error!("couldn't convert opcode");
                break
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    unhook(&mut audio_cb_conns);
    xns.unregister_server(codec_sid).unwrap();
    xous::destroy_server(codec_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}


fn do_hook(hookdata: ScalarHook, cb_conns: &mut [Option<ScalarCallback>; 32]) {
    let (s0, s1, s2, s3) = hookdata.sid;
    let sid = xous::SID::from_u32(s0, s1, s2, s3);
    let server_to_cb_cid = xous::connect(sid).unwrap();
    let cb_dat = Some(ScalarCallback {
        server_to_cb_cid,
        cb_to_client_cid: hookdata.cid,
        cb_to_client_id: hookdata.id,
    });
    let mut found = false;
    for entry in cb_conns.iter_mut() {
        if entry.is_none() {
            *entry = cb_dat;
            found = true;
            break;
        }
    }
    if !found {
        log::error!("ran out of space registering callback");
    }
}
fn unhook(cb_conns: &mut [Option<ScalarCallback>; 32]) {
    for entry in cb_conns.iter_mut() {
        if let Some(scb) = entry {
            xous::send_message(scb.server_to_cb_cid,
                xous::Message::new_blocking_scalar(EventCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0)
            ).unwrap();
            unsafe{xous::disconnect(scb.server_to_cb_cid).unwrap();}
        }
        *entry = None;
    }
}
fn send_event(cb_conns: &[Option<ScalarCallback>; 32], free_play: usize, avail_rec: usize) {
    for entry in cb_conns.iter() {
        if let Some(scb) = entry {
            // note that the "which" argument is only used for GPIO events, to indicate which pin had the event
            xous::send_message(scb.server_to_cb_cid,
                xous::Message::new_scalar(EventCallback::Event.to_usize().unwrap(),
                   scb.cb_to_client_cid as usize, scb.cb_to_client_id as usize, free_play, avail_rec)
            ).unwrap();
        };
    }
}
