use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String;

//use core::convert::TryFrom;
use codec::*;
use xous::MessageEnvelope;

#[derive(Debug)]
pub struct Audio {
    codec: codec::Codec,
    sample: xous::MemoryRange,
    header: Header,
    raw_data: *const u32,
    raw_len_bytes: u32,
    play_ptr_bytes: usize,
    framecount: u32,
}
impl Audio {
    pub fn new(xns: &xous_names::XousNames) -> Self {
        let sample = xous::syscall::map_memory(
            Some(core::num::NonZeroUsize::new(0x2600_0000).unwrap()), // it's here, because we know it's here!
            None,
            0x8_0000,
            xous::MemoryFlags::R,
        ).expect("couldn't map in the audio sample");
        let mut codec = codec::Codec::new(xns).unwrap();
        let samples: *const [u8; 16] = unsafe{sample.as_ptr().add(20)} as *const [u8; 16];
        let mut raw_header: [u8; 16] = [0; 16];
        for i in 0..16 {
            unsafe{ raw_header[i] = (*samples)[i] };
        }

        log::trace!("setting up audio stream");
        codec.setup_8k_stream().expect("couldn't set the CODEC to expected defaults");
        log::trace!("getting a callback ID");
        let callback_conn = xns.request_connection_blocking(crate::SERVER_NAME_SHELLCHAT).unwrap();
        log::trace!("hooking frame callback");
        codec.hook_frame_callback(0xDEAD_BEEF, callback_conn).unwrap(); // any non-handled IDs get routed to our callback port
        log::trace!("returning from setup");

        let mut audio = Audio {
            codec,
            sample,
            header: Header::from(raw_header),
            raw_data: unsafe{sample.as_ptr().add(44)} as *const u32,
            raw_len_bytes: unsafe{*(sample.as_ptr().add(40) as *const u32)},
            play_ptr_bytes: 0,
            framecount: 0,
        };
        /*
        // load the initial sample data
        let (play_free, _) = audio.codec.free_frames().unwrap();

        let mut frames: FrameRing = FrameRing::new();
        let frames_to_push = if frames.writeable_count() < play_free {
            frames.writeable_count()
        } else {
            play_free
        };
        log::debug!("loading up {} frames", frames_to_push);
        audio.framecount += frames_to_push as u32;
        for i in 0..frames_to_push {
            let mut frame: [u32; codec::FIFO_DEPTH] = [codec::ZERO_PCM as u32 | (codec::ZERO_PCM as u32) << 16; codec::FIFO_DEPTH];
            for sample in frame.iter_mut() {
                *sample = unsafe{audio.raw_data.add(i).read_volatile()};
            }
            audio.play_ptr_bytes += codec::FIFO_DEPTH * 4;
            frames.nq_frame(frame).unwrap();
        }
        log::debug!("pushing frames");
        audio.codec.swap_frames(&mut frames).unwrap();
        // start the playing
        log::debug!("starting playback");
        audio.codec.resume().unwrap();
        */
        audio
    }
}

impl<'a> ShellCmdApi<'a> for Audio {
    cmd_api!(audio);

    fn process(&mut self, args: String::<1024>, _env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;

        let mut ret = String::<1024>::new();
        let helpstring = "audio [play] [info]";

        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "play" => {
                    write!(ret, "Playing sample...").unwrap();
                    // load the initial sample data
                    let (play_free, _) = self.codec.free_frames().unwrap();

                    let mut frames: FrameRing = FrameRing::new();
                    let frames_to_push = if frames.writeable_count() < play_free {
                        frames.writeable_count()
                    } else {
                        play_free
                    };
                    log::debug!("loading up {} frames", frames_to_push);
                    self.framecount += frames_to_push as u32;
                    for i in 0..frames_to_push {
                        let mut frame: [u32; codec::FIFO_DEPTH] = [codec::ZERO_PCM as u32 | (codec::ZERO_PCM as u32) << 16; codec::FIFO_DEPTH];
                        for sample in frame.iter_mut() {
                            *sample = unsafe{self.raw_data.add(i).read_volatile()};
                        }
                        self.play_ptr_bytes += codec::FIFO_DEPTH * 4;
                        frames.nq_frame(frame).unwrap();
                    }
                    log::debug!("pushing frames");
                    self.codec.swap_frames(&mut frames).unwrap();
                    // start the playing
                    log::debug!("starting playback");
                    self.codec.resume().unwrap();

                    // we'll get a callback that demands the next data...
                }
                "dump" => {
                    let mut temp = String::<9>::new();
                    for i in 0..8 {
                        write!(temp, "{:08x} ", unsafe{self.raw_data.add(i).read_volatile()}).unwrap();
                        ret.append(temp.to_str()).unwrap();
                        temp.clear();
                    }
                    ret.append("\n").unwrap();
                    ret.append("\n").unwrap();
                    for i in 0..8 {
                        write!(temp, "{:08x} ", unsafe{self.raw_data.add(i + 0x2000).read_volatile()}).unwrap();
                        ret.append(temp.to_str()).unwrap();
                        temp.clear();
                    }
                }
                "info" => {
                    write!(ret, "Loaded sample is {}kHz, {} channels, {} format, {} bytes", self.header.sampling_rate, self.header.channel_count, self.header.audio_format, self.raw_len_bytes).unwrap();
                }
                _ =>  write!(ret, "{}", helpstring).unwrap(),
            }
        } else {
            write!(ret, "{}", helpstring).unwrap();
        }

        Ok(Some(ret))
    }

    fn callback(&mut self, msg: &MessageEnvelope, _env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;

        log::debug!("audio callback");
        let mut ret = String::<1024>::new();
        xous::msg_scalar_unpack!(msg, free_play, _avail_rec, _, _, {
            if self.play_ptr_bytes + codec::FIFO_DEPTH *4 < self.raw_len_bytes as usize {
                log::debug!("{} extending playback", free_play);
                let mut frames: FrameRing = FrameRing::new();
                let frames_to_push = if frames.writeable_count() < free_play {
                    frames.writeable_count()
                } else {
                    free_play
                };
                self.framecount += frames_to_push as u32;
                log::debug!("frame {}", self.framecount);
                for _ in 0..frames_to_push {
                    let mut frame: [u32; codec::FIFO_DEPTH] = [ZERO_PCM as u32 | (ZERO_PCM as u32) << 16; codec::FIFO_DEPTH];
                    for i in 0..codec::FIFO_DEPTH {
                        frame[i] = unsafe{*self.raw_data.add(i + self.play_ptr_bytes/4)};
                    }
                    self.play_ptr_bytes += codec::FIFO_DEPTH * 4;
                    frames.nq_frame(frame).unwrap();
                }
                self.codec.swap_frames(&mut frames).unwrap();

                return Ok(None)
            } else {
                log::debug!("stopping playback");
                if self.framecount != 0 {
                    self.codec.pause().unwrap(); // this should stop callbacks from occurring too.
                    self.framecount = 0;
                    self.play_ptr_bytes = 0;
                    write!(ret, "{}", "Playback finished").unwrap();
                } else {
                    // we will get extra callbacks as the pipe clears
                    return Ok(None)
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
