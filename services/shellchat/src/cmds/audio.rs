//use core::convert::TryFrom;
use codec::*;
use xous::MessageEnvelope;
use String;

use crate::{CommonEnv, ShellCmdApi};

#[allow(dead_code)]
#[derive(Debug)]
pub struct Audio {
    codec: codec::Codec,
    sample: xous::MemoryRange,
    header: Header,
    raw_data: *const u32,
    raw_len_bytes: u32,
    play_ptr_bytes: usize,
    framecount: u32,
    callback_id: Option<u32>,
    callback_conn: u32,
    recbuf: xous::MemoryRange,
    rec_data: *mut u32,
    rec_ptr_words: u32,
    play_or_rec_n: bool, // true if play sample, false if play recorded data
}
impl Audio {
    pub fn new(xns: &xous_names::XousNames) -> Self {
        #[cfg(any(feature = "precursor", feature = "renode"))]
        let sample = xous::syscall::map_memory(
            // 0x2634_0000 is the long sample. 0x2600_0000 is the short sample.
            Some(core::num::NonZeroUsize::new(0x2634_0000).unwrap()), /* it's here, because we know it's
                                                                       * here! */
            None,
            0x1c4_0000, // 0x8_0000 is length of short sample, 0x1C4_0000 is the long sample
            xous::MemoryFlags::R,
        )
        .expect("couldn't map in the audio sample");
        #[cfg(not(target_os = "xous"))]
        // just make a dummy mapping to keep things from crashing in hosted mode
        let sample = xous::syscall::map_memory(None, None, 0x8_0000, xous::MemoryFlags::R)
            .expect("couldn't map in the audio sample");

        let codec = codec::Codec::new(xns).unwrap();
        let samples: *const [u8; 16] = unsafe { sample.as_ptr().add(20) } as *const [u8; 16];
        let mut raw_header: [u8; 16] = [0; 16];
        for i in 0..16 {
            unsafe { raw_header[i] = (*samples)[i] };
        }
        let recbuf =
            xous::syscall::map_memory(None, None, 0x8_0000, xous::MemoryFlags::R | xous::MemoryFlags::W)
                .expect("couldn't allocate record buffer");

        let audio = Audio {
            codec,
            sample,
            header: Header::from(raw_header),
            raw_data: unsafe { sample.as_ptr().add(44) } as *const u32,
            raw_len_bytes: unsafe { *(sample.as_ptr().add(40) as *const u32) },
            play_ptr_bytes: 0,
            framecount: 0,
            callback_id: None,
            callback_conn: xns.request_connection_blocking(crate::SERVER_NAME_SHELLCHAT).unwrap(),
            recbuf,
            rec_data: recbuf.as_mut_ptr() as *mut u32,
            rec_ptr_words: 0,
            play_or_rec_n: true,
        };
        audio
    }
}

impl<'a> ShellCmdApi<'a> for Audio {
    cmd_api!(audio);

    fn process(&mut self, args: String, env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;

        let mut ret = String::new();
        let helpstring = "audio [play] [info]";

        let mut tokens = &args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "play" => {
                    log::trace!("setting up codec hardware parameters");
                    self.codec.setup_8k_stream().expect("couldn't set the CODEC to expected defaults");
                    env.ticktimer.sleep_ms(50).unwrap();

                    if self.callback_id.is_none() {
                        let cb_id = env.register_handler(String::from(self.verb()));
                        log::trace!("hooking frame callback with ID {}", cb_id);
                        self.codec.hook_frame_callback(cb_id, self.callback_conn).unwrap(); // any non-handled IDs get routed to our callback port
                        self.callback_id = Some(cb_id);
                    }

                    if self.play_or_rec_n == true {
                        write!(ret, "Playing sample...").unwrap();
                    } else {
                        write!(ret, "Playing previous microphone recording...").unwrap();
                    }
                    self.play_ptr_bytes = 0;
                    self.rec_ptr_words = 0;

                    log::info!("starting playback");
                    self.codec.resume().unwrap();

                    // we'll get a callback that demands the next data...
                }
                "stop" => {
                    self.codec.abort().unwrap(); // this should stop callbacks from occurring too.
                    write!(ret, "Playback stopped at {} frames.", self.framecount).unwrap();
                    self.framecount = 0;
                    self.play_ptr_bytes = 0;
                    self.rec_ptr_words = 0;
                    self.codec.power_off().unwrap();
                }
                "fromrec" => {
                    self.play_or_rec_n = false;
                    write!(ret, "playing back from record buffer").unwrap();
                }
                "fromsample" => {
                    self.play_or_rec_n = true;
                    write!(ret, "playing back from sample on FLASH").unwrap();
                }
                "info" => {
                    write!(
                        ret,
                        "Loaded sample is {}kHz, {} channels, {} format, {} bytes",
                        self.header.sampling_rate,
                        self.header.channel_count,
                        self.header.audio_format,
                        self.raw_len_bytes
                    )
                    .unwrap();
                } /*  */
                //"dump" => {
                //let mut temp = String::new();
                //for i in 0..8 {
                //write!(temp, "{:08x} ", unsafe{self.raw_data.add(i).read_volatile()}).unwrap();
                //ret.append(temp.to_str()).unwrap();
                //temp.clear();
                //}
                //ret.push_str("\n");
                //ret.push_str("\n");
                //for i in 0..8 {
                //write!(temp, "{:08x} ", unsafe{self.raw_data.add(i + 0x2000).read_volatile()}).unwrap();
                //ret.append(temp.to_str()).unwrap();
                //temp.clear();
                //}
                //}
                _ => write!(ret, "{}", helpstring).unwrap(),
            }
        } else {
            write!(ret, "{}", helpstring).unwrap();
        }

        Ok(Some(ret))
    }

    fn callback(
        &mut self,
        msg: &MessageEnvelope,
        _env: &mut CommonEnv,
    ) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;

        log::debug!("audio callback");
        let mut ret = String::new();
        xous::msg_scalar_unpack!(msg, free_play, _avail_rec, _, _, {
            if self.play_ptr_bytes + codec::FIFO_DEPTH * 4 < self.raw_len_bytes as usize {
                log::debug!("{} extending playback", free_play);
                let mut frames: FrameRing = FrameRing::new();
                let frames_to_push =
                    if frames.writeable_count() < free_play { frames.writeable_count() } else { free_play };
                self.framecount += frames_to_push as u32;
                log::debug!("f{} p{}", self.framecount, frames_to_push);
                for _ in 0..frames_to_push {
                    let mut frame: [u32; codec::FIFO_DEPTH] =
                        [ZERO_PCM as u32 | (ZERO_PCM as u32) << 16; codec::FIFO_DEPTH];
                    if self.play_or_rec_n {
                        for i in 0..codec::FIFO_DEPTH {
                            frame[i] = unsafe { *self.raw_data.add(i + self.play_ptr_bytes / 4) };
                        }
                    } else {
                        for i in 0..codec::FIFO_DEPTH {
                            frame[i] = unsafe { *self.rec_data.add(i + self.play_ptr_bytes / 4) };
                        }
                    }
                    self.play_ptr_bytes += codec::FIFO_DEPTH * 4;
                    frames.nq_frame(frame).unwrap();
                }
                self.codec.swap_frames(&mut frames).unwrap();

                loop {
                    if let Some(frame) = frames.dq_frame() {
                        if self.rec_ptr_words < (0x8_0000 / 4 - codec::FIFO_DEPTH) as u32 {
                            for i in 0..codec::FIFO_DEPTH {
                                unsafe { *self.rec_data.add(i + self.rec_ptr_words as usize) = frame[i] };
                            }
                            self.rec_ptr_words += codec::FIFO_DEPTH as u32;
                        } else {
                            // just silently toss any overrun for now
                        }
                    } else {
                        break;
                    };
                }

                return Ok(None);
            } else {
                log::debug!("stopping playback");
                if self.framecount != 0 {
                    self.codec.abort().unwrap(); // this should stop callbacks from occurring too.
                    write!(ret, "Playback of {} frames finished", self.framecount).unwrap();
                    self.framecount = 0;
                    self.play_ptr_bytes = 0;
                    self.rec_ptr_words = 0;
                    self.codec.power_off().unwrap();
                } else {
                    // we will get extra callbacks as the pipe clears
                    return Ok(None);
                }
            }
        });
        Ok(Some(ret))
    }
}

#[derive(Debug, Default, Copy, Clone, Hash, PartialEq, Eq)]
pub struct Header {
    pub audio_format: u16,
    pub channel_count: u16,
    pub sampling_rate: u32,
    pub bytes_per_second: u32,
    pub bytes_per_sample: u16,
    pub bits_per_sample: u16,
}

impl From<[u8; 16]> for Header {
    fn from(v: [u8; 16]) -> Self {
        let audio_format = u16::from_le_bytes([v[0], v[1]]);
        let channel_count = u16::from_le_bytes([v[2], v[3]]);
        let sampling_rate = u32::from_le_bytes([v[4], v[5], v[6], v[7]]);
        let bytes_per_second = u32::from_le_bytes([v[8], v[9], v[10], v[11]]);
        let bytes_per_sample = u16::from_le_bytes([v[12], v[13]]);
        let bits_per_sample = u16::from_le_bytes([v[14], v[15]]);

        Header {
            audio_format,
            channel_count,
            sampling_rate,
            bytes_per_second,
            bytes_per_sample,
            bits_per_sample,
        }
    }
}
