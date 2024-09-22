use core::fmt::Write;

use codec::*;
use locales::t;
use xous::{Message, MessageEnvelope};
use String;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Audio {
    callback_id: Option<u32>,
    callback_conn: u32,
    framecount: u32,
    play_sample: f32, // count of play samples generated. in f32 to avoid int<->f32 conversions
    freq: f32,
}
impl Audio {
    pub fn new(xns: &xous_names::XousNames) -> Self {
        let callback_conn = xns.request_connection_blocking(crate::SERVER_NAME_REPL).unwrap();
        Audio { callback_id: None, callback_conn, framecount: 0, play_sample: 0.0, freq: 440.0 }
    }
}

const STOP_ID: usize = 1;
const SAMPLE_RATE_HZ: f32 = 8000.0;
// note to self: A4 = 440.0, E4 = 329.63, C4 = 261.63

use std::num::ParseIntError;
/// this will parse a simple decimal into an i32, multiplied by 1000
/// we do this because the full f32 parsing stuff is pretty heavy, some
/// 28kiB of code
#[inline(never)]
fn simple_kilofloat_parse(input: &str) -> core::result::Result<i32, ParseIntError> {
    if let Some((integer, fraction)) = input.split_once('.') {
        let mut result = integer.parse::<i32>()? * 1000;
        let mut significance = 100i32;
        for (place, digit) in fraction.chars().enumerate() {
            if place >= 3 {
                break;
            }
            if let Some(d) = digit.to_digit(10) {
                if result >= 0 {
                    result += (d as i32) * significance;
                } else {
                    result -= (d as i32) * significance;
                }
                significance /= 10;
            } else {
                return "z".parse::<i32>(); // you can't create a ParseIntError any other way
            }
        }
        Ok(result)
    } else {
        let base = input.parse::<i32>()?;
        Ok(base * 1000)
    }
}

impl<'a> ShellCmdApi<'a> for Audio {
    cmd_api!(audio);

    fn process(
        &mut self,
        args: String,
        env: &mut CommonEnv,
    ) -> Result<Option<String>, xous::Error> {
        let mut ret = String::new();
        let helpstring = t!("replapp.audio.help", locales::LANG);
        let mut tokens = &args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "tone" => {
                    self.freq = if let Some(freq_str) = tokens.next() {
                        match simple_kilofloat_parse(freq_str) {
                            Ok(f) => (f as f32) / 1000.0,
                            Err(_) => 440.0,
                        }
                    } else {
                        440.0
                    };
                    let mut duration: f32 = if let Some(duration_str) = tokens.next() {
                        simple_kilofloat_parse(duration_str).unwrap_or(500) as f32 / 1000.0
                    } else {
                        0.5
                    };
                    if duration > 10.0 {
                        duration = 10.0; // sanity check the duration so we don't go nuts
                    }

                    env.codec.setup_8k_stream().expect("couldn't set the CODEC to expected defaults");
                    env.ticktimer.sleep_ms(50).unwrap();

                    env.codec.set_speaker_volume(VolumeOps::RestoreDefault, None).unwrap();
                    env.codec.set_headphone_volume(VolumeOps::RestoreDefault, None).unwrap();

                    if self.callback_id.is_none() {
                        let cb_id = env.register_handler(String::from(self.verb()));
                        log::trace!("hooking frame callback with ID {}", cb_id);
                        env.codec.hook_frame_callback(cb_id, self.callback_conn).unwrap(); // any non-handled IDs get routed to our callback port
                        self.callback_id = Some(cb_id);
                    }

                    self.play_sample = 0.0;

                    env.codec.resume().unwrap();

                    // kick off a thread that stops the playback, after the designated delay
                    std::thread::spawn({
                        let cb_id = self.callback_id.unwrap().clone();
                        let conn = self.callback_conn.clone();
                        let duration = duration.clone();
                        move || {
                            let ticktimer = ticktimer_server::Ticktimer::new().unwrap();
                            ticktimer.sleep_ms((duration * 1000.0) as usize).unwrap();
                            xous::send_message(conn, Message::new_scalar(cb_id as usize, 0, 0, 0, STOP_ID))
                                .unwrap();
                        }
                    });
                    write!(ret, "{}", t!("replapp.audio.start", locales::LANG)).unwrap();
                }
                _ => {
                    write!(ret, "{}", helpstring).unwrap();
                }
            }
        }
        Ok(Some(ret))
    }

    fn callback(
        &mut self,
        msg: &MessageEnvelope,
        env: &mut CommonEnv,
    ) -> Result<Option<String>, xous::Error> {
        const AMPLITUDE: f32 = 0.8;

        match &msg.body {
            Message::Scalar(xous::ScalarMessage {
                id: _,
                arg1: free_play,
                arg2: _avail_rec,
                arg3: _,
                arg4: routing_id,
            }) => {
                if *routing_id == codec::AUDIO_CB_ROUTING_ID {
                    let mut frames: FrameRing = FrameRing::new();
                    let frames_to_push = if frames.writeable_count() < *free_play {
                        frames.writeable_count()
                    } else {
                        *free_play
                    };
                    self.framecount += frames_to_push as u32;

                    log::debug!("f{} p{}", self.framecount, frames_to_push);
                    for _ in 0..frames_to_push {
                        let mut frame: [u32; codec::FIFO_DEPTH] =
                            [ZERO_PCM as u32 | (ZERO_PCM as u32) << 16; codec::FIFO_DEPTH];
                        // put the "expensive" f32 comparison outside the cosine wave table computation loop
                        let omega = self.freq * 2.0 * std::f32::consts::PI / SAMPLE_RATE_HZ;
                        for sample in frame.iter_mut() {
                            let raw_sine: i16 = (AMPLITUDE
                                * cos_table::cos(self.play_sample * omega)
                                * i16::MAX as f32) as i16;
                            let left = raw_sine as u16;
                            let right = raw_sine as u16;
                            *sample = right as u32 | (left as u32) << 16;
                            self.play_sample += 1.0;
                        }

                        frames.nq_frame(frame).unwrap();
                    }
                    env.codec.swap_frames(&mut frames).unwrap();
                } else if *routing_id == STOP_ID {
                    let mut ret = String::new();
                    env.codec.abort().unwrap(); // this should stop callbacks from occurring too.
                    write!(
                        ret,
                        "{} {} {}.",
                        t!("replapp.audio.completion_a", locales::LANG),
                        self.framecount,
                        t!("replapp.audio.completion_b", locales::LANG),
                    )
                    .unwrap();
                    self.framecount = 0;
                    self.play_sample = 0.0;
                    env.codec.power_off().unwrap();
                    return Ok(Some(ret));
                }
            }
            Message::Move(_mm) => {
                log::error!("received memory message when not expected")
            }
            _ => {
                log::error!("received unknown callback type")
            }
        }
        Ok(None)
    }
}
