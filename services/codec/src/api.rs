pub(crate) const SERVER_NAME_CODEC: &str     = "_Low-level Audio Codec Server_";

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

    /// Pause the stream without powering anything off
    PauseStream,

    /// Resumes the stream withouth re-initializing anything
    ResumeStream,

    /// register a callback for audio frame notification
    AudioStreamSubscribe,

    /// send a frame ready notification
    AnotherFrame,

    /// Suspend/resume callback
    SuspendResume,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum EventCallback {
    Event,
    Drop,
}

//////////////////////////////////////////////////////////////////////////////////////

pub(crate) const ZERO_PCM: u16 = 0x0; // assumes 2's compliment. 0x8000 otherwise.
pub const FIFO_DEPTH: usize = 256;
pub const FRAME_DEPTH: usize = 16;
/*
The format of samples appears to be
  u32: |31 right 16|15 left 0|
*/

pub struct FrameRing<const DEPTH: usize, const FRAMES: usize> {
    // a set of frames we will circulate through
    buffer: [[u32; DEPTH]; FRAMES],
    // the current readable frame number
    rd_frame: usize,
    // the current writeable frame number
    wr_frame: usize,
}
impl<const DEPTH: usize, const FRAMES: usize> FrameRing<DEPTH, FRAMES> {
    pub fn new() -> FrameRing::<DEPTH, FRAMES> {
        FrameRing {
            buffer: [[(ZERO_PCM as u32 | (ZERO_PCM as u32) << 16); DEPTH]; FRAMES],
            rd_frame: 0,
            wr_frame: 0,
        }
    }
    /*
      empty: rd_frame == wr_frame
      full: wr_frame == (rd_frame - 1) || (rd_frame == 0) && (wr_frame == FRAMES-1)
    */
    pub fn is_empty(&self) -> bool {
        self.rd_frame == self.wr_frame
    }
    pub fn is_full(&self) -> bool {
        if self.rd_frame == 0 {
            self.wr_frame == FRAMES-1
        } else {
            self.wr_frame == (self.rd_frame - 1)
        }
    }
    pub fn readable_count(&self) -> usize {
        if self.wr_frame >= self.rd_frame {
            self.wr_frame - self.rd_frame
        } else {
            self.wr_frame + FRAMES - self.rd_frame
        }
    }
    pub fn writeable_count(&self) -> usize {
        (FRAMES-1) - self.readable_count()
    }
    pub fn nq_frame(&mut self, frame: [u32; DEPTH]) -> Result<(), [u32; DEPTH]> {
        if self.is_full() {
            Err(frame)
        } else {
            for (src, &stereo_sample) in frame.iter().zip(self.buffer[self.wr_frame].iter_mut()) {
                *stereo_sample = src;
            }
        }
        self.wr_frame = ((self.wr_frame + 1) % FRAMES);
    }
    pub fn dq_frame(&mut self) -> Option<[u32; DEPTH]> {
        if self.is_empty() {
            None
        } else {
            let playbuf = [u32; DEPTH];
            for (src, &dst) in self.buffer[self.rd_frame].iter().zip(playbuf.iter_mut()) {
                *dst = src;
            }
            self.rd_frame = ((self.rd_frame + 1) % FRAMES);
            Some(playbuf)
        }
    }
}
