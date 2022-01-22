use crate::api::*;
use std::sync::Arc;
use core::sync::atomic::{AtomicU32, Ordering};
use com::{WlanStatus, WlanStatusIpc};
use net::MIN_EC_REV;
use xous::{msg_blocking_scalar_unpack, msg_scalar_unpack, send_message, Message};
use xous_ipc::Buffer;
use num_traits::*;
use std::io::Read;
use std::collections::HashMap;

#[allow(dead_code)]
const BOOT_POLL_INTERVAL_MS: usize = 5_758; // a slightly faster poll during boot so we acquire wifi faster once PDDB is mounted
/// this is shared externally so other functions (e.g. in status bar) that want to query the net manager know how long to back off, otherwise the status query will block
#[allow(dead_code)]
const POLL_INTERVAL_MS: usize = 20_151; // stagger slightly off of an integer-seconds interval to even out loads

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum ConnectionManagerOpcode {
    Run,
    Poll,
    Stop,
    SubscribeWifiStats,
    UnsubWifiStats,
    Quit,
}

pub(crate) fn connection_manager(sid: xous::SID, activity_interval: Arc<AtomicU32>) {
    let tt = ticktimer_server::Ticktimer::new().unwrap();
    let xns = xous_names::XousNames::new().unwrap();
    let mut com = com::Com::new(&xns).unwrap();
    let netmgr = net::NetManager::new();
    let mut pddb = pddb::Pddb::new();
    let self_cid = xous::connect(sid).unwrap();
    // give the system some time to boot before trying to run a check on the EC minimum version, as it is in reset on boot
    tt.sleep_ms(POLL_INTERVAL_MS).unwrap();

    // check that the EC rev meets the minimum version for this service to function
    // otherwise, we could crash the EC before it can update itself.
    let (maj, min, rev, commits) = com.get_ec_sw_tag().unwrap();
    let ec_rev = (maj as u32) << 24 | (min as u32) << 16 | (rev as u32) << 8 | commits as u32;
    let rev_ok = ec_rev >= MIN_EC_REV;
    if !rev_ok {
        log::warn!("EC firmware is too old to interoperate with the connection manager.");
    }

    let mut run = rev_ok;
    let mut mounted = false;
    let mut current_interval = BOOT_POLL_INTERVAL_MS;
    let mut wifi_stats_cache: WlanStatus = WlanStatus::from_ipc(WlanStatusIpc::default());
    let mut status_subscribers = HashMap::<xous::CID, WifiStateSubscription>::new();
    send_message(self_cid, Message::new_scalar(ConnectionManagerOpcode::Poll.to_usize().unwrap(), current_interval, 0, 0, 0)).expect("couldn't kick off next poll");
    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("got msg: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(ConnectionManagerOpcode::Run) => msg_scalar_unpack!(msg, _, _, _, _, {
                if !run {
                    run = true;
                    send_message(self_cid, Message::new_scalar(ConnectionManagerOpcode::Poll.to_usize().unwrap(), 0, 0, 0, 0)).expect("couldn't kick off next poll");
                }
            }),
            Some(ConnectionManagerOpcode::Poll) => msg_scalar_unpack!(msg, _, _, _, _, {
                if activity_interval.fetch_add(current_interval as u32, Ordering::SeqCst) > current_interval as u32 {
                    log::info!("wlan activity interval timeout");
                    // if the pddb isn't mounted, don't even bother checking -- we can't connect until we have a place to get keys
                    if pddb.is_mounted() && rev_ok {
                        mounted = true;
                        // the status check code is going to get refactored, so this is a "bare minimum" check
                        let new_state = com.wlan_status().unwrap();
                        if wifi_stats_cache.link_state != new_state.link_state ||
                        wifi_stats_cache.ssid.unwrap_or(com::SsidRecord::default()).name != new_state.ssid.unwrap_or(com::SsidRecord::default()).name ||
                        wifi_stats_cache.ssid.unwrap_or(com::SsidRecord::default()).rssi != new_state.ssid.unwrap_or(com::SsidRecord::default()).rssi
                        {
                            for &sub in status_subscribers.keys() {
                                let buf = Buffer::into_buf(com::WlanStatusIpc::from_status(wifi_stats_cache)).or(Err(xous::Error::InternalError)).unwrap();
                                buf.send(sub, WifiStateCallback::Update.to_u32().unwrap()).or(Err(xous::Error::InternalError)).unwrap();
                            }
                        }
                        wifi_stats_cache = new_state;
                        let config = netmgr.get_ipv4_config();
                        if wifi_stats_cache.link_state == com_rs_ref::LinkState::WFXError {
                            com.wlan_leave().expect("couldn't issue leave command"); // this may not be received by the wf200, *but* it will also re-init the DHCP stack on the EC
                            netmgr.reset(); // this will clear our internal net state
                            // the wfx chip is wedged. kick it. This call has a built-in 2 second delay.
                            com.wifi_reset().expect("couldn't reset the wf200 chip");
                        }
                        let needs_reconnect =
                            if wifi_stats_cache.link_state == com_rs_ref::LinkState::Connected {
                                if let Some(config) = config { // check that the EC's view of the world is synchronized with our view
                                    // is it enough to just check that the address is the same?
                                    if config.addr != wifi_stats_cache.ipv4.addr {
                                        true
                                    } else {
                                        false
                                    }
                                } else {
                                    true
                                }
                            } else {
                                true
                            };
                        if needs_reconnect {
                            log::info!("wlan is not connected, attempting auto-reconnect to known AP list");
                            if let Ok(ap_list) = pddb.list_keys(AP_DICT_NAME, None) {
                                com.wlan_leave().expect("couldn't issue leave command"); // leave the previous config to reset state
                                netmgr.reset();
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
                                    wifi_stats_cache = com.wlan_status().unwrap();
                                    if wifi_stats_cache.link_state == com_rs_ref::LinkState::Connected {
                                        break;
                                    }
                                }
                                for &sub in status_subscribers.keys() {
                                    let buf = Buffer::into_buf(com::WlanStatusIpc::from_status(wifi_stats_cache)).or(Err(xous::Error::InternalError)).unwrap();
                                    buf.send(sub, WifiStateCallback::Update.to_u32().unwrap()).or(Err(xous::Error::InternalError)).unwrap();
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
                    tt.sleep_ms(current_interval).unwrap();
                    if !mounted {
                        current_interval = BOOT_POLL_INTERVAL_MS;
                    } else {
                        current_interval = POLL_INTERVAL_MS;
                    }
                    send_message(self_cid, Message::new_scalar(ConnectionManagerOpcode::Poll.to_usize().unwrap(), 0, 0, 0, 0)).expect("couldn't kick off next poll");
                }
            }),
            Some(ConnectionManagerOpcode::SubscribeWifiStats) => {
                let buffer = unsafe {
                    Buffer::from_memory_message(msg.body.memory_message().unwrap())
                };
                let sub = buffer.to_original::<WifiStateSubscription, _>().unwrap();
                let sub_cid = xous::connect(xous::SID::from_array(sub.sid)).expect("couldn't connect to wifi subscriber callback");
                status_subscribers.insert(sub_cid, sub);
            },
            Some(ConnectionManagerOpcode::UnsubWifiStats) => msg_blocking_scalar_unpack!(msg, s0, s1, s2, s3, {
                // note: this routine largely untested, could have some errors around the ordering of the blocking return vs the disconnect call.
                let sid = [s0 as u32, s1 as u32, s2 as u32, s3 as u32];
                let mut valid_sid: Option<xous::CID> = None;
                for (&cid, &sub) in status_subscribers.iter() {
                    if sub.sid == sid {
                        valid_sid = Some(cid)
                    }
                }
                xous::return_scalar(msg.sender, 1).expect("couldn't ack unsub");
                if let Some(cid) = valid_sid {
                    status_subscribers.remove(&cid);
                    unsafe{xous::disconnect(cid).expect("couldn't remove wifi status subscriber from our CID list that is limited to 32 items total. Suspect issue with ordering of disconnect vs blocking return...");}
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