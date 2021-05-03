use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String;

use core::convert::TryFrom;


#[derive(Debug)]
pub struct Audio {
    codec: codec::Codec,
    sample: xous::MemoryRange,
    header: Header,
    raw_data: *const u32,
    raw_len_bytes: u32,
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
        Audio {
            codec,
            sample,
            header: Header::from(raw_header),
            raw_data: (sample.as_ptr() + 44) as *const u32,
            raw_len_bytes: unsafe{*((sample.as_ptr() + 40) as *const u32)},
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
                    write!(ret, "TODO").unwrap();
                    // need to set up the callback handler
                    // load the initial sample data
                    // start the playing

                    // for now, we'll just discard the recorded data...
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
