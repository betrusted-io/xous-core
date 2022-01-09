use crate::api::*;
use std::sync::Arc;
use core::sync::atomic::{AtomicU32, Ordering};
use xous::{msg_blocking_scalar_unpack, msg_scalar_unpack, send_message, Message};
use num_traits::*;
use std::io::Read;

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum ConnectionManagerOpcode {
    Run,
    Poll,
    Stop,
    Quit,
}
#[allow(dead_code)]
const POLL_INTERVAL_MS: usize = 10_151; // stagger slightly off of an integer-seconds interval to even out loads

pub(crate) fn connection_manager(sid: xous::SID, activity_interval: Arc<AtomicU32>) {
    let tt = ticktimer_server::Ticktimer::new().unwrap();
    let xns = xous_names::XousNames::new().unwrap();
    let mut com = com::Com::new(&xns).unwrap();
    let mut pddb = pddb::Pddb::new();
    let self_cid = xous::connect(sid).unwrap();
    // give the system some time to boot before trying to run this
    tt.sleep_ms(POLL_INTERVAL_MS).unwrap();

    let mut run = true;
    send_message(self_cid, Message::new_scalar(ConnectionManagerOpcode::Poll.to_usize().unwrap(), POLL_INTERVAL_MS, 0, 0, 0)).expect("couldn't kick off next poll");
    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("got msg: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(ConnectionManagerOpcode::Run) => msg_scalar_unpack!(msg, poll_interval, _, _, _, {
                if !run {
                    run = true;
                    send_message(self_cid, Message::new_scalar(ConnectionManagerOpcode::Poll.to_usize().unwrap(), poll_interval, 0, 0, 0)).expect("couldn't kick off next poll");
                }
            }),
            Some(ConnectionManagerOpcode::Poll) => msg_scalar_unpack!(msg, next_poll, _, _, _, {
                if activity_interval.fetch_add(POLL_INTERVAL_MS as u32, Ordering::SeqCst) > POLL_INTERVAL_MS as u32 {
                    log::info!("wlan activity interval timeout");
                    // if the pddb isn't mounted, don't even bother checking -- we can't connect until we have a place to get keys
                    if pddb.is_mounted() {
                        // the status check code is going to get refactored, so this is a "bare minimum" check
                        let status = com.wlan_status().unwrap();
                        if status.contains("down") {
                            log::info!("wlan is not connected, attempting auto-reconnect to known AP list");
                            if let Ok(ap_list) = pddb.list_keys(AP_DICT_NAME, None) {
                                com.wlan_leave().expect("couldn't issue leave command"); // leave the previous config to reset state
                                // TODO: add an SSID scan phase, so we only try to connect to SSIDs that are currently visible.
                                // for now, just try every single one as a brute force approach.
                                for ap in ap_list {
                                    let mut wpa_pw_file = pddb.get(AP_DICT_NAME, &ap, None, false, false, None, Some(||{})).expect("couldn't retrieve AP password");
                                    let mut wp_pw_raw = [0u8; com::api::WF200_PASS_MAX_LEN];
                                    if let Ok(readlen) = wpa_pw_file.read(&mut wp_pw_raw) {
                                        let pw = std::str::from_utf8(&wp_pw_raw[..readlen]).expect("password was not valid utf-8");
                                        com.wlan_set_ssid(&ap).expect("couldn't set SSID");
                                        com.wlan_set_pass(pw).expect("couldn't set password");
                                        com.wlan_join().expect("couldn't issue join command");
                                    }
                                    // this needs to not be a dead-wait loop, but for now the WLAN API doesn't support anything better
                                    tt.sleep_ms(5_000).unwrap();
                                    if !com.wlan_status().unwrap().contains("down") {
                                        break;
                                    }
                                }
                            } else {
                                log::warn!("Connection manager couldn't access {}, but continuing to poll.", AP_DICT_NAME);
                            }
                        } else {
                            // we're up, reset the interval
                            activity_interval.store(0, Ordering::SeqCst);
                        }
                    }
                }
                if run {
                    tt.sleep_ms(next_poll).unwrap();
                    send_message(self_cid, Message::new_scalar(ConnectionManagerOpcode::Poll.to_usize().unwrap(), next_poll, 0, 0, 0)).expect("couldn't kick off next poll");
                }
            }),
            // stop is blocking because we need to ensure the previous poll has finished before moving on, otherwise,
            // we could get a double-run condition
            Some(ConnectionManagerOpcode::Stop) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                run = false;
                xous::return_scalar(msg.sender, 0).expect("couldn't ack stop");
            }),
            Some(ConnectionManagerOpcode::Quit) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, 0).unwrap();
                log::warn!("exiting connection manager");
                break;
            }),
            None => {
                log::error!("couldn't convert opcode: {:?}", msg);
            }
        }
    }
    unsafe{xous::disconnect(self_cid).ok()};
    xous::destroy_server(sid).unwrap();
}