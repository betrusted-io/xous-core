pub(crate) const SERVER_NAME_CODEC: &str = "_Low-level Audio Codec Server_";

#[allow(dead_code)]
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// TODO just play
    //PutPlayFrames,

    /// TODO just record
    //GetRecFrames,

    /// play and record
    SwapFrames,

    /// how many empty play frames, full rec frames are available right now
    FreeFrames,

    /// if the CODEC is live
    IsLive,

    /// turns off the CODEC, stops streaming
    PowerOff,

    /// Powers on the CODEC, sets up 8k stereo streaming; puts audio in "paused" state
    Setup8kStereo,

    /// Pause the stream without powering anything off. Will wait until the current playback frames in
    /// process are finished.
    PauseStream,
    /// Pause the stream without powering anything off. Clears the buffer immediately, losing any frames in
    /// playback.
    AbortStream,

    /// Resumes the stream withouth re-initializing anything
    ResumeStream,

    /// register a callback for audio frame notification
    AudioStreamSubscribe,

    /// send a frame ready notification
    AnotherFrame,

    /// set speaker volume
    SetSpeakerVolume,

    /// set headphone volume -- L&R channels are ganged together in this API, but codec can do separately
    SetHeadphoneVolume,

    /// get headphone type code
    GetHeadphoneCode,

    /// Suspend/resume callback
    SuspendResume,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum HeadphoneState {
    NotPresent = 0,
    PresentWithMic = 1,
    PresentWithoutMic = 2,
    Reserved = 3,
    CodecOff = 4,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum VolumeOps {
    UpOne,
    DownOne,
    Set,
    Mute,
    RestoreDefault,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum EventCallback {
    Event,
    Drop,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub(crate) struct ScalarHook {
    pub sid: (u32, u32, u32, u32),
    pub id: u32, /* ID of the scalar message to send through (e.g. the discriminant of the Enum on the
                  * caller's side API) */
    pub cid: xous::CID, /* caller-side connection ID for the scalar message to route to. Created by the
                         * caller before hooking. */
}

//////////////////////////////////////////////////////////////////////////////////////

pub const ZERO_PCM: u16 = 0x0; // assumes 2's compliment. 0x8000 otherwise.
pub const FIFO_DEPTH: usize = 256;
/*
The format of samples appears to be
  u32: |31 right 16|15 left 0|
*/

// pub const FRAME_DEPTH: usize = 16;
/*
Implementation note: rkyv's derives are having trouble with specifying the
depth of the frame as a const generic, eg.
pub struct FrameRing::<const F: usize> {...}
because the derive macro is not putting the `const F` at the end. This problem
exists as of rkyv 0.6.2. Implement using fixed-size variants, with traits.

We would ideally like to be able to specify various-sized frame rings for
more efficient memory usage and message passing, but for now, we will fix
the size at 16 frames.
*/
const FRAMES: usize = 16;
#[derive(rkyv::Serialize, rkyv::Deserialize, Debug, rkyv::Archive, Copy, Clone)]
pub struct FrameRing {
    // a set of frames we will circulate through
    buffer: [[u32; FIFO_DEPTH]; FRAMES],
    // the current readable frame number
    rd_frame: usize,
    // the current writeable frame number
    wr_frame: usize,
    // a pointer for more efficient recording during interrupt contexts
    rec_ptr: usize,
    // authenication token authorizing playback
    auth_token: Option<[u32; 4]>,
}
impl FrameRing {
    pub fn new() -> FrameRing {
        FrameRing {
            buffer: [[(ZERO_PCM as u32 | (ZERO_PCM as u32) << 16); FIFO_DEPTH]; FRAMES],
            rd_frame: 0,
            wr_frame: 0,
            rec_ptr: 0,
            auth_token: None,
        }
    }

    pub fn clear(&mut self) {
        self.buffer = [[(ZERO_PCM as u32 | (ZERO_PCM as u32) << 16); FIFO_DEPTH]; FRAMES];
        self.rd_frame = 0;
        self.wr_frame = 0;
        self.rec_ptr = 0;
    }

    pub fn reset_ptrs(&mut self) {
        self.rd_frame = 0;
        self.wr_frame = 0;
        self.rec_ptr = 0;
    }

    /*
      empty: rd_frame == wr_frame
      full: wr_frame == (rd_frame - 1) || (rd_frame == 0) && (wr_frame == F-1)
    */
    pub fn is_empty(&self) -> bool { self.rd_frame == self.wr_frame }

    pub fn is_full(&self) -> bool {
        if self.rd_frame == 0 { self.wr_frame == FRAMES - 1 } else { self.wr_frame == (self.rd_frame - 1) }
    }

    pub fn readable_count(&self) -> usize {
        if self.wr_frame >= self.rd_frame {
            self.wr_frame - self.rd_frame
        } else {
            self.wr_frame + FRAMES - self.rd_frame
        }
    }

    pub fn writeable_count(&self) -> usize { (FRAMES - 1) - self.readable_count() }

    // this is less time efficient, but more space efficient, as we don't allocate intermediate buffers, clear
    // them, etc.
    pub fn rec_sample(&mut self, sample: u32) -> bool {
        if self.rec_ptr < FIFO_DEPTH {
            self.buffer[self.wr_frame][self.rec_ptr] = sample;
            self.rec_ptr += 1;
            true
        } else {
            false
        }
    }

    pub fn rec_advance(&mut self) -> bool {
        if self.is_full() {
            false
        } else {
            self.rec_ptr = 0;
            self.wr_frame = (self.wr_frame + 1) % FRAMES;
            true
        }
    }

    pub fn nq_frame(&mut self, frame: [u32; FIFO_DEPTH]) -> Result<(), [u32; FIFO_DEPTH]> {
        if self.is_full() {
            return Err(frame);
        } else {
            for (&src, stereo_sample) in frame.iter().zip(self.buffer[self.wr_frame].iter_mut()) {
                *stereo_sample = src;
            }
        }
        self.wr_frame = (self.wr_frame + 1) % FRAMES;
        self.rec_ptr = 0;
        Ok(())
    }

    pub fn dq_frame(&mut self) -> Option<[u32; FIFO_DEPTH]> {
        if self.is_empty() {
            None
        } else {
            let mut playbuf: [u32; FIFO_DEPTH] = [0; FIFO_DEPTH];
            for (&src, dst) in self.buffer[self.rd_frame].iter().zip(playbuf.iter_mut()) {
                *dst = src;
            }
            self.rd_frame = (self.rd_frame + 1) % FRAMES;
            Some(playbuf)
        }
    }
}
