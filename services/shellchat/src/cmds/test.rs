use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String;
use xous::{MessageEnvelope, Message};
use llio::I2cStatus;

use codec::*;
use spectrum_analyzer::{FrequencyLimit, FrequencySpectrum, samples_fft_to_spectrum};
use spectrum_analyzer::windows::hann_window;
use rtc::{DateTime, Weekday};
use std::sync::{Arc, Mutex};
use std::thread;
use keyboard::RowCol;
use std::collections::HashMap;
use gam::modal::*;
use num_traits::*;
use core::fmt::Write;

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
    //rtc: rtc::Rtc,
    start_time: Option<DateTime>,
    end_time: Option<DateTime>,
    start_elapsed: Option<u64>,
    end_elapsed: Option<u64>,
    kbd: Arc<Mutex<keyboard::Keyboard>>,
    jtag: jtag::Jtag,
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
            //rtc: rtc::Rtc::new(&xns).unwrap(),
            start_time: None,
            end_time: None,
            start_elapsed: None,
            end_elapsed: None,
            kbd: Arc::new(Mutex::new(keyboard::Keyboard::new(&xns).unwrap())),
            jtag: jtag::Jtag::new(&xns).unwrap(),
        }
    }
}

const SAMPLE_RATE_HZ: f32 = 8000.0;
// note to self: A4 = 440.0, E4 = 329.63, C4 = 261.63

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
enum TestOp {
    KeyCode,
    UxGutter,
    ModalRedraw,
    ModalKeys,
    ModalDrop,
}


impl<'a> ShellCmdApi<'a> for Test {
    cmd_api!(test);

    fn process(&mut self, args: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        const SENTINEL: &'static str = "|TSTR";

        self.state += 1;
        let mut ret = String::<1024>::new();
        write!(ret, "Test has run {} times.", self.state).unwrap();

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

                    let vccint = env.llio.adc_vccint().unwrap() as f32 / 1365.0;
                    if vccint < 0.92 || vccint > 0.98 {
                        log::info!("{}|VCCINT|FAIL|{}", SENTINEL, vccint);
                    } else {
                        log::info!("{}|VCCINT|PASS|{}", SENTINEL, vccint);
                    }
                    let vccaux = env.llio.adc_vccaux().unwrap() as f32 / 1365.0;
                    if vccaux < 1.71 || vccaux > 1.89 {
                        log::info!("{}|VCCAUX|FAIL|{}", SENTINEL, vccaux);
                    } else {
                        log::info!("{}|VCCAUX|PASS|{}", SENTINEL, vccaux);
                    }
                    let vccbram = env.llio.adc_vccbram().unwrap() as f32 / 1365.0;
                    if vccbram < 0.92 || vccbram > 0.98 {
                        log::info!("{}|VCCBRAM|FAIL|{}", SENTINEL, vccbram);
                    } else {
                        log::info!("{}|VCCBRAM|PASS|{}", SENTINEL, vccbram);
                    }

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

                    let mut av_pass = [true, true];
                    let mut ro_pass = true;
                    let mut av_excurs: [u32; 2] = [0; 2];
                    let mut ht = env.trng.get_health_tests().unwrap();
                    for _ in 0..3 { // run the test 3 times
                        av_excurs = [
                            (((ht.av_excursion[0].max as f64 - ht.av_excursion[0].min as f64) / 4096.0) * 1000.0) as u32,
                            (((ht.av_excursion[1].max as f64 - ht.av_excursion[1].min as f64) / 4096.0) * 1000.0) as u32,
                        ];
                        // 78mv minimum excursion requirement for good entropy generation
                        if av_excurs[0] < 78 { av_pass[0] = false; }
                        if av_excurs[1] < 78 { av_pass[1] = false; }
                        for core in ht.ro_miniruns.iter() {
                            for (bin, &val) in core.run_count.iter().enumerate() {
                                match bin {
                                    0 => {
                                        if val < 440 || val > 584 { ro_pass = false; }
                                    },
                                    1 => {
                                        if val < 193 || val > 318 { ro_pass = false; }
                                    },
                                    2 => {
                                        if val < 80 || val > 175 { ro_pass = false; }
                                    },
                                    3 => {
                                        if val < 29 || val > 99 { ro_pass = false; }
                                    }
                                    _ => {
                                        log::error!("internal error: too many bins in trng test!");
                                    }
                                }
                            }
                        }
                        const ROUNDS: usize = 16; // pump a bunch of data to trigger another trng buffer refill, resetting the stats
                        for _ in 0..ROUNDS {
                            let mut buf: [u32; 1024] = [0; 1024];
                            env.trng.fill_buf(&mut buf).unwrap();
                            log::debug!("pump samples: {:x}, {:x}, {:x}", buf[0], buf[512], buf[1023]); // prevent the pump values from being optimized out
                        }
                        ht = env.trng.get_health_tests().unwrap();
                    }
                    if av_pass[0] && av_pass[1] && ro_pass {
                        log::info!("{}|TRNG|PASS|{}|{}|{}|{}|{}|{}|", SENTINEL, av_excurs[0], av_excurs[1],
                            ht.ro_miniruns[0].run_count[0],
                            ht.ro_miniruns[0].run_count[1],
                            ht.ro_miniruns[0].run_count[2],
                            ht.ro_miniruns[0].run_count[3],
                        );
                    }
                    if !av_pass[0] {
                        log::info!("{}|TRNG|FAIL|AV0|{}|", SENTINEL, av_excurs[0]);
                    }
                    if !av_pass[1] {
                        log::info!("{}|TRNG|FAIL|AV1|{}|", SENTINEL, av_excurs[1]);
                    }
                    if !ro_pass {
                        log::info!("{}|TRNG|FAIL|RO|{}|{}|{}|{}|", SENTINEL,
                            ht.ro_miniruns[0].run_count[0],
                            ht.ro_miniruns[0].run_count[1],
                            ht.ro_miniruns[0].run_count[2],
                            ht.ro_miniruns[0].run_count[3],
                        );
                    }

                    let ut = env.com.get_ec_uptime().unwrap();
                    env.llio.ec_reset().unwrap();

                    env.ticktimer.sleep_ms(4000).unwrap(); // wait so we have some realistic delta on the datetime function
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

                    env.com.link_reset().unwrap();
                    env.com.reseed_ec_trng().unwrap();
                    let ut_after = env.com.get_ec_uptime().unwrap();

                    if ut < ut_after || ut_after > 4200 {
                        log::info!("{}|ECRESET|FAIL|{}|{}|", SENTINEL, ut, ut_after);
                    } else {
                        log::info!("{}|ECRESET|PASS|{}|{}|", SENTINEL, ut, ut_after);
                    }

                    log::info!("{}|DONE|", SENTINEL);
                    write!(ret, "Factory test script has run, check serial terminal for output").unwrap();
                    env.llio.wfi_override(false).unwrap();
                }
                "bl2" => {
                    env.llio.wfi_override(true).unwrap();
                    env.com.set_backlight(255, 255).unwrap();
                    log::info!("{}|BL2|", SENTINEL);
                }
                "bl1" => {
                    env.llio.wfi_override(true).unwrap();
                    env.com.set_backlight(180, 180).unwrap();
                    log::info!("{}|BL1|", SENTINEL);
                }
                "bl0" => {
                    env.llio.wfi_override(true).unwrap();
                    env.com.set_backlight(0, 0).unwrap();
                    log::info!("{}|BL0|", SENTINEL);
                }
                "wfireset" => {
                    env.llio.wfi_override(false).unwrap();
                    log::info!("{}|WFIRESET|", SENTINEL);
                }
                "wfioff" => {
                    env.llio.wfi_override(true).unwrap();
                    log::info!("{}|WFIOFF|", SENTINEL);
                }
                "vibe" => {
                    env.llio.vibe(llio::VibePattern::Long).unwrap();
                    log::info!("{}|VIBE|", SENTINEL);
                }
                "booston" => {
                    env.llio.boost_on(true).unwrap();
                    env.com.set_boost(true).unwrap();
                    log::info!("{}|BOOSTON|", SENTINEL);
                }
                "boostoff" => {
                    env.com.set_boost(false).unwrap();
                    env.ticktimer.sleep_ms(50).unwrap();
                    env.llio.boost_on(false).unwrap();
                    log::info!("{}|BOOSTOFF|", SENTINEL);
                }
                "kill" => {
                    log::info!("{}|KILL|", SENTINEL);
                    env.ticktimer.sleep_ms(500).unwrap();
                    env.llio.self_destruct(0x2718_2818).unwrap();
                    env.llio.self_destruct(0x3141_5926).unwrap();
                    env.ticktimer.sleep_ms(100).unwrap();
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
                "oqc" => {
                    env.llio.wfi_override(true).unwrap();
                    //xous::rsyscall(xous::SysCall::IncreaseHeap(65536, xous::MemoryFlags::R | xous::MemoryFlags::W)).expect("couldn't increase our heap");
                    /* // temp clear so renode can run
                    ret.clear();
                    if 0x362f093 != self.jtag.get_id().unwrap() {
                        write!(ret, "FAIL: JTAG self access").unwrap();
                        return Ok(Some(ret));
                    }
                    env.com.set_backlight(196, 196).unwrap();
                    */
                    log::info!("building modal");
                    const OQC_SERVER: &str = "_OQC server_";
                    let kbdtest_sid = env.xns.register_name(OQC_SERVER, Some(1)).unwrap();
                    let test_cid = xous::connect(kbdtest_sid).unwrap();
                    let mut test_action = Notification::new(
                        test_cid,
                        TestOp::UxGutter.to_u32().unwrap()
                    );
                    test_action.manual_dismiss = false;
                    let mut test_modal = Modal::new(
                        "test modal",
                        ActionType::Notification(test_action),
                        Some("Keyboard Test"),
                        Some("test"),
                        GlyphStyle::Small,
                        8
                    );
                    log::info!("making helper");
                    test_modal.spawn_helper(kbdtest_sid, test_modal.sid,
                        TestOp::ModalRedraw.to_u32().unwrap(),
                        TestOp::ModalKeys.to_u32().unwrap(),
                        TestOp::ModalDrop.to_u32().unwrap(),
                    );
                    log::info!("raising modal");
                    test_modal.activate();
                    test_modal.redraw();

                    log::info!("spawning helper thread");
                    let handle = thread::spawn({
                        log::info!("clone kbd");
                        let kbd = Arc::clone(&self.kbd);
                        log::info!("move kbd");
                        move || {
                            log::info!("oqc server starting");
                            kbd.lock().unwrap()
                                .register_raw_listener(
                                    OQC_SERVER,
                                    TestOp::KeyCode.to_usize().unwrap()
                                );
                            let mut count = 0;
                            let mut remaining = populate_vectors();

                            let mut bot_str = String::<1024>::new();
                            loop {
                                let msg = xous::receive_message(kbdtest_sid).unwrap();
                                log::info!("got msg: {:?}", msg);
                                match FromPrimitive::from_usize(msg.body.id()) {
                                    Some(TestOp::KeyCode) => xous::msg_scalar_unpack!(msg, is_keyup, r, c, _, {
                                        log::info!("rawstates");
                                        if is_keyup == 0 { // only worry about keydowns
                                            let key = RowCol::new(r as u8, c as u8);
                                            match remaining.get(&key) {
                                                Some(_hit) => {
                                                    log::info!("got {}", map_codes(key));
                                                    remaining.insert(key, true);
                                                    write!(bot_str, "{}", map_codes(key)).unwrap();
                                                },
                                                None => log::warn!("got unexpected r/c: {:?}", key),
                                            };
                                            log::info!("update modal");
                                            test_modal.modify(None, None, false,
                                                Some(bot_str.as_str().unwrap()), false, None);
                                            log::info!("redraw modal");
                                            test_modal.redraw();
                                            xous::yield_slice();
                                        }
                                        log::info!("epilogue");
                                        // iterate and see if all keys have been hit
                                        let mut finished = true;
                                        for &vals in remaining.values() {
                                            if vals == false {
                                                log::info!("short-circuit remaining eval");
                                                finished = false;
                                                break;
                                            }
                                        }
                                        if finished {
                                            log::info!("all keys hit, exiting");
                                            break;
                                        }

                                        count += 1;
                                        if count > 10 {
                                            log::info!("test threshold hit, exiting");
                                            break;
                                        }
                                        //xous::return_scalar(msg.sender, 0).unwrap();
                                    }),
                                    Some(TestOp::UxGutter) => {
                                        log::info!("gutter");
                                        // an intentional NOP for UX actions that require a destintation but need no action
                                    },
                                    Some(TestOp::ModalRedraw) => {
                                        log::info!("modal redraw handler");
                                        test_modal.redraw();
                                    },
                                    Some(TestOp::ModalKeys) => xous::msg_scalar_unpack!(msg, _k1, _k2, _k3, _k4, {
                                        log::info!("modal keys message, ignoring");
                                        // ignore keys, we have our own key routine
                                    }),
                                    Some(TestOp::ModalDrop) => {
                                        log::error!("test modal quit unexpectedly");
                                    }
                                    _ => {
                                        log::error!("unrecognized message: {:?}", msg);
                                    }
                                }
                            }
                            test_modal.gam.relinquish_focus().unwrap();
                        }
                    });
                    handle.join().unwrap();

                    write!(ret, "CHECK: was backlight on?\nIf so, then PASS").unwrap();
                    env.com.set_backlight(0, 0).unwrap();
                    env.llio.wfi_override(false).unwrap();
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

fn populate_vectors() -> HashMap::<RowCol, bool> {
    let mut vectors = HashMap::<RowCol, bool>::new();
    vectors.insert(RowCol::new(0, 0), false);
    vectors.insert(RowCol::new(0, 1), false);
    vectors.insert(RowCol::new(0, 2), false);
    vectors.insert(RowCol::new(0, 3), false);
    vectors.insert(RowCol::new(0, 4), false);
    vectors.insert(RowCol::new(4, 5), false);
    vectors.insert(RowCol::new(4, 6), false);
    vectors.insert(RowCol::new(4, 7), false);
    vectors.insert(RowCol::new(4, 8), false);
    vectors.insert(RowCol::new(4, 9), false);
    vectors.insert(RowCol::new(1, 0), false);
    vectors.insert(RowCol::new(1, 1), false);
    vectors.insert(RowCol::new(1, 2), false);
    vectors.insert(RowCol::new(1, 3), false);
    vectors.insert(RowCol::new(1, 4), false);
    vectors.insert(RowCol::new(5, 5), false);
    vectors.insert(RowCol::new(5, 6), false);
    vectors.insert(RowCol::new(5, 7), false);
    vectors.insert(RowCol::new(5, 8), false);
    vectors.insert(RowCol::new(5, 9), false);
    vectors.insert(RowCol::new(2, 0), false);
    vectors.insert(RowCol::new(2, 1), false);
    vectors.insert(RowCol::new(2, 2), false);
    vectors.insert(RowCol::new(2, 3), false);
    vectors.insert(RowCol::new(2, 4), false);
    vectors.insert(RowCol::new(6, 5), false);
    vectors.insert(RowCol::new(6, 6), false);
    vectors.insert(RowCol::new(6, 7), false);
    vectors.insert(RowCol::new(6, 8), false);
    vectors.insert(RowCol::new(6, 9), false);
    vectors.insert(RowCol::new(3, 0), false);
    vectors.insert(RowCol::new(3, 1), false);
    vectors.insert(RowCol::new(3, 2), false);
    vectors.insert(RowCol::new(3, 3), false);
    vectors.insert(RowCol::new(3, 4), false);
    vectors.insert(RowCol::new(7, 5), false);
    vectors.insert(RowCol::new(7, 6), false);
    vectors.insert(RowCol::new(7, 7), false);
    vectors.insert(RowCol::new(7, 8), false);
    vectors.insert(RowCol::new(7, 9), false);
    vectors.insert(RowCol::new(8, 5), false);
    vectors.insert(RowCol::new(8, 6), false);
    vectors.insert(RowCol::new(8, 7), false);
    vectors.insert(RowCol::new(8, 8), false);
    vectors.insert(RowCol::new(8, 9), false);
    vectors.insert(RowCol::new(8, 0), false);
    vectors.insert(RowCol::new(8, 1), false);
    vectors.insert(RowCol::new(3, 8), false);
    vectors.insert(RowCol::new(3, 9), false);
    vectors.insert(RowCol::new(8, 3), false);
    vectors.insert(RowCol::new(3, 6), false);
    vectors.insert(RowCol::new(6, 4), false);
    vectors.insert(RowCol::new(8, 2), false);
    vectors.insert(RowCol::new(5, 2), false);

    vectors
}

fn map_codes(code: RowCol) -> &'static str {
    let rc = (code.r, code.c);

    match rc {
        (0, 0) => "1",
        (0, 1) => "2",
        (0, 2) => "3",
        (0, 3) => "4",
        (0, 4) => "5",
        (4, 5) => "6",
        (4, 6) => "7",
        (4, 7) => "8",
        (4, 8) => "9",
        (4, 9) => "0",
        (1, 0) => "q",
        (1, 1) => "w",
        (1, 2) => "e",
        (1, 3) => "r",
        (1, 4) => "t",
        (5, 5) => "y",
        (5, 6) => "u",
        (5, 7) => "i",
        (5, 8) => "o",
        (5, 9) => "p",
        (2, 0) => "a",
        (2, 1) => "s",
        (2, 2) => "d",
        (2, 3) => "f",
        (2, 4) => "g",
        (6, 5) => "h",
        (6, 6) => "j",
        (6, 7) => "k",
        (6, 8) => "l",
        (6, 9) => "BS",
        (3, 0) => "!",
        (3, 1) => "z",
        (3, 2) => "x",
        (3, 3) => "c",
        (3, 4) => "v",
        (7, 5) => "b",
        (7, 6) => "n",
        (7, 7) => "m",
        (7, 8) => "?",
        (7, 9) => "↩️",
        (8, 5) => "LS",
        (8, 6) => ",",
        (8, 7) => "SP",
        (8, 8) => ".",
        (8, 9) => "RS",
        (8, 0) => "F1",
        (8, 1) => "F2",
        (3, 8) => "F3",
        (3, 9) => "F4",
        (8, 3) => "⬅️",
        (3, 6) => "➡️",
        (6, 4) => "⬆️",
        (8, 2) => "⬇️",
        (5, 2) => "MID",
        _ => "ERR!",
    }
}