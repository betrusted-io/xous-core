use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};

use com::Com;
use llio::Llio;
use ticktimer_server::Ticktimer;
use xous::{Message, MessageEnvelope, ScalarMessage};
use String;

use crate::cmds::pds::Rate;
use crate::cmds::pds::PDS_DATA;
#[allow(unused_imports)]
use crate::cmds::pds::PDS_STOP_DATA;
use crate::{CommonEnv, ShellCmdApi};
static CB_RUN: AtomicBool = AtomicBool::new(false);
static CB_GO: AtomicBool = AtomicBool::new(false);
pub fn callback_thread(callback_id: usize) {
    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");
    let xns = xous_names::XousNames::new().unwrap();
    let callback_conn = xns.request_connection_blocking(crate::SERVER_NAME_SHELLCHAT).unwrap();

    loop {
        if CB_RUN.load(Ordering::Relaxed) {
            CB_RUN.store(false, Ordering::Relaxed);
            ticktimer.sleep_ms(20_000).unwrap();
            // after we wait, check and see if we still need the callback...
            if CB_GO.load(Ordering::Relaxed) {
                // send a message that will get routed to our callback handler, locatable with the
                // `callback_id`
                xous::send_message(
                    callback_conn,
                    Message::Scalar(ScalarMessage { id: callback_id, arg1: 0, arg2: 0, arg3: 0, arg4: 0 }),
                )
                .unwrap();
            }
        } else {
            ticktimer.sleep_ms(250).unwrap(); // a little more polite than simply busy-waiting
        }
    }
}

#[derive(Debug)]
pub struct Fcc {
    channel: Option<u8>,
    rate: Option<Rate>,
    pds_list: [Option<String>; 8],
    go: bool,
    tx_start_time: u64,
}
impl Fcc {
    pub fn new(env: &mut CommonEnv) -> Fcc {
        let callback_id = env.register_handler(String::from("fcc"));
        xous::create_thread_1(callback_thread, callback_id as usize)
            .expect("couldn't create callback generator thread");
        Fcc {
            channel: None, //Some(2), // default to simplify testing, replace with None
            rate: None,    //Some(Rate::B1Mbps),  // default to simplify testing, replace with None
            pds_list: [None; 8],
            go: false,
            tx_start_time: 0,
        }
    }

    fn send_pds(&self, com: &Com, ticktimer: &Ticktimer) {
        for &maybe_pds in self.pds_list.iter() {
            if let Some(pds) = maybe_pds {
                if pds.len() > 0 {
                    com.send_pds_line(&pds).unwrap();
                    ticktimer.sleep_ms(50).expect("couldn't sleep during send_pds");
                }
            }
        }
    }

    fn clear_pds(&mut self) {
        for pds in self.pds_list.iter_mut() {
            *pds = None;
        }
    }

    fn stop_tx(&mut self, _com: &com::Com, ticktimer: &Ticktimer, llio: &Llio) {
        self.go = false;
        CB_GO.store(false, Ordering::Relaxed);
        CB_RUN.store(false, Ordering::Relaxed); // make sure the callback function is disabled
        self.clear_pds();
        //self.pds_list[0] = Some(String::from(PDS_STOP_DATA));
        //self.send_pds(&com, &ticktimer);
        //self.clear_pds();
        llio.ec_reset().unwrap();
        ticktimer.sleep_ms(2000).unwrap();
    }
}
impl<'a> ShellCmdApi<'a> for Fcc {
    cmd_api!(fcc);

    // inserts boilerplate for command API

    fn process(&mut self, args: String, env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        let mut ret = String::new();
        let helpstring = "fcc [ch 1-11] [euch 1-13] [rate <code>] [go] [stop] [rev] [res]\nrate code: b[1,2,5.5,11], g[6,9,12,18,24,36,48,54], mcs[0-7]";

        // no matter what, we want SSID scanning to be off
        env.com.set_ssid_scanning(false).expect("couldn't turn off SSID scanning");

        let mut tokens = args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "res" => {
                    env.llio.ec_reset().unwrap();
                    //env.com.wifi_reset().unwrap(); // doesn't work right now :-/
                    write!(ret, "EC has been reset").unwrap();
                }
                "rev" => {
                    let (maj, min, rev) = env.com.get_wf200_fw_rev().unwrap();
                    write!(ret, "Wf200 fw rev {}.{}.{}", maj, min, rev).unwrap();
                }
                "stop" => {
                    self.stop_tx(&env.com, &env.ticktimer, &env.llio);
                    write!(ret, "{}", "Transmission stopped").unwrap();
                }
                "go" => {
                    if let Some(channel) = self.channel {
                        if channel < 1 || channel > 14 {
                            write!(ret, "Channel {} out of bounds", channel).unwrap();
                        } else {
                            if let Some(rate) = self.rate {
                                let mut found = false;
                                self.clear_pds();
                                for record in PDS_DATA.iter() {
                                    if record.rate == rate {
                                        let mut index: usize = 0;
                                        for &line in record.pds_data[(channel - 1) as usize].iter() {
                                            self.pds_list[index] = Some(String::from(line));
                                            index += 1;
                                        }
                                        found = true;
                                        break;
                                    }
                                }
                                if found {
                                    self.send_pds(&env.com, &env.ticktimer);
                                    write!(ret, "TX live: ch {} rate {:?}", channel, rate).unwrap();
                                    self.go = true;
                                    CB_GO.store(true, Ordering::Relaxed);
                                    self.tx_start_time = env.ticktimer.elapsed_ms();
                                    CB_RUN.store(true, Ordering::Relaxed); // initiate the callback for the next wakeup
                                } else {
                                    write!(
                                        ret,
                                        "Rate/channel combo not found: ch {} rate {:?}",
                                        channel, rate
                                    )
                                    .unwrap();
                                }
                            } else {
                                write!(ret, "{}", "No rate selected").unwrap();
                            }
                        }
                    } else {
                        write!(ret, "{}", "No channel selected").unwrap();
                    }
                }
                "ch" => {
                    if let Some(ch_str) = tokens.next() {
                        if let Ok(ch) = ch_str.parse::<u8>() {
                            if ch >= 1 && ch <= 11 {
                                self.channel = Some(ch);
                                write!(ret, "Channel set to {}", ch).unwrap();
                            } else {
                                write!(ret, "Channel {} is out of range (1-11)", ch).unwrap();
                            }
                        } else {
                            write!(ret, "Couldn't parse channel: {}", ch_str).unwrap();
                        }
                    } else {
                        write!(ret, "Specify channel number of 1-11").unwrap();
                    }
                }
                "euch" => {
                    if let Some(ch_str) = tokens.next() {
                        if let Ok(ch) = ch_str.parse::<u8>() {
                            if ch >= 1 && ch <= 13 {
                                self.channel = Some(ch);
                                write!(ret, "Channel set to {}", ch).unwrap();
                            } else {
                                write!(ret, "Channel {} is out of range (1-13)", ch).unwrap();
                            }
                        } else {
                            write!(ret, "Couldn't parse channel: {}", ch_str).unwrap();
                        }
                    } else {
                        write!(ret, "Specify channel number of 1-13").unwrap();
                    }
                }
                "rate" => {
                    if let Some(rate_str) = tokens.next() {
                        match rate_str {
                            "b1" => self.rate = Some(Rate::B1Mbps),
                            "b2" => self.rate = Some(Rate::B2Mbps),
                            "b5.5" => self.rate = Some(Rate::B5_5Mbps),
                            "b11" => self.rate = Some(Rate::B11Mbps),
                            "g6" => self.rate = Some(Rate::G6Mbps),
                            "g9" => self.rate = Some(Rate::G9Mbps),
                            "g12" => self.rate = Some(Rate::G12Mbps),
                            "g18" => self.rate = Some(Rate::G18Mbps),
                            "g24" => self.rate = Some(Rate::G24Mbps),
                            "g36" => self.rate = Some(Rate::G36Mbps),
                            "g48" => self.rate = Some(Rate::G48Mbps),
                            "g54" => self.rate = Some(Rate::G54Mbps),
                            "mcs0" => self.rate = Some(Rate::NMCS0),
                            "mcs1" => self.rate = Some(Rate::NMCS1),
                            "mcs2" => self.rate = Some(Rate::NMCS2),
                            "mcs3" => self.rate = Some(Rate::NMCS3),
                            "mcs4" => self.rate = Some(Rate::NMCS4),
                            "mcs5" => self.rate = Some(Rate::NMCS5),
                            "mcs6" => self.rate = Some(Rate::NMCS6),
                            "mcs7" => self.rate = Some(Rate::NMCS7),
                            _ => write!(ret, "Rate code {} invalid, pick one of b[1,2,5.5,11], g[6,9,12,18,24,36,48,54], mcs[0-7]", rate_str).unwrap(),
                        }
                        if let Some(rate) = self.rate {
                            write!(ret, "Rate set to {:?}", rate).unwrap();
                        }
                    } else {
                        write!(ret, "Specify rate code of b[1,2,5.5,11], g[6,9,12,18,24,36,48,54], mcs[0-7]")
                            .unwrap();
                    }
                }
                _ => {
                    write!(ret, "{}", helpstring).unwrap();
                }
            }
        } else {
            write!(ret, "{}", helpstring).unwrap();
        }
        Ok(Some(ret))
    }

    fn callback(
        &mut self,
        _msg: &MessageEnvelope,
        env: &mut CommonEnv,
    ) -> Result<Option<String>, xous::Error> {
        let mut ret = String::new();
        if self.go {
            if (env.ticktimer.elapsed_ms() - self.tx_start_time) > (1000 * 60 * 5) {
                self.stop_tx(&env.com, &env.ticktimer, &env.llio);
                write!(ret, "5 minute timeout on TX, stopping for TX overheat safety!").unwrap();
            } else {
                self.send_pds(&env.com, &env.ticktimer);
                write!(ret, "TX renewed: ch {} rate {:?}", self.channel.unwrap(), self.rate.unwrap())
                    .unwrap();
                CB_RUN.store(true, Ordering::Relaxed); // re-initiate the callback
            }
        } else {
            write!(ret, "Info: passing on TX renewal").unwrap();
        }
        Ok(Some(ret))
    }
}
