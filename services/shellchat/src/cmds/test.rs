use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String;
use xous::MessageEnvelope;

use codec::*;
use spectrum_analyzer::{samples_fft_to_spectrum, FrequencyLimit};
use spectrum_analyzer::windows::hann_window;

#[derive(Debug)]
pub struct Test {
    state: u32,
    codec: codec::Codec,
    recbuf: xous::MemoryRange,
    callback_id: Option<u32>,
    callback_conn: u32,
    framecount: u32,
    play_sample: f32, // count of play samples generated. in f32 to avoid int<->f32 conversions
    rec_sample: usize, // count of record samples recorded. in usize because we're not doing f32 wave table computations on this
}
impl Test {
    pub fn new(xns: &xous_names::XousNames) -> Self {
        let codec = codec::Codec::new(xns).unwrap();

        let recbuf = xous::syscall::map_memory(
            None,
            None,
            0x8000,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        ).expect("couldn't allocate record buffer");

        Test {
            codec,
            recbuf,
            state: 0,
            callback_id: None,
            callback_conn: xns.request_connection_blocking(crate::SERVER_NAME_SHELLCHAT).unwrap(),
            framecount: 0,
            play_sample: 0.0,
            rec_sample: 0,
        }
    }
}

const SAMPLE_RATE_HZ: f32 = 8000.0;

impl<'a> ShellCmdApi<'a> for Test {
    cmd_api!(test);

    fn process(&mut self, args: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;

        const SENTINEL: &'static str = "|TSTR";

        self.state += 1;
        let mut ret = String::<1024>::new();
        write!(ret, "Test has run {} times.\n", self.state).unwrap();

        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "factory" => {
                    // set uart MUX, and turn off WFI so UART reports are "clean" (no stuck characters when CPU is in WFI)
                    env.llio.set_uart_mux(llio::UartType::Log).unwrap();
                    env.llio.wfi_override(true).unwrap();
                    let (x, y, z, id) = env.com.gyro_read_blocking().unwrap();
                    log::info!("{}|GYRO|{}|{}|{}|{}|", SENTINEL, x, y, z, id);
                    let (wf_maj, wf_min, wf_rev) = env.com.get_wf200_fw_rev().unwrap();
                    log::info!("{}|WF200REV|{}|{}|{}|", SENTINEL, wf_maj, wf_min, wf_rev);
                    let (ec_rev, ec_dirty) =  env.com.get_ec_git_rev().unwrap();
                    log::info!("{}|ECREV|{:x}|{:?}|", SENTINEL, ec_rev, ec_dirty);
                    let morestats = env.com.get_more_stats().unwrap();
                    log::info!("{}|BATTSTATS|{:?}|", SENTINEL, morestats);
                    let (usbcc_event, usbcc_regs, usbcc_rev) = env.com.poll_usb_cc().unwrap();
                    log::info!("{}|USBCC|{:?}|{:?}|{}|", SENTINEL, usbcc_event, usbcc_regs, usbcc_rev);

                    log::info!("{}|DONE|", SENTINEL);
                    write!(ret, "Factory test script has run, check serial terminal for output").unwrap();
                    env.llio.wfi_override(false).unwrap();
                }
                "audio" => {
                    self.codec.setup_8k_stream().expect("couldn't set the CODEC to expected defaults");
                    env.ticktimer.sleep_ms(50).unwrap();

                    if self.callback_id.is_none() {
                        let cb_id = env.register_handler(String::<256>::from_str(self.verb()));
                        log::trace!("hooking frame callback with ID {}", cb_id);
                        self.codec.hook_frame_callback(cb_id, self.callback_conn).unwrap(); // any non-handled IDs get routed to our callback port
                        self.callback_id = Some(cb_id);
                    }

                    self.play_sample = 0.0;
                    self.rec_sample = 0;

                    log::info!("starting playback");
                    self.codec.resume().unwrap();

                    env.ticktimer.sleep_ms(4000).unwrap();

                    self.codec.pause().unwrap(); // this should stop callbacks from occurring too.
                    write!(ret, "Playback stopped at {} frames.", self.framecount).unwrap();
                    self.framecount = 0;
                    self.play_sample = 0.0;
                    self.rec_sample = 0;
                    self.codec.power_off().unwrap();

                    // now do FFT analysis on the sample buffer
                    // analyze one channel at a time
                    let mut left_samples = Vec::<f32>::new();
                    let mut right_samples = Vec::<f32>::new();
                    for &sample in self.recbuf.as_slice::<u32>().iter() {
                        left_samples.push( ((sample & 0xFFFF) as i16) as f32 );
                        right_samples.push( (((sample >> 16) & 0xFFFF) as i16) as f32 );
                    }
                    let hann_left = hann_window(&left_samples);
                    let hann_right = hann_window(&right_samples);
                    let spectrum_left = samples_fft_to_spectrum(
                        &hann_left,
                        SAMPLE_RATE_HZ as _,
                        FrequencyLimit::All,
                        None,
                        None
                    );
                    let spectrum_right = samples_fft_to_spectrum(
                        &hann_right,
                        SAMPLE_RATE_HZ as _,
                        FrequencyLimit::All,
                        None,
                        None
                    );
                    log::info!("left");
                    for (fr, fr_val) in spectrum_left.data().iter() {
                        log::info!("{}Hz => {}", fr, fr_val)
                    }
                    log::info!("right");
                    for (fr, fr_val) in spectrum_right.data().iter() {
                        log::info!("{}Hz => {}", fr, fr_val)
                    }
                    /*
                    Left off notes:
                      - simplify to single tones played for 3 seconds
                      - one tone each for hp left, hp right, and speaker
                      - analysis only needs to happen on one channel, as mic is mono. pick one, see if it works.
                      - in addition to frequency analysis, need to pick amplitude threshold
                      - need to integrate spectrum and calculate %age of total power in a given bin for a pass/fail criteria
                     */
                }
                "devboot" => {
                    env.gam.set_devboot(true).unwrap();
                    write!(ret, "devboot on").unwrap();
                }
                "devbootoff" => {
                    // this should do nothing if devboot was already set
                    env.gam.set_devboot(false).unwrap();
                    write!(ret, "devboot off").unwrap();
                }
                _ => {
                    () // do nothing
                }
            }

        }
        Ok(Some(ret))
    }

    fn callback(&mut self, msg: &MessageEnvelope, _env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        const OMEGA1: f32 = 440.0 * 2.0 * std::f32::consts::PI / SAMPLE_RATE_HZ;  // A4
        const OMEGA2: f32 = 329.63 * 2.0 * std::f32::consts::PI / SAMPLE_RATE_HZ; // E4
        const OMEGA3: f32 = 261.63 * 2.0 * std::f32::consts::PI / SAMPLE_RATE_HZ; // C4
        const AMPLITUDE: f32 = 0.707;

        log::debug!("audio callback");
        xous::msg_scalar_unpack!(msg, free_play, _avail_rec, _, _, {
            log::debug!("{} extending playback", free_play);
            let mut frames: FrameRing = FrameRing::new();
            let frames_to_push = if frames.writeable_count() < free_play {
                frames.writeable_count()
            } else {
                free_play
            };
            self.framecount += frames_to_push as u32;
            log::debug!("f{} p{}", self.framecount, frames_to_push);
            for _ in 0..frames_to_push {
                let mut frame: [u32; codec::FIFO_DEPTH] = [ZERO_PCM as u32 | (ZERO_PCM as u32) << 16; codec::FIFO_DEPTH];
                // put the "expensive" f32 comparison outside the cosine wave table computation loop
                if self.play_sample < SAMPLE_RATE_HZ { // 1 second A4
                    for sample in frame.iter_mut() {
                        // left channel, A4 note
                        let raw_sine: i16 = (AMPLITUDE * f32::cos( self.play_sample * OMEGA1 ) * i16::MAX as f32) as i16;
                        *sample = raw_sine as u32 | (ZERO_PCM as u32) << 16;
                        self.play_sample += 1.0;
                    }
                } else if self.play_sample < SAMPLE_RATE_HZ * 2.0 {
                    for sample in frame.iter_mut() { // next second E4
                        // right channel, E4 note
                        let raw_sine: i16 = (AMPLITUDE * f32::cos( self.play_sample * OMEGA2 ) * i16::MAX as f32) as i16;
                        *sample = ZERO_PCM as u32 | (raw_sine as u32) << 16;
                        self.play_sample += 1.0;
                    }
                } else {
                    for sample in frame.iter_mut() { // rest C4
                        // both channels, C4 note
                        let raw_sine: i16 = (AMPLITUDE * f32::cos( self.play_sample * OMEGA3 ) * i16::MAX as f32) as i16;
                        *sample = raw_sine as u32 | (raw_sine as u32) << 16;
                        self.play_sample += 1.0;
                    }
                }

                frames.nq_frame(frame).unwrap();

            }
            self.codec.swap_frames(&mut frames).unwrap();

            let rec_samples = self.recbuf.as_slice_mut::<u32>();
            let rec_len = rec_samples.len();
            loop {
                if let Some(frame) = frames.dq_frame() {
                    for sample in rec_samples.iter_mut() {
                        *sample = frame[self.rec_sample];
                        // increment and wrap around on overflow
                        // we should be sampling a continuous tone, so we'll get a small phase discontinutity once in the buffer.
                        // should be no problem for the analysis phase.
                        self.rec_sample += 1;
                        if self.rec_sample > rec_len {
                            self.rec_sample = 0;
                        }
                    }
                } else {
                    break;
                };
            }
        });
        Ok(None)
    }
}
