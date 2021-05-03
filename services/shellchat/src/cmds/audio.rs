use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String;

use core::convert::TryFrom;
use codec::*;

#[derive(Debug)]
pub struct Audio {
    codec: codec::Codec,
    sample: xous::MemoryRange,
    header: Header,
    raw_data: *const u32,
    raw_len_bytes: u32,
    play_ptr_bytes: usize,
}
impl Audio {
    pub fn new(xns: &xous_names::XousNames) -> Self {
        let sample = xous::syscall::map_memory(
            0x2600_0000, // it's here, because we know it's here!
            None,
            0x8_0000,
            xous::MemoryFlags::R,
        ).expect("couldn't map in the audio sample");
        let codec = codec::Codec::new(xns).unwrap();
        let mut raw_header: [u8; 16] = [0; 16];
        let samples: *const [u8; 16] = (sample.as_ptr() + 12) as *const [u8; 16];
        for i in 0..16 {
            unsafe{ raw_header[i] = (*samples)[i] };
        }

        codec.setup_8k_stream();
        let callback_conn = xns.request_connection_blocking(crate::SERVER_NAME_SHELLCHAT).unwrap();
        codec.hook_frame_callback(0xDEAD_BEEF, callback_conn).unwrap(); // any non-handled IDs get routed to our callback port

        Audio {
            codec,
            sample,
            header: Header::from(raw_header),
            raw_data: (sample.as_ptr() + 44) as *const u32,
            raw_len_bytes: unsafe{*((sample.as_ptr() + 40) as *const u32)},
            play_ptr_bytes: 0,
        }
    }
}

impl<'a> ShellCmdApi<'a> for Audio {
    cmd_api!(audio);

    fn process(&mut self, _args: String::<1024>, _env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
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

                    let frames: FrameRing::<codec::FIFO_DEPTH, 4> = FrameRing::new();
                    let frame_to_push = if frames.writeable_count() < play_free {
                        frames.writeable_count();
                    } else {
                        play_free;
                    };
                    for i in 0..frame_to_push {
                        let frame = [u32; codec::FIFO_DEPTH];
                        for i in 0..codec::FIFO_DEPTH {
                            frame[i] = unsafe{*(raw_data + i)};
                        }
                        self.play_ptr_bytes += codec::FIFO_DEPTH * 4;
                        frames.nq_frame(frame).unwrap();
                    }
                    self.codec.swap_frames(frames).unwrap();
                    // start the playing
                    self.codec.resume().unwrap();

                    // we'll get a callback that demands the next data...
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

    fn callback(&mut self, msg: &MessageEnvelope, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;

        xous::msg_scalar_unpack!(msg, free_play, avail_rec, _, _, {
            if self.play_ptr_bytes + codec::FIFO_DEPTH *4 < self.raw_len_bytes {
                let frames: FrameRing::<codec::FIFO_DEPTH, 4> = FrameRing::new();
                let frame_to_push = if frames.writeable_count() < play_free {
                    frames.writeable_count();
                } else {
                    play_free;
                };
                for i in 0..frame_to_push {
                    let frame = [u32; codec::FIFO_DEPTH];
                    for i in 0..codec::FIFO_DEPTH {
                        frame[i] = unsafe{*(raw_data + i)};
                    }
                    self.play_ptr_bytes += codec::FIFO_DEPTH * 4;
                    frames.nq_frame(frame).unwrap();
                }
                self.codec.swap_frames(frames).unwrap();

                Ok(None)
            } else {
                self.codec.pause().unwrap(); // this should stop callbacks from occurring too.
                let mut ret = String::<1024>::new();
                write!(ret, "{}", "Playback finished").unwrap();
                Ok(ret)
            }
        });
    }
}


/// Value signifying PCM data.
pub const WAV_FORMAT_PCM: u16 = 0x01;
/// Value signifying IEEE float data.
pub const WAV_FORMAT_IEEE_FLOAT: u16 = 0x03;

/// Structure for the `"fmt "` chunk of wave files, specifying key information
/// about the enclosed data.
///
/// This struct supports only PCM and IEEE float data, which is to say there is
/// no extra members for compressed format data.
#[derive(Debug, Default, Copy, Clone, Hash, PartialEq, Eq)]
pub struct Header {
    pub audio_format: u16,
    pub channel_count: u16,
    pub sampling_rate: u32,
    pub bytes_per_second: u32,
    pub bytes_per_sample: u16,
    pub bits_per_sample: u16,
}

impl Header {
    /// Creates a new Header object.
    ///
    /// ## Note
    ///
    /// While the [`crate::read`] and [`crate::write`] functions only support
    /// uncompressed PCM/IEEE for the audio format, the option is given here to
    /// select any audio format for custom implementations of wave features.
    ///
    /// ## Parameters
    ///
    /// * `audio_format` - Audio format. Only [`WAV_FORMAT_PCM`] (0x01) and
    ///                    [`WAV_FORMAT_IEEE_FLOAT`] (0x03) are supported.
    /// * `channel_count` - Channel count, the number of channels each sample
    ///                     has. Generally 1 (mono) or 2 (stereo).
    /// * `sampling_rate` - Sampling rate (e.g. 44.1kHz, 48kHz, 96kHz, etc.).
    /// * `bits_per_sample` - Number of bits in each (sub-channel) sample.
    ///                       Generally 8, 16, 24, or 32.
    ///
    /// ## Example
    ///
    /// ```
    /// let h = wav::Header::new(wav::header::WAV_FORMAT_PCM, 2, 48_000, 16);
    /// ```
    pub fn new(
        audio_format: u16,
        channel_count: u16,
        sampling_rate: u32,
        bits_per_sample: u16,
    ) -> Header {
        Header {
            audio_format,
            channel_count,
            sampling_rate,
            bits_per_sample,
            bytes_per_second: (((bits_per_sample >> 3) * channel_count) as u32) * sampling_rate,
            bytes_per_sample: ((bits_per_sample >> 3) * channel_count) as u16,
        }
    }
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
