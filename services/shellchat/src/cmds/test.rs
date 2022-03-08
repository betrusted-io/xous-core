use crate::oqc_test::OqcOp;
use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String;
use xous::{MessageEnvelope, Message};
use llio::I2cStatus;

use codec::*;
use spectrum_analyzer::{FrequencyLimit, FrequencySpectrum, samples_fft_to_spectrum};
use spectrum_analyzer::windows::hann_window;
use llio::{DateTime, Weekday};
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering, AtomicU32};
use std::sync::Arc;
use num_traits::*;

static AUDIO_OQC: AtomicBool = AtomicBool::new(false);

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
    start_time: Option<DateTime>,
    end_time: Option<DateTime>,
    start_elapsed: Option<u64>,
    end_elapsed: Option<u64>,
    oqc_cid: Option<xous::CID>,
    kbd: Option<keyboard::Keyboard>,
    oqc_start: u64,
    #[cfg(any(target_os = "none", target_os = "xous"))]
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
            start_time: None,
            end_time: None,
            start_elapsed: None,
            end_elapsed: None,
            oqc_cid: None,
            kbd: Some(keyboard::Keyboard::new(&xns).unwrap()), // allocate and save for use in the oqc_tester, so that the xous_names table is fully allocated
            oqc_start: 0,
            #[cfg(any(target_os = "none", target_os = "xous"))]
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
                    if !rtc_set(&mut env.i2c, 0, 0, 10, 1, 6, 21) {
                        log::info!("{}|RTC|FAIL|SET|", SENTINEL);
                    }

                    self.start_elapsed = Some(env.ticktimer.elapsed_ms());
                    self.start_time = rtc_get(&mut env.i2c);

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

                    env.ticktimer.sleep_ms(6000).unwrap(); // wait so we have some realistic delta on the datetime function
                    self.end_elapsed = Some(env.ticktimer.elapsed_ms());
                    self.end_time = rtc_get(&mut env.i2c);

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

                    if ut < ut_after || ut_after > 8500 {
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
                    self.codec.abort().unwrap(); // this should stop callbacks from occurring too.
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
                    if ((env.llio.adc_vbus().unwrap() as f64) * 0.005033) > 1.5 {
                        // if power is plugged in, deny powerdown request
                        write!(ret, "Can't run OQC test while charging. Unplug charging cable and try again.").unwrap();
                        return Ok(Some(ret));
                    }
                    // start the server if it isn't started already, but only allow it to start once. Note that the CID stays the same between calls,
                    // because the SID is stable between calls and we're calling from the same process each time.
                    let oqc_cid = if let Some(oc) = self.oqc_cid {
                        oc
                    } else {
                        let oqc_cid = Arc::new(AtomicU32::new(0));
                        let kbd = self.kbd.take().expect("someone took the keyboard server before we could use it!");
                        // start the OQC thread
                        let _ = std::thread::spawn({
                            let oqc_cid = oqc_cid.clone();
                            move || {
                                crate::oqc_test::oqc_test(oqc_cid, kbd);
                            }
                        });
                        // wait until the OQC thread has connected itself
                        while oqc_cid.load(Ordering::SeqCst) == 0 {
                            env.ticktimer.sleep_ms(200).unwrap();
                        }
                        self.oqc_cid = Some(oqc_cid.load(Ordering::SeqCst));
                        oqc_cid.load(Ordering::SeqCst)
                    };

                    let susres = susres::Susres::new_without_hook(&env.xns).unwrap();
                    // turn off the connection for the duration of this test
                    env.netmgr.connection_manager_stop().unwrap();
                    env.llio.wfi_override(true).unwrap();
                    // activate SSID scanning while the test runs
                    env.com.set_ssid_scanning(true).expect("couldn't turn on SSID scanning");
                    //xous::rsyscall(xous::SysCall::IncreaseHeap(65536, xous::MemoryFlags::R | xous::MemoryFlags::W)).expect("couldn't increase our heap");
                    ret.clear();
                    #[cfg(any(target_os = "none", target_os = "xous"))]
                    if 0x362f093 != self.jtag.get_id().unwrap() {
                        write!(ret, "FAIL: JTAG self access").unwrap();
                        return Ok(Some(ret));
                    }
                    let battstats = env.com.get_more_stats().unwrap();
                    if battstats[12] < 3900 {
                        write!(ret, "FAIL: Battery voltage too low ({}mV) for shipment. Charge to >3900mV before OQC testing.", battstats[12]).unwrap();
                        return Ok(Some(ret));
                    }
                    if battstats[12] > 4200 {
                        write!(ret, "FAIL: Battery voltage too high ({}mV).\nSuspect issue with U17P or U11K.", battstats[12]).unwrap();
                        return Ok(Some(ret));
                    }
                    log::info!("initiating suspend");
                    env.ticktimer.sleep_ms(250).unwrap(); // give a moment for all the command queues to clear
                    susres.initiate_suspend().unwrap();
                    env.ticktimer.sleep_ms(1000).unwrap(); // pause for the suspend/resume cycle

                    let timeout = 60_000;
                    xous::send_message(oqc_cid,
                        xous::Message::new_blocking_scalar(OqcOp::Trigger.to_usize().unwrap(), timeout, 0, 0, 0,)
                    ).expect("couldn't trigger self test");
                    // join the LAN while the keyboard test is running
                    log::info!("starting wlan join");
                    env.com.wlan_set_ssid("precursortest").unwrap();
                    env.ticktimer.sleep_ms(500).unwrap();
                    env.com.wlan_set_pass("notasecret").unwrap();
                    env.ticktimer.sleep_ms(500).unwrap();
                    env.com.wlan_join().unwrap();

                    loop {
                        match oqc_status(oqc_cid) {
                            Some(true) => {
                                log::info!("wrapping up: fetching SSID list");
                                let ssid_list = env.netmgr.wifi_get_ssid_list().unwrap();
                                write!(ret, "RSSI reported in dBm:\n").unwrap();
                                for ssid in ssid_list {
                                    if ssid.name.len() > 0 {
                                        write!(ret, "-{} {}\n", ssid.rssi, &ssid.name.as_str().unwrap_or("UTF-8 error")).unwrap();
                                    }
                                }
                                write!(ret, "CHECK: was backlight on?\ndid keyboard vibrate?\nwas there sound?\n",).unwrap();
                                let (maj, min, rev, extra, gitrev) = env.llio.soc_gitrev().unwrap();
                                write!(ret, "Version {}.{}.{}+{}, commit {:x}\n", maj, min, rev, extra, gitrev).unwrap();
                                log::info!("finished status update");
                                break;
                            }
                            Some(false) => {
                                write!(ret, "Keyboard test failed.\n").unwrap();
                                break;
                            }
                            None => {
                                env.ticktimer.sleep_ms(500).unwrap();
                            }
                        }
                    }
                    // re-connect to the network, if things didn't work in the first place
                    let mut net_up = false;
                    let mut dhcp_ok = false;
                    let mut ssid_ok = false;
                    let mut wifi_tries = 0;
                    loop {
                        // parse and see if we connected from the first attempt (called before this loop)
                        log::info!("polling WLAN status");
                        if let Ok(status) = env.com.wlan_status() {
                            log::info!("got status: {:?}", status);
                            net_up = status.link_state == com_rs_ref::LinkState::Connected;
                            dhcp_ok = status.ipv4.dhcp == com_rs_ref::DhcpState::Bound;
                            ssid_ok = if let Some(ssid) = status.ssid {
                                log::info!("got ssid: {}", ssid.name.as_str().unwrap_or("invalid"));
                                ssid.name.as_str().unwrap_or("invalid") == "precursortest"
                            } else {
                                false
                            };
                            // if connected, break
                            if net_up && dhcp_ok && ssid_ok {
                                log::info!("WLAN is OK");
                                write!(ret, "WLAN OK\n").unwrap();
                                break;
                            } else {
                                log::info!("WLAN is TRY");
                                write!(ret, "WLAN TRY\n").unwrap();
                            }
                        } else {
                            log::info!("WLAN couldn't get status");
                            write!(ret, "WLAN TRY: Couldn't get status!\n").unwrap();
                        }
                        if wifi_tries < 3 {
                            // else retry the connection sequence -- leave, ssid, pass, join. takes some time.
                            env.com.wlan_leave().unwrap();
                            env.ticktimer.sleep_ms(2000).unwrap();
                            env.com.wlan_set_ssid("precursortest").unwrap();
                            env.ticktimer.sleep_ms(800).unwrap();
                            env.com.wlan_set_pass("notasecret").unwrap();
                            env.ticktimer.sleep_ms(800).unwrap();
                            env.com.wlan_join().unwrap();
                            env.ticktimer.sleep_ms(8000).unwrap();
                        } else {
                            if !net_up {
                                log::info!("connection failed");
                                write!(ret, "WLAN FAIL: connection failed\n").unwrap();
                            }
                            if !dhcp_ok {
                                log::info!("dhcp failed");
                                write!(ret, "WLAN FAIL: dhcp fail\n").unwrap();
                            }
                            if !ssid_ok {
                                log::info!("ssid mismatch");
                                write!(ret, "WLAN FAIL: ssid mismatch\n").unwrap();
                            }
                            return Ok(Some(ret));
                        }
                        wifi_tries += 1;
                    }

                    AUDIO_OQC.store(true, Ordering::Relaxed);
                    self.freq = 659.25;
                    self.left_play = true;
                    self.right_play = true;
                    self.speaker_play = true;
                    self.codec.setup_8k_stream().expect("couldn't set the CODEC to expected defaults");
                    env.ticktimer.sleep_ms(50).unwrap();
                    self.codec.set_speaker_volume(VolumeOps::RestoreDefault, None).unwrap();
                    self.codec.set_headphone_volume(VolumeOps::RestoreDefault, None).unwrap();
                    if self.callback_id.is_none() {
                        let cb_id = env.register_handler(String::<256>::from_str(self.verb()));
                        log::trace!("hooking frame callback with ID {}", cb_id);
                        self.codec.hook_frame_callback(cb_id, self.callback_conn).unwrap(); // any non-handled IDs get routed to our callback port
                        self.callback_id = Some(cb_id);
                    }
                    self.play_sample = 0.0;
                    self.rec_sample = 0;
                    self.oqc_start = env.ticktimer.elapsed_ms();
                    self.codec.resume().unwrap();

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
                "ship" => {
                    if ((env.llio.adc_vbus().unwrap() as f64) * 0.005033) > 1.5 {
                        // if power is plugged in, deny powerdown request
                        write!(ret, "System can't go into ship mode while charging. Unplug charging cable and try again.").unwrap();
                    } else {
                        if Ok(true) == env.gam.shipmode_blank_request() {
                            env.ticktimer.sleep_ms(500).unwrap(); // let the screen redraw

                            // allow EC to snoop, so that it can wake up the system
                            env.llio.allow_ec_snoop(true).unwrap();
                            // allow the EC to power me down
                            env.llio.allow_power_off(true).unwrap();
                            // now send the power off command
                            env.com.ship_mode().unwrap();

                            // now send the power off command
                            env.com.power_off_soc().unwrap();

                            log::info!("CMD: ship mode now!");
                            // pause execution, nothing after this should be reachable
                            env.ticktimer.sleep_ms(10000).unwrap(); // ship mode happens in 10 seconds
                            log::info!("CMD: if you can read this, ship mode failed!");
                        }
                        write!(ret, "Ship mode request denied").unwrap();
                    }
                }
                _ => {
                    () // do nothing
                }
            }

        }
        Ok(Some(ret))
    }

    fn callback(&mut self, msg: &MessageEnvelope, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
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

                if !AUDIO_OQC.load(Ordering::Relaxed) {
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
                } else {
                    let elapsed = env.ticktimer.elapsed_ms();
                    let increment = (elapsed - self.oqc_start) / 500;
                    match increment % 3 {
                        0 => self.freq = 659.25,
                        1 => self.freq = 783.99,
                        2 => self.freq = 987.77,
                        _ => self.freq = 659.25,
                    }
                    if elapsed - self.oqc_start > 6000 {
                        self.codec.abort().unwrap();

                        // put system automatically into ship mode at conclusion of test
                        env.gam.shipmode_blank_request().unwrap();
                        env.ticktimer.sleep_ms(500).unwrap(); // let the screen redraw

                        // allow EC to snoop, so that it can wake up the system
                        env.llio.allow_ec_snoop(true).unwrap();
                        // allow the EC to power me down
                        env.llio.allow_power_off(true).unwrap();
                        // now send the power off command
                        env.com.ship_mode().unwrap();

                        // now send the power off command
                        env.com.power_off_soc().unwrap();

                        log::info!("CMD: ship mode now!");
                        // pause execution, nothing after this should be reachable
                        env.ticktimer.sleep_ms(10000).unwrap(); // ship mode happens in 10 seconds
                        log::info!("CMD: if you can read this, ship mode failed!");
                    }
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
fn rtc_set(i2c: &mut llio::I2c, secs: u8, mins: u8, hours: u8, days: u8, months: u8, years: u8) -> bool {
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

    match i2c.i2c_write(ABRTCMC_I2C_ADR, ABRTCMC_CONTROL3, &txbuf) {
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
fn rtc_get(i2c: &mut llio::I2c) -> Option<DateTime> {
    let mut rxbuf = [0; 7];
    match i2c.i2c_read(ABRTCMC_I2C_ADR, ABRTCMC_SECONDS, &mut rxbuf, None) {
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

fn oqc_status(conn: xous::CID) -> Option<bool> { // None if still running or not yet run; Some(true) if pass; Some(false) if fail
    let result = xous::send_message(conn,
        xous::Message::new_blocking_scalar(OqcOp::Status.to_usize().unwrap(), 0, 0, 0, 0)
    ).expect("couldn't query test status");
    match result {
        xous::Result::Scalar1(val) => {
            match val {
                0 => return None,
                1 => return Some(true),
                2 => return Some(false),
                _ => return Some(false),
            }
        }
        _ => {
            log::error!("internal error");
            panic!("improper result code on oqc status query");
        }
    }
}
