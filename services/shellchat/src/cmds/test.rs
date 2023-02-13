use crate::oqc_test::OqcOp;
use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String;
use xous::{MessageEnvelope, Message};

use codec::*;
use base64::encode;
use core::fmt::Write;
use core::mem::size_of;
use core::sync::atomic::{AtomicBool, Ordering, AtomicU32};
use std::sync::Arc;
use num_traits::*;
#[cfg(feature="extra-tests")]
use std::time::Instant;

static AUDIO_OQC: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
pub struct Test {
    state: u32,
    // audio
    codec: codec::Codec,
    recbuf: Option<xous::MemoryRange>,
    callback_id: Option<u32>,
    callback_conn: u32,
    framecount: u32,
    play_sample: f32, // count of play samples generated. in f32 to avoid int<->f32 conversions
    rec_sample: usize, // count of record samples recorded. in usize because we're not doing f32 wave table computations on this
    left_play: bool,
    right_play: bool,
    speaker_play: bool,
    freq: f32,
    start_time: Option<u64>,
    end_time: Option<u64>,
    start_elapsed: Option<u64>,
    end_elapsed: Option<u64>,
    oqc_cid: Option<xous::CID>,
    kbd: Option<keyboard::Keyboard>,
    oqc_start: u64,
    #[cfg(any(feature="precursor", feature="renode"))]
    jtag: jtag::Jtag,
}
impl Test {
    pub fn new(xns: &xous_names::XousNames) -> Self {
        let codec = codec::Codec::new(xns).unwrap();

        let callback_conn = xns.request_connection_blocking(crate::SERVER_NAME_SHELLCHAT).unwrap();

        Test {
            codec,
            recbuf: None,
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
            #[cfg(any(feature="precursor", feature="renode"))]
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

use std::num::ParseIntError;
/// this will parse a simple decimal into an i32, multiplied by 1000
/// we do this because the full f32 parsing stuff is pretty heavy, some
/// 28kiB of code
#[inline(never)]
fn simple_kilofloat_parse(input: &str) -> core::result::Result<i32, ParseIntError> {
    if let Some((integer, fraction)) = input.split_once('.') {
        let mut result = integer.parse::<i32>()? * 1000;
        let mut significance = 100i32;
        for (place, digit) in fraction.chars().enumerate() {
            if place >= 3 {
                break;
            }
            if let Some(d) = digit.to_digit(10) {
                if result >= 0 {
                    result += (d as i32) * significance;
                } else {
                    result -= (d as i32) * significance;
                }
                significance /= 10;
            } else {
                return "z".parse::<i32>() // you can't create a ParseIntError any other way
            }
        }
        Ok(result)
    } else {
        let base = input.parse::<i32>()?;
        Ok(base * 1000)
    }
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
                #[cfg(feature="extra-tests")]
                "panic" => {
                    assert!(1 == 0, "Panic test: 1 == 0 failure!");
                }
                #[cfg(feature="extra-tests")]
                "instant" => {
                    write!(ret, "start elapsed_ms {}\n", env.ticktimer.elapsed_ms()).unwrap();
                    let now = Instant::now();
                    env.ticktimer.sleep_ms(5000).unwrap();
                    write!(ret, "Duration (ms): {}\n", now.elapsed().as_millis()).unwrap();
                    write!(ret, "end elapsed_ms {}\n", env.ticktimer.elapsed_ms()).unwrap();
                }
                "factory" => {
                    self.start_time = match env.llio.get_rtc_secs() {
                        Ok(s) => Some(s),
                        _ => {
                            log::info!("{}|RTC|FAIL|SET|", SENTINEL);
                            None
                        },
                    };
                    self.start_elapsed = Some(env.ticktimer.elapsed_ms());

                    // set uart MUX, and turn off WFI so UART reports are "clean" (no stuck characters when CPU is in WFI)
                    env.llio.set_uart_mux(llio::UartType::Log).unwrap();
                    env.llio.wfi_override(true).unwrap();

                    let vccint = env.llio.adc_vccint().unwrap() as f32 / 1365.0;
                    if vccint < 0.92 || vccint > 1.05 {
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
                    if vccbram < 0.92 || vccbram > 1.05 {
                        log::info!("{}|VCCBRAM|FAIL|{}", SENTINEL, vccbram);
                    } else {
                        log::info!("{}|VCCBRAM|PASS|{}", SENTINEL, vccbram);
                    }

                    let (x, y, z, id) = env.com.gyro_read_blocking().unwrap();
                    log::info!("{}|GYRO|{}|{}|{}|{}|", SENTINEL, x, y, z, id);
                    let wf_rev = env.com.get_wf200_fw_rev().unwrap();
                    log::info!("{}|WF200REV|{}|{}|{}|", SENTINEL, wf_rev.maj, wf_rev.min, wf_rev.rev);
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
                            (((ht.av_excursion[0].max as f32 - ht.av_excursion[0].min as f32) / 4096.0) * 1000.0) as u32,
                            (((ht.av_excursion[1].max as f32 - ht.av_excursion[1].min as f32) / 4096.0) * 1000.0) as u32,
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
                    self.end_time = env.llio.get_rtc_secs().ok();

                    let exact_time_secs = ((self.end_elapsed.unwrap() - self.start_elapsed.unwrap()) / 1000) as i32;
                    if let Some(end_secs) = self.end_time {
                        if let Some(start_secs) = self.start_time {
                            let elapsed_secs = (end_secs - start_secs) as i32;

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
                        match simple_kilofloat_parse(freq_str) {
                            Ok(f) => (f as f32) / 1000.0,
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
                    if self.recbuf.is_none() { // lazy allocate recbuf
                        self.recbuf = Some(xous::syscall::map_memory(
                            None,
                            None,
                            0x8000,
                            xous::MemoryFlags::R | xous::MemoryFlags::W,
                        ).expect("couldn't allocate record buffer"));
                    }
                    if let Some(recbuf) = self.recbuf {
                        let recslice = recbuf.as_slice::<u8>();
                        const BUFLEN: usize = 512;
                        // serialize and send audio as b64 encoded data
                        for (i, sample) in recslice[recslice.len()-4096 * size_of::<u32>()..].chunks_exact(BUFLEN).enumerate() {
                            let b64str = encode(sample);
                            log::info!("{}|ASAMP|{}|{}", SENTINEL, i, b64str);
                        }
                    } else {
                        panic!("recbuf was not allocated");
                    }
                    log::info!("{}|ASTOP|", SENTINEL);
                }
                "oqc" => {
                    if ((env.llio.adc_vbus().unwrap() as u32) * 503) > 150_000 { // 0.005033 * 100_000 against 1.5V * 100_000
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
                    #[cfg(any(feature="precursor", feature="renode"))]
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
                    log::info!("resumed");
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
                                let soc_ver = env.llio.soc_gitrev().unwrap();
                                write!(ret, "Version {}\n", soc_ver.to_string()).unwrap();
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

                    log::info!("Resetting the don't ask flag for initializing root keys");
                    let pddb = pddb::Pddb::new();
                    pddb.reset_dont_ask_init();

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
                #[cfg(feature="extra-tests")]
                "devboot" => {
                    env.gam.set_devboot(true).unwrap();
                    write!(ret, "devboot on").unwrap();
                }
                #[cfg(feature="extra-tests")]
                "devbootoff" => {
                    // this should do nothing if devboot was already set
                    env.gam.set_devboot(false).unwrap();
                    write!(ret, "devboot off").unwrap();
                }
                "ship" => {
                    if ((env.llio.adc_vbus().unwrap() as u32) * 503) > 150_000 { // 0.005033 * 100_000 against 1.5V * 100_000
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
                            let susres = susres::Susres::new_without_hook(&env.xns).unwrap();
                            susres.immediate_poweroff().unwrap();

                            log::info!("CMD: ship mode now!");
                            // pause execution, nothing after this should be reachable
                            env.ticktimer.sleep_ms(10000).unwrap(); // ship mode happens in 10 seconds
                            log::info!("CMD: if you can read this, ship mode failed!");
                        }
                        write!(ret, "Ship mode request denied").unwrap();
                    }
                }
                #[cfg(feature="extra-tests")]
                "timeblock" => {
                    let time_cid = xous::connect(xous::SID::from_bytes(b"timeserverpublic").unwrap()).unwrap();
                    let result = xous::send_message(time_cid,
                        xous::Message::new_blocking_scalar(3, 0, 0, 0, 0)
                    ).unwrap();
                    match result {
                        xous::Result::Scalar2(msb, lsb) => {
                            log::info!("GetTimeUtc: {}, {}", msb, lsb);
                        }
                        _ => {
                            log::info!("GetTimeUtc returned an unexpected result");
                        }
                    }
                }
                #[cfg(feature="ditherpunk")]
                "modals" => {
                    modals::tests::spawn_test();
                }
                #[cfg(feature="extra-tests")]
                "bip39" => {
                    let modals = modals::Modals::new(&env.xns).unwrap();
                    // 4. bip39 display test
                    let refnum = 0b00000110001101100111100111001010000110110010100010110101110011111101101010011100000110000110101100110110011111100010011100011110u128;
                    let refvec = refnum.to_be_bytes().to_vec();
                    modals.show_bip39(Some("Some bip39 words"), &refvec)
                    .expect("couldn't show bip39 words");

                    // 5. bip39 input test
                    log::info!("type these words: alert record income curve mercy tree heavy loan hen recycle mean devote");
                    match modals.input_bip39(Some("Input BIP39 words")) {
                        Ok(data) => {
                            log::info!("got bip39 input: {:x?}", data);
                            log::info!("reference: 0x063679ca1b28b5cfda9c186b367e271e");
                        }
                        Err(e) => log::error!("couldn't get input: {:?}", e),
                    }
                }
                "hpstate" => {
                    let state = self.codec.poll_headphone_state();
                    log::info!("{:?}", state);
                    write!(ret, "{:?}", state).ok();
                }
                "ecup" => {
                    let ecup_conn = env.xns.request_connection_blocking("__ECUP server__").unwrap();
                    xous::send_message(ecup_conn,
                        xous::Message::new_blocking_scalar(
                            3, // hard coded to match UpdateOp
                            0, 0, 0, 0
                        )
                    ).unwrap();
                    write!(ret, "\nDid EC auto update command").unwrap();
                }
                #[cfg(feature="benchmarks")]
                "bench" => {
                    let bench_original_sid = xous::create_server().unwrap();
                    let bench_original_cid = xous::connect(bench_original_sid).unwrap();
                    std::thread::spawn({
                        move || {
                            loop {
                                let msg = xous::receive_message(bench_original_sid).unwrap();
                                xous::msg_blocking_scalar_unpack!(msg, a1, _, _, _, {
                                    xous::return_scalar(msg.sender, a1 + 1).unwrap();
                                });
                                if msg.id() == 1 {
                                    break;
                                }
                            }
                            log::info!("Quitting old bench thread");
                        }
                    });

                    let bench_new_sid = xous::create_server().unwrap();
                    let bench_new_cid = xous::connect(bench_new_sid).unwrap();
                    std::thread::spawn({
                        move || {
                            let mut msg_opt = None;
                            let mut return_type = 0;
                            loop {
                                xous::reply_and_receive_next_legacy(bench_new_sid, &mut msg_opt, &mut return_type)
                                    .unwrap();
                                let msg = msg_opt.as_mut().unwrap();
                                if let Some(scalar) = msg.body.scalar_message_mut() {
                                    scalar.arg1 += 1;
                                    return_type = 1;
                                    if scalar.id == 1 {
                                        scalar.id = 1;
                                        xous::return_scalar(msg.sender, scalar.arg1).ok();
                                        core::mem::forget(msg_opt.take());
                                        break;
                                    } else {
                                        scalar.id = 0;
                                    }
                                }
                            }
                            log::info!("Quitting new bench thread");
                        }
                    });
                    const ITERS: usize = 10_000;
                    let tt = ticktimer_server::Ticktimer::new().unwrap();
                    let start_time = tt.elapsed_ms();
                    let mut a = 0;
                    while a < ITERS {
                        a = match xous::send_message(bench_original_cid,
                            Message::new_blocking_scalar(0, a, 0, 0, 0)
                        ) {
                            Ok(xous::Result::Scalar1(a_prime)) => a_prime,
                            _ => panic!("incorrect return type")
                        }
                    }
                    let result = format!("Original took {}ms for {} iters\n", tt.elapsed_ms() - start_time, ITERS);
                    log::info!("{}", result);
                    write!(ret, "{}\n", result).ok();
                    // this quits the thread
                    xous::send_message(bench_original_cid,
                        Message::new_blocking_scalar(1, 0, 0, 0, 0)
                    ).ok();
                    unsafe {xous::disconnect(bench_original_cid).ok()};

                    let start_time = tt.elapsed_ms();
                    let mut a = 0;
                    while a < ITERS {
                        a = match xous::send_message(bench_new_cid,
                            Message::new_blocking_scalar(0, a, 0, 0, 0)
                        ) {
                            Ok(xous::Result::Scalar1(a_prime)) => a_prime,
                            _ => panic!("incorrect return type")
                        }
                    }
                    let result = format!("New took {}ms for {} iters\n", tt.elapsed_ms() - start_time, ITERS);
                    write!(ret, "{}", result).ok();
                    log::info!("{}", result);
                    // this quits the thread
                    xous::send_message(bench_new_cid,
                        Message::new_blocking_scalar(1, 0, 0, 0, 0)
                    ).ok();
                    unsafe {xous::disconnect(bench_new_cid).ok()};
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
                        let raw_sine: i16 = (AMPLITUDE * cos_table::cos( self.play_sample * omega ) * i16::MAX as f32) as i16;
                        let left = if self.left_play { raw_sine as u16 } else { ZERO_PCM };
                        let right = if self.right_play { raw_sine as u16 } else { ZERO_PCM };
                        *sample = right as u32 | (left as u32) << 16;
                        self.play_sample += 1.0;
                    }

                    frames.nq_frame(frame).unwrap();

                }
                self.codec.swap_frames(&mut frames).unwrap();

                if !AUDIO_OQC.load(Ordering::Relaxed) {
                    if self.recbuf.is_none() { // lazy allocate recbuf
                        self.recbuf = Some(xous::syscall::map_memory(
                            None,
                            None,
                            0x8000,
                            xous::MemoryFlags::R | xous::MemoryFlags::W,
                        ).expect("couldn't allocate record buffer"));
                    }
                    if let Some(mut recbuf) = self.recbuf {
                        let rec_samples = recbuf.as_slice_mut::<u32>();
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
                        panic!("recbuf was not allocated");
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
                        let susres = susres::Susres::new_without_hook(&env.xns).unwrap();
                        susres.immediate_poweroff().unwrap();

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
