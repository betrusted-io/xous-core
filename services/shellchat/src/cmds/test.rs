use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String;
use xous::{MessageEnvelope, Message};
use llio::I2cStatus;

use codec::*;
use spectrum_analyzer::{FrequencyLimit, FrequencySpectrum, samples_fft_to_spectrum};
use spectrum_analyzer::windows::hann_window;
use rtc::{DateTime, Weekday};

#[derive(Debug)]
pub struct Test {
    state: u32,
    // audio
    codec: codec::Codec,
    recbuf: xous::MemoryRange,
    callback_id: Option<u32>,
    callback_conn: u32,
    framecount: u32,
    play_sample: f32, // count of play samples generated. in f32 to avoid int<->f32 conversions
    rec_sample: usize, // count of record samples recorded. in usize because we're not doing f32 wave table computations on this
    left_play: bool,
    right_play: bool,
    speaker_play: bool,
    freq: f32,
    // rtc
    start_time: Option<DateTime>,
    end_time: Option<DateTime>,
    start_elapsed: Option<u64>,
    end_elapsed: Option<u64>,
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

        let callback_conn = xns.request_connection_blocking(crate::SERVER_NAME_SHELLCHAT).unwrap();

        Test {
            codec,
            recbuf,
            state: 0,
            callback_id: None,
            callback_conn,
            framecount: 0,
            play_sample: 0.0,
            rec_sample: 0,
            left_play: true,
            right_play: true,
            speaker_play: true,
            freq: 440.0,
            start_time: None,
            end_time: None,
            start_elapsed: None,
            end_elapsed: None,
        }
    }
}

const SAMPLE_RATE_HZ: f32 = 8000.0;
// note to self: A4 = 440.0, E4 = 329.63, C4 = 261.63

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
                    // force a specified time to make sure the elapsed time computation later on works
                    if !rtc_set(&mut env.llio, 0, 0, 10, 1, 6, 21) {
                        log::info!("{}|RTC|FAIL|SET|", SENTINEL);
                    }

                    self.start_elapsed = Some(env.ticktimer.elapsed_ms());
                    self.start_time = rtc_get(&mut env.llio);

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

                    env.ticktimer.sleep_ms(3000).unwrap(); // wait so we have some realistic delta on the datetime function
                    self.end_elapsed = Some(env.ticktimer.elapsed_ms());
                    self.end_time = rtc_get(&mut env.llio);

                    let exact_time_secs = ((self.end_elapsed.unwrap() - self.start_elapsed.unwrap()) / 1000) as i32;
                    if let Some(end_dt) = self.end_time {
                        if let Some(start_dt) = self.start_time {
                            // this method of computation fails if the test was started just before midnight
                            // on the last day of the month and completes just after. we set the time so we ensure this
                            // can't happen.
                            let end_secs =
                                end_dt.days as i32 * 24 * 60 * 60 +
                                end_dt.hours as i32 * 60 * 60 +
                                end_dt.minutes as i32 * 60 +
                                end_dt.seconds as i32;
                            let start_secs =
                                start_dt.days as i32 * 24 * 60 * 60 +
                                start_dt.hours as i32 * 60 * 60 +
                                start_dt.minutes as i32 * 60 +
                                start_dt.seconds as i32;
                            let elapsed_secs = end_secs - start_secs;

                            let delta = exact_time_secs - elapsed_secs;
                            if delta.abs() > 2 {
                                log::info!("{}|RTC|FAIL|{}|{}|", SENTINEL, exact_time_secs, elapsed_secs);
                            } else {
                                log::info!("{}|RTC|PASS|{}|{}|", SENTINEL, exact_time_secs, elapsed_secs);
                            }
                        } else {
                            log::info!("{}|RTC|FAIL|NO_START|", SENTINEL);
                        }
                    } else {
                        log::info!("{}|RTC|FAIL|NO_END|", SENTINEL);
                    }

                    log::info!("{}|DONE|", SENTINEL);
                    write!(ret, "Factory test script has run, check serial terminal for output").unwrap();
                    env.llio.wfi_override(false).unwrap();
                }
                "astart" => {
                    self.freq = if let Some(freq_str) = tokens.next() {
                        match freq_str.parse::<f32>() {
                            Ok(f) => f,
                            Err(_) => 440.0,
                        }
                    } else {
                        440.0
                    };
                    if let Some(channel_str) = tokens.next() {
                        match channel_str {
                            "left" => {self.left_play = true; self.right_play = false; self.speaker_play = false;},
                            "right" => {self.right_play = true; self.left_play = false; self.speaker_play = false;},
                            "speaker" => {self.left_play = true; self.right_play = false; self.speaker_play = true;},
                            _ => {self.left_play = true; self.right_play = true; self.speaker_play = true;}
                        }
                    } else {
                        self.left_play = true;
                        self.right_play = true;
                        self.speaker_play = true;
                    }
                    self.codec.setup_8k_stream().expect("couldn't set the CODEC to expected defaults");
                    env.ticktimer.sleep_ms(50).unwrap();

                    if self.speaker_play {
                        self.codec.set_speaker_volume(VolumeOps::RestoreDefault, None).unwrap();
                    } else {
                        self.codec.set_speaker_volume(VolumeOps::Mute, None).unwrap();
                    }
                    if self.left_play || self.right_play {
                        self.codec.set_headphone_volume(VolumeOps::RestoreDefault, None).unwrap();
                    } else {
                        self.codec.set_headphone_volume(VolumeOps::Mute, None).unwrap();
                    }

                    if self.callback_id.is_none() {
                        let cb_id = env.register_handler(String::<256>::from_str(self.verb()));
                        log::trace!("hooking frame callback with ID {}", cb_id);
                        self.codec.hook_frame_callback(cb_id, self.callback_conn).unwrap(); // any non-handled IDs get routed to our callback port
                        self.callback_id = Some(cb_id);
                    }

                    self.play_sample = 0.0;
                    self.rec_sample = 0;

                    self.codec.resume().unwrap();
                    log::info!("{}|ASTART|{}|{}|{}|", SENTINEL, self.freq, self.left_play, self.right_play);

                }
                "astop" => {
                    self.codec.pause().unwrap(); // this should stop callbacks from occurring too.
                    write!(ret, "Playback stopped at {} frames.", self.framecount).unwrap();
                    self.framecount = 0;
                    self.play_sample = 0.0;
                    self.rec_sample = 0;
                    self.codec.power_off().unwrap();

                    // now do FFT analysis on the sample buffer
                    // analyze one channel at a time
                    let mut right_samples = Vec::<f32>::new();
                    let recslice = self.recbuf.as_slice::<u32>();
                    for &sample in recslice[recslice.len()-4096..].iter() {
                        right_samples.push( ((sample & 0xFFFF) as i16) as f32 );
                        //left_samples.push( (((sample >> 16) & 0xFFFF) as i16) as f32 ); // reminder of how to extract the right channel
                    }
                    // only one channel is considered, because in reality the left is just a copy of the right, as
                    // we are taking a mono microphone signal and mixing it into both ADCs

                    let db = db_compute(&right_samples);
                    let hann_right = hann_window(&right_samples);
                    let spectrum_right = samples_fft_to_spectrum(
                        &hann_right,
                        SAMPLE_RATE_HZ as _,
                        FrequencyLimit::All,
                        None,
                        None
                    );
                    asciiplot(&spectrum_right);
                    let ratio = analyze(&spectrum_right, self.freq);
                    // ratio typical range 0.35 (speaker) to 3.5 (headphones)
                    // speaker has a wider spectrum because we don't have a filter on the PWM, so there are sampling issues feeding it back into the mic
                    // db typical <10 (silence) to 78 (full amplitude)
                    if (ratio > 0.25) && (db > 60.0) {
                        log::info!("{}|ARESULT|PASS|{}|{}|{}|{}|{}|", SENTINEL, ratio, db, self.freq, self.left_play, self.right_play);
                    } else {
                        log::info!("{}|ARESULT|FAIL|{}|{}|{}|{}|{}|", SENTINEL, ratio, db, self.freq, self.left_play, self.right_play);
                    }
                    log::debug!("off-target 1 {}", analyze(&spectrum_right, 329.63));
                    log::debug!("off-target 2 {}", analyze(&spectrum_right, 261.63));
                    log::info!("{}|ASTOP|", SENTINEL);
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
        const AMPLITUDE: f32 = 0.8;

        match &msg.body {
            Message::Scalar(xous::ScalarMessage{id: _, arg1: free_play, arg2: _avail_rec, arg3: _, arg4: _}) => {
                log::debug!("{} extending playback", free_play);
                let mut frames: FrameRing = FrameRing::new();
                let frames_to_push = if frames.writeable_count() < *free_play {
                    frames.writeable_count()
                } else {
                    *free_play
                };
                self.framecount += frames_to_push as u32;
                log::debug!("f{} p{}", self.framecount, frames_to_push);
                for _ in 0..frames_to_push {
                    let mut frame: [u32; codec::FIFO_DEPTH] = [ZERO_PCM as u32 | (ZERO_PCM as u32) << 16; codec::FIFO_DEPTH];
                    // put the "expensive" f32 comparison outside the cosine wave table computation loop
                    let omega = self.freq * 2.0 * std::f32::consts::PI / SAMPLE_RATE_HZ;
                    for sample in frame.iter_mut() {
                        let raw_sine: i16 = (AMPLITUDE * f32::cos( self.play_sample * omega ) * i16::MAX as f32) as i16;
                        let left = if self.left_play { raw_sine as u16 } else { ZERO_PCM };
                        let right = if self.right_play { raw_sine as u16 } else { ZERO_PCM };
                        *sample = right as u32 | (left as u32) << 16;
                        self.play_sample += 1.0;
                    }

                    frames.nq_frame(frame).unwrap();

                }
                self.codec.swap_frames(&mut frames).unwrap();

                let rec_samples = self.recbuf.as_slice_mut::<u32>();
                let rec_len = rec_samples.len();
                loop {
                    if let Some(frame) = frames.dq_frame() {
                        for &sample in frame.iter() {
                            rec_samples[self.rec_sample] = sample;
                            // increment and wrap around on overflow
                            // we should be sampling a continuous tone, so we'll get a small phase discontinutity once in the buffer.
                            // should be no problem for the analysis phase.
                            self.rec_sample += 1;
                            if self.rec_sample >= rec_len {
                                self.rec_sample = 0;
                            }
                        }
                    } else {
                        break;
                    };
                }
            },
            Message::Move(_mm) => {
                log::error!("received memory message when not expected")
            },
            _ => {
                log::error!("received unknown callback type")
            }
        }
        log::debug!("audio callback");
        Ok(None)
    }
}

// plus minus this amount for frequencing searching
// this is set less by our desired accuracy of the frequency
// than by the artifacts of the analysis: the FFT window has some "skirt" to it
// by a few Hz, and this ratio tries to capture about 90% of the power within the hanning window's skirt.
const ANALYSIS_DELTA: f32 = 10.0;
fn analyze(fs: &FrequencySpectrum, target: f32) -> f32 {
    let mut total_power: f32 = 0.0;
    let mut h1_power: f32 = 0.0;
    let mut h2_power: f32 = 0.0;
    let mut outside_power: f32 = 0.0;

    for &(freq, mag) in fs.data().iter() {
        let f = freq.val();
        let m = mag.val();
        total_power += m;
        if (f >= target - ANALYSIS_DELTA) && (f <= target + ANALYSIS_DELTA) {
            h1_power += m;
        } else if (f >= (target * 2.0 - ANALYSIS_DELTA)) && (f <= (target * 2.0 + ANALYSIS_DELTA)) {
            h2_power += m;
        } else if f >= ANALYSIS_DELTA { // don't count the DC offset
            outside_power += m;
        } else {
            log::debug!("ignoring {}Hz @ {}", f, m);
        }
    }
    log::debug!("h1: {}, h2: {}, outside: {}, total: {}", h1_power, h2_power, outside_power, total_power);

    let ratio = if outside_power > 0.0 {
        (h1_power + h2_power) / outside_power
    } else {
        1_000_000.0
    };
    ratio
}

fn db_compute(samps: &[f32]) -> f32 {
    let mut cum = 0.0;
    for &s in samps.iter() {
        cum += s;
    }
    let mid = cum / samps.len() as f32;
    cum = 0.0;
    for &s in samps.iter() {
        let a = s - mid;
        cum += a * a;
    }
    cum /= samps.len() as f32;
    let db = 10.0 * f32::log10(cum);
    db
}

fn asciiplot(fs: &FrequencySpectrum) {
    let (max_f, max_val) = fs.max();
    const LINES: usize = 100;
    const WIDTH: usize = 80;
    const MAX_F: usize = 1000;

    for f in (0..MAX_F).step_by(MAX_F / LINES) {
        let (freq, val) = fs.freq_val_closest(f as f32);
        let pos = ((val.val() / max_val.val()) * WIDTH as f32) as usize;
        log::debug!("{:>5} | {:width$}*", freq.val() as u32, " ", width = pos);
    }
    log::debug!("max freq: {}, max_val: {}", max_f, max_val);
}


/// convert binary to BCD
fn to_bcd(binary: u8) -> u8 {
    let mut lsd: u8 = binary % 10;
    if lsd > 9 {
        lsd = 9;
    }
    let mut msd: u8 = binary / 10;
    if msd > 9 {
        msd = 9;
    }

    (msd << 4) | lsd
}

fn to_binary(bcd: u8) -> u8 {
    (bcd & 0xf) + ((bcd >> 4) * 10)
}

fn to_weekday(bcd: u8) -> Weekday {
    match bcd {
        0 => Weekday::Sunday,
        1 => Weekday::Monday,
        2 => Weekday::Tuesday,
        3 => Weekday::Wednesday,
        4 => Weekday::Thursday,
        5 => Weekday::Friday,
        6 => Weekday::Saturday,
        _ => Weekday::Sunday,
    }
}
const ABRTCMC_I2C_ADR: u8 = 0x68;
const ABRTCMC_CONTROL3: u8 = 0x02;
const ABRTCMC_SECONDS: u8 = 0x3;

// vendor in the RTC code -- the RTC system was not architected to do our test sets
// in particular we need a synchronous callback on the date/time, which is not terribly useful in most other contexts
// so instead of burdening the OS with it, we just incorprate it specifically into this test function
fn rtc_set(llio: &mut llio::Llio, secs: u8, mins: u8, hours: u8, days: u8, months: u8, years: u8) -> bool {
    let mut txbuf: [u8; 8] = [0; 8];

    // convert enum to bitfields
    let d = 1;

    // make sure battery switchover is enabled, otherwise we won't keep time when power goes off
    txbuf[0] = 0;
    txbuf[1] = to_bcd(secs);
    txbuf[2] = to_bcd(mins);
    txbuf[3] = to_bcd(hours);
    txbuf[4] = to_bcd(days);
    txbuf[5] = to_bcd(d);
    txbuf[6] = to_bcd(months);
    txbuf[7] = to_bcd(years);

    match llio.i2c_write(ABRTCMC_I2C_ADR, ABRTCMC_CONTROL3, &txbuf, None) {
        Ok(status) => {
            match status {
                I2cStatus::ResponseWriteOk => true,
                I2cStatus::ResponseBusy => {log::error!("i2c server busy on RTC test function"); false},
                _ => {log::error!("try_send_i2c unhandled response: {:?}", status); false},
            }
        }
        _ => {log::error!("try_send_i2c unhandled error"); false}
    }
}
fn rtc_get(llio: &mut llio::Llio) -> Option<DateTime> {
    let mut rxbuf = [0; 7];
    match llio.i2c_read(ABRTCMC_I2C_ADR, ABRTCMC_SECONDS, &mut rxbuf, None) {
        Ok(status) => {
            match status {
                I2cStatus::ResponseReadOk => {
                    let dt = DateTime {
                        seconds: to_binary(rxbuf[0] & 0x7f),
                        minutes: to_binary(rxbuf[1] & 0x7f),
                        hours: to_binary(rxbuf[2] & 0x3f),
                        days: to_binary(rxbuf[3] & 0x3f),
                        weekday: to_weekday(rxbuf[4] & 0x7f),
                        months: to_binary(rxbuf[5] & 0x1f),
                        years: to_binary(rxbuf[6]),
                    };
                    Some(dt)
                },
                _ => None,
            }
        }
        _ => None
    }
}
