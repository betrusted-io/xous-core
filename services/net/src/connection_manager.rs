use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::Arc;
use std::time::{Duration, Instant};

use com::{SsidRecord, WlanStatus, WlanStatusIpc};
use com_rs::{ConnectResult, LinkState};
use net::MIN_EC_REV;
use num_traits::*;
use xous::{Message, msg_blocking_scalar_unpack, msg_scalar_unpack, send_message, try_send_message};
use xous_ipc::Buffer;

use crate::ComIntSources;
use crate::api::*;

#[allow(dead_code)]
const BOOT_POLL_INTERVAL_MS: usize = 4_758; // a slightly faster poll during boot so we acquire wifi faster once PDDB is mounted
/// this is shared externally so other functions (e.g. in status bar) that want to query the net manager know
/// how long to back off, otherwise the status query will block
#[allow(dead_code)]
const POLL_INTERVAL_MS: usize = 7_151; // stagger slightly off of an integer-seconds interval to even out loads. impacts rssi update frequency.
const INTERVALS_BEFORE_RETRY: usize = 3; // how many poll intervals we'll wait before we give up and try a new AP
const SCAN_COUNT_MAX: usize = 5;
const SSID_SCAN_AGING_THRESHOLD: Duration = Duration::from_secs(5); // time before a scan is considered "stale" and needs to be redone
const SSID_RESULT_AGING_THRESHOLD: Duration = Duration::from_secs(60); // time before an individual scan result is retired for being "too rarely seen"

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum ConnectionManagerOpcode {
    Run,
    Poll,
    Stop,
    DisconnectAndStop,
    WifiOnAndRun,
    WifiOn,
    SubscribeWifiStats,
    UnsubWifiStats,
    FetchSsidList,
    ComInt,
    SuspendResume,
    EcReset,
    Quit,
}
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
enum PumpOp {
    Pump,
    Quit,
}

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
enum WifiState {
    Unknown,
    Connecting,
    WaitDhcp,
    Retry,
    InvalidAp,
    InvalidAuth,
    Connected,
    Disconnected,
    Off,
    Error,
}
#[derive(Eq, PartialEq)]
enum SsidScanState {
    /// Records the time when the last scan had finished, so we can judge if the scan cache is stale, and/or
    /// rate-limit scan requests.
    Idle(Instant),
    Scanning,
    Invalid,
}

#[derive(Eq, PartialEq, Clone, Debug)]
struct SsidOrdByRssi {
    pub ssid: String,
    pub rssi: u8,
    pub last_seen: Instant,
}
impl SsidOrdByRssi {
    pub fn new(ssid: String, rssi: u8) -> Self { Self { ssid, rssi, last_seen: Instant::now() } }
}
impl Ord for SsidOrdByRssi {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering { self.rssi.cmp(&other.rssi) }
}
impl PartialOrd for SsidOrdByRssi {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
}

pub(crate) fn connection_manager(sid: xous::SID, activity_interval: Arc<AtomicU32>) {
    let tt = ticktimer_server::Ticktimer::new().unwrap();
    let xns = xous_names::XousNames::new().unwrap();
    let mut com = com::Com::new(&xns).unwrap();
    let netmgr = net::NetManager::new();
    let pddb = pddb::Pddb::new();
    let self_cid = xous::connect(sid).unwrap();
    // give the system some time to boot before trying to run a check on the EC minimum version, as it is in
    // reset on boot
    tt.sleep_ms(POLL_INTERVAL_MS).unwrap();

    // check that the EC rev meets the minimum version for this service to function
    // otherwise, we could crash the EC before it can update itself.
    let ec_rev = com.get_ec_sw_tag().unwrap();
    let rev_ok = ec_rev >= MIN_EC_REV;
    if !rev_ok {
        log::warn!("EC rev {} is incompatible with connection manager", ec_rev.to_string());
    }

    let run = Arc::new(AtomicBool::new(rev_ok));
    let pumping = Arc::new(AtomicBool::new(false));
    let mut mounted = false;
    let current_interval = Arc::new(AtomicU32::new(BOOT_POLL_INTERVAL_MS as u32));
    let mut intervals_without_activity = 0;
    let mut wifi_stats_cache: WlanStatus = WlanStatus::from_ipc(WlanStatusIpc::default());
    let mut status_subscribers = HashMap::<xous::CID, WifiStateSubscription>::new();
    let mut wifi_state = WifiState::Unknown;
    let mut last_wifi_state = wifi_state;
    // keyed on String so that dups of ssid records are replaced
    let mut ssid_list = HashMap::<String, SsidOrdByRssi>::new();
    let mut ssid_attempted = HashSet::<String>::new();
    let mut wait_count = 0;
    let mut scan_count = 0;

    let run_sid = xous::create_server().unwrap();
    let run_cid = xous::connect(run_sid).unwrap();
    let _ = std::thread::spawn({
        let run = run.clone();
        let sid = run_sid.clone();
        let main_cid = self_cid.clone();
        let self_cid = run_cid.clone();
        let interval = current_interval.clone();
        let pumping = pumping.clone();
        move || {
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            let pddb = pddb::Pddb::new();
            pddb.is_mounted_blocking();
            loop {
                let msg = xous::receive_message(sid).unwrap();
                match FromPrimitive::from_usize(msg.body.id()) {
                    Some(PumpOp::Pump) => msg_scalar_unpack!(msg, _, _, _, _, {
                        if run.load(Ordering::SeqCst) {
                            pumping.store(true, Ordering::SeqCst);
                            try_send_message(
                                main_cid,
                                Message::new_scalar(
                                    ConnectionManagerOpcode::Poll.to_usize().unwrap(),
                                    0,
                                    0,
                                    0,
                                    0,
                                ),
                            )
                            .ok();
                            tt.sleep_ms(interval.load(Ordering::SeqCst) as usize).unwrap();
                            try_send_message(
                                self_cid,
                                Message::new_scalar(PumpOp::Pump.to_usize().unwrap(), 0, 0, 0, 0),
                            )
                            .ok();
                            pumping.store(false, Ordering::SeqCst);
                        }
                    }),
                    Some(PumpOp::Quit) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        xous::return_scalar(msg.sender, 1).ok();
                        break;
                    }),
                    _ => log::error!("Unrecognized message: {:?}", msg),
                }
            }
            xous::destroy_server(sid).unwrap();
        }
    });

    // this thread ensures that the connection manager does not become a blocking item for a suspend
    // sometimes the connection manager can get stuck in very long ops, which will need to be restarted
    // anyways on resume.
    let _ = std::thread::spawn({
        let main_cid: u32 = self_cid;
        move || {
            let sus_server = xous::create_server().unwrap();
            let sus_cid = xous::connect(sus_server).unwrap();
            let mut susres = susres::Susres::new(Some(susres::SuspendOrder::Late), &xns, 0, sus_cid)
                .expect("couldn't create suspend/resume object");
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            loop {
                let msg = xous::receive_message(sus_server).unwrap();
                xous::msg_scalar_unpack!(msg, token, _, _, _, {
                    // for now, nothing to do to prepare for suspend...
                    susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                    // but on resume, kick a message to the main loop to tell it to recheck its connections!
                    tt.sleep_ms(1000).unwrap(); // wait a full second before kicking out this message, so other services can normalize before attempting a re-connect
                    xous::send_message(
                        main_cid,
                        Message::new_scalar(
                            ConnectionManagerOpcode::SuspendResume.to_usize().unwrap(),
                            0,
                            0,
                            0,
                            0,
                        ),
                    )
                    .expect("couldn't send the resume message to the main thread");
                });
            }
        }
    });

    com.set_ssid_scanning(true).unwrap(); // kick off an initial SSID scan, we'll always want this info regardless
    let mut scan_state = SsidScanState::Scanning;

    send_message(run_cid, Message::new_scalar(PumpOp::Pump.to_usize().unwrap(), 0, 0, 0, 0))
        .expect("couldn't kick off next poll");
    loop {
        let mut msg = xous::receive_message(sid).unwrap();
        log::trace!("got msg: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(ConnectionManagerOpcode::SuspendResume) => xous::msg_scalar_unpack!(msg, _, _, _, _, {
                // this doesn't follow the usual "suspender" pattern. In fact, we don't do anything special on
                // suspend; however, on resume, check with the EC and see where the link state
                // ended up.
                let (res_linkstate, _res_dhcpstate) = com.wlan_sync_state().unwrap();
                wifi_stats_cache = com.wlan_status().unwrap();
                match res_linkstate {
                    LinkState::Connected => {
                        match wifi_state {
                            WifiState::Connected => {
                                // everything is A-OK
                            }
                            WifiState::Error => {
                                // let the error handler do its thing on the next pump cycle
                            }
                            _ => {
                                // somehow, we thought we were disconnected, but then we resumed and we're
                                // magically connected. it's not clear to me
                                // how we get into this state, so let's be conservative and just leave the
                                // link and restart things.
                                com.wlan_leave().expect("couldn't issue leave command"); // leave the previous config to reset state
                                netmgr.reset();
                                send_message(
                                    self_cid,
                                    Message::new_scalar(
                                        ConnectionManagerOpcode::Poll.to_usize().unwrap(),
                                        0,
                                        0,
                                        0,
                                        0,
                                    ),
                                )
                                .expect("couldn't kick off next poll");
                            }
                        }
                    }
                    LinkState::WFXError => {
                        // reset the stats cache, and update subscribers that we're disconnected
                        wifi_stats_cache = WlanStatus::from_ipc(WlanStatusIpc::default());
                        for &sub in status_subscribers.keys() {
                            let buf = Buffer::into_buf(com::WlanStatusIpc::from_status(wifi_stats_cache))
                                .or(Err(xous::Error::InternalError))
                                .unwrap();
                            match buf.send(sub, WifiStateCallback::Update.to_u32().unwrap()) {
                                Err(e) => log::warn!("Couldn't update wifi state subscriber: {:?}", e),
                                _ => (),
                            }
                        }
                        wifi_state = WifiState::Error;
                    }
                    LinkState::ResetHold => {
                        // wifi was manually forced "off", leave it off; presume connection manager is also
                        // stopped.
                        wifi_state = WifiState::Off;
                    }
                    _ => {
                        // should approximately be a "disconnected" state.
                        match wifi_state {
                            WifiState::Connected => {
                                // move the wifi into the disconnected state to re-initiate a connection
                                netmgr.reset();
                                // reset the stats cache, and update subscribers that we're disconnected
                                wifi_stats_cache = WlanStatus::from_ipc(WlanStatusIpc::default());
                                for &sub in status_subscribers.keys() {
                                    let buf =
                                        Buffer::into_buf(com::WlanStatusIpc::from_status(wifi_stats_cache))
                                            .or(Err(xous::Error::InternalError))
                                            .unwrap();
                                    match buf.send(sub, WifiStateCallback::Update.to_u32().unwrap()) {
                                        Err(e) => {
                                            log::warn!("Couldn't update wifi state subscriber: {:?}", e)
                                        }
                                        _ => (),
                                    }
                                }
                            }
                            WifiState::Error => {
                                // let the error handler do its thing on the next pump cycle
                            }
                            _ => {
                                // we were in some intermediate state, just "snap" us to disconnected and let
                                // the state machine take care of the rest
                                wifi_state = WifiState::Disconnected;
                            }
                        }
                    }
                }
            }),
            Some(ConnectionManagerOpcode::ComInt) => msg_scalar_unpack!(msg, ints, raw_arg, 0, 0, {
                log::trace!("debug: {:x}, {:x}", ints, raw_arg);
                let mut mask_bit: u16 = 1;
                for _ in 0..16 {
                    let source = ComIntSources::from(mask_bit & (ints as u16));
                    match source {
                        ComIntSources::Connect => {
                            log::info!("{:?}", source);
                            wifi_state = match ConnectResult::decode_u16(raw_arg as u16) {
                                ConnectResult::Success => {
                                    activity_interval.store(0, Ordering::SeqCst);
                                    WifiState::WaitDhcp
                                }
                                ConnectResult::NoMatchingAp => WifiState::InvalidAp,
                                ConnectResult::Timeout => WifiState::Retry,
                                ConnectResult::Reject | ConnectResult::AuthFail => WifiState::InvalidAuth,
                                ConnectResult::Aborted => WifiState::Retry,
                                ConnectResult::Error => WifiState::Error,
                                ConnectResult::Pending => WifiState::Error,
                            };
                            log::info!("comint new wifi state: {:?}", wifi_state);
                        }
                        ComIntSources::Disconnect => {
                            log::info!("{:?}", source);
                            if wifi_state != WifiState::Off {
                                ssid_list.clear(); // clear the ssid list because a likely cause of disconnect is we've moved out of range
                                com.set_ssid_scanning(true).unwrap();
                                scan_state = SsidScanState::Scanning;
                                wifi_state = WifiState::Disconnected;
                            } else {
                                log::info!("Wifi intent is off. Ignoring disconnect interrupt.");
                            }
                        }
                        ComIntSources::WlanSsidScanUpdate => {
                            log::debug!("{:?}", source);
                            // aggressively pre-fetch results so we can connect as soon as we see an SSID
                            match com.ssid_fetch_as_list() {
                                Ok(slist) => {
                                    for (rssi, ssid) in slist.iter() {
                                        // dupes removed by nature of the HashMap
                                        ssid_list.insert(
                                            ssid.to_string(),
                                            SsidOrdByRssi::new(ssid.to_string(), *rssi),
                                        );
                                    }
                                }
                                _ => continue,
                            }
                        }
                        ComIntSources::WlanSsidScanFinished => {
                            log::debug!("{:?}", source);
                            match com.ssid_fetch_as_list() {
                                Ok(slist) => {
                                    for (rssi, ssid) in slist.iter() {
                                        ssid_list.insert(
                                            ssid.to_string(),
                                            SsidOrdByRssi::new(ssid.to_string(), *rssi),
                                        );
                                    }
                                    // prune any results that haven't been seen in a while
                                    ssid_list
                                        .retain(|_k, v| v.last_seen.elapsed() <= SSID_RESULT_AGING_THRESHOLD);
                                }
                                _ => continue,
                            }
                            scan_state = SsidScanState::Idle(Instant::now());
                        }
                        ComIntSources::WlanIpConfigUpdate => {
                            log::info!("{:?}", source);
                            activity_interval.store(0, Ordering::SeqCst);
                            // this is the "first" path -- it's hit immediately on connect.
                            // relay status updates to any subscribers that want to know if a state has
                            // changed
                            if wifi_state != WifiState::Off {
                                wifi_stats_cache = com.wlan_status().unwrap();
                                log::debug!("stats update: {:?}", wifi_stats_cache);
                                for &sub in status_subscribers.keys() {
                                    let buf =
                                        Buffer::into_buf(com::WlanStatusIpc::from_status(wifi_stats_cache))
                                            .or(Err(xous::Error::InternalError))
                                            .unwrap();
                                    match buf.send(sub, WifiStateCallback::Update.to_u32().unwrap()) {
                                        Err(e) => {
                                            log::warn!("Couldn't update wifi state subscriber: {:?}", e)
                                        }
                                        _ => (),
                                    }
                                }
                                if wifi_stats_cache.ipv4.dhcp == com_rs::DhcpState::Bound {
                                    wifi_state = WifiState::Connected;
                                } else {
                                    wifi_state = WifiState::WaitDhcp;
                                }
                                log::debug!("comint new wifi state: {:?}", wifi_state);
                            } else {
                                log::info!("Wifi intent is off, skipping wlan status query update.");
                            }
                        }
                        ComIntSources::WfxErr => {
                            log::info!("{:?}", source);
                            wifi_state = WifiState::Error;
                        }
                        _ => {}
                    }
                    mask_bit <<= 1;
                }
            }),
            Some(ConnectionManagerOpcode::Poll) => msg_scalar_unpack!(msg, _, _, _, _, {
                let interval = current_interval.load(Ordering::SeqCst) as u32;
                if activity_interval.fetch_add(interval, Ordering::SeqCst) > interval {
                    log::debug!("wlan activity interval timeout");
                    intervals_without_activity += 1;
                    if rev_ok {
                        mounted = true;

                        // heuristic to catch sync problems in the state machine: the cache won't get updated
                        // if the EC reset itself otherwise
                        if intervals_without_activity > 3 {
                            // we'd expect at least an ARP or something...
                            wifi_stats_cache = com.wlan_status().unwrap();
                            if wifi_stats_cache.link_state != com_rs::LinkState::Connected {
                                if wifi_stats_cache.link_state == com_rs::LinkState::WFXError {
                                    log::info!("WFX chipset error detected, resetting WF200");
                                    com.wifi_reset().expect("couldn't reset the wf200 chip");
                                }
                                log::info!(
                                    "Link state mismatch: moving state to disconnected ({:?})",
                                    wifi_stats_cache.link_state
                                );
                                netmgr.reset();
                            } else if wifi_stats_cache.ipv4.dhcp != com_rs::DhcpState::Bound {
                                log::info!(
                                    "DHCP state mismatch: moving state to disconnected ({:?})",
                                    wifi_stats_cache.ipv4.dhcp
                                );
                                netmgr.reset();
                            }
                            intervals_without_activity = 0;
                        }

                        if last_wifi_state == WifiState::Connected && wifi_state != WifiState::Connected {
                            log::debug!("sending disconnect update to subscribers");
                            // reset the stats cache, and update subscribers that we're disconnected
                            wifi_stats_cache = WlanStatus::from_ipc(WlanStatusIpc::default());
                            for &sub in status_subscribers.keys() {
                                let buf = Buffer::into_buf(com::WlanStatusIpc::from_status(wifi_stats_cache))
                                    .or(Err(xous::Error::InternalError))
                                    .unwrap();
                                match buf.send(sub, WifiStateCallback::Update.to_u32().unwrap()) {
                                    Err(e) => log::warn!("Couldn't update wifi state subscriber: {:?}", e),
                                    _ => (),
                                }
                            }
                        }

                        if let Ok(ap_list_vec) = pddb.list_keys(AP_DICT_NAME, None) {
                            let mut ap_list = HashSet::<String>::new();
                            for ap in ap_list_vec {
                                ap_list.insert(ap);
                            }
                            match wifi_state {
                                WifiState::Unknown
                                | WifiState::Disconnected
                                | WifiState::InvalidAp
                                | WifiState::InvalidAuth => {
                                    if scan_count > SCAN_COUNT_MAX {
                                        scan_state = SsidScanState::Idle(Instant::now());
                                        log::warn!("scan timed out, forcing scan state to idle!");
                                    }
                                    match scan_state {
                                        SsidScanState::Scanning => scan_count += 1,
                                        SsidScanState::Invalid => {
                                            scan_count = 0;
                                            com.set_ssid_scanning(true).unwrap();
                                            scan_state = SsidScanState::Scanning;
                                        }
                                        SsidScanState::Idle(_last_scan_time) => {
                                            scan_count = 0;
                                            if let Some(ssid) =
                                                get_next_ssid(&mut ssid_list, &mut ssid_attempted, ap_list)
                                            {
                                                let mut wpa_pw_file = pddb
                                                    .get(
                                                        AP_DICT_NAME,
                                                        &ssid,
                                                        None,
                                                        false,
                                                        false,
                                                        None,
                                                        Some(|| {}),
                                                    )
                                                    .expect("couldn't retrieve AP password");
                                                let mut wp_pw_raw = [0u8; com::api::WF200_PASS_MAX_LEN];
                                                if let Ok(readlen) = wpa_pw_file.read(&mut wp_pw_raw) {
                                                    let pw = std::str::from_utf8(&wp_pw_raw[..readlen])
                                                        .expect("password was not valid utf-8");
                                                    log::info!("Attempting wifi connection: {}", ssid);
                                                    com.wlan_set_ssid(&ssid).expect("couldn't set SSID");
                                                    com.wlan_set_pass(pw).expect("couldn't set password");
                                                    com.wlan_join().expect("couldn't issue join command");
                                                    wifi_state = WifiState::Connecting;
                                                }
                                            } else {
                                                // no SSIDs available, scan again
                                                log::info!("No SSIDs found, restarting SSID scan...");
                                                com.set_ssid_scanning(true).unwrap();
                                                scan_state = SsidScanState::Scanning;
                                            }
                                        }
                                    }
                                }
                                WifiState::WaitDhcp | WifiState::Connecting => {
                                    log::debug!("still waiting for connection result...");
                                    wait_count += 1;
                                    if wait_count > INTERVALS_BEFORE_RETRY {
                                        wait_count = 0;
                                        wifi_state = WifiState::Retry;
                                    }
                                }
                                WifiState::Retry => {
                                    log::debug!("got Retry on connect");
                                    com.wlan_leave().expect("couldn't issue leave command"); // leave the previous config to reset state
                                    netmgr.reset();
                                    send_message(
                                        self_cid,
                                        Message::new_scalar(
                                            ConnectionManagerOpcode::Poll.to_usize().unwrap(),
                                            0,
                                            0,
                                            0,
                                            0,
                                        ),
                                    )
                                    .expect("couldn't kick off next poll");
                                }
                                WifiState::Error => {
                                    log::debug!("got error on connect, resetting wifi chip");
                                    com.wifi_reset().expect("couldn't reset the wf200 chip");
                                    netmgr.reset(); // this can result in a suspend failure, but the suspend timeout is currently set long enough to accommodate this possibility
                                    send_message(
                                        self_cid,
                                        Message::new_scalar(
                                            ConnectionManagerOpcode::Poll.to_usize().unwrap(),
                                            0,
                                            0,
                                            0,
                                            0,
                                        ),
                                    )
                                    .expect("couldn't kick off next poll");
                                }
                                WifiState::Connected => {
                                    // this is the "rare" path -- it's if we connected and not much is going
                                    // on, so we timeout and hit this ping
                                    log::debug!("connected, updating stats cache");
                                    // relay status updates to any subscribers that want to know if a state
                                    // has changed
                                    wifi_stats_cache = com.wlan_status().unwrap();
                                    log::debug!("stats update: {:?}", wifi_stats_cache);
                                    for &sub in status_subscribers.keys() {
                                        let buf = Buffer::into_buf(com::WlanStatusIpc::from_status(
                                            wifi_stats_cache,
                                        ))
                                        .or(Err(xous::Error::InternalError))
                                        .unwrap();
                                        match buf.send(sub, WifiStateCallback::Update.to_u32().unwrap()) {
                                            // just issue a warning -- this isn't a hard error because
                                            // subscribers can disappear, but good to know this is happening.
                                            Err(e) => {
                                                log::warn!("Couldn't update wifi state subscriber: {:?}", e)
                                            }
                                            _ => (),
                                        };
                                    }
                                }
                                WifiState::Off => {
                                    // this state should not be reachable
                                    log::warn!(
                                        "Wifi state was manually forced off; did you remember to turn it on before setting the connection manager to RUN?"
                                    );
                                }
                            }
                        }
                    }
                    last_wifi_state = wifi_state;
                } else {
                    intervals_without_activity = 0;
                }

                if wifi_state == WifiState::Connected {
                    if let Some(ssid_stats) = wifi_stats_cache.ssid.as_mut() {
                        let rssi_u8 = com.wlan_get_rssi().ok().unwrap_or(255);
                        // only send an update if the RSSI changed
                        if ssid_stats.rssi != rssi_u8 {
                            ssid_stats.rssi = rssi_u8;
                            log::debug!("stats update: {:?}", wifi_stats_cache);
                            for &sub in status_subscribers.keys() {
                                let buf = Buffer::into_buf(com::WlanStatusIpc::from_status(wifi_stats_cache))
                                    .or(Err(xous::Error::InternalError))
                                    .unwrap();
                                match buf.send(sub, WifiStateCallback::Update.to_u32().unwrap()) {
                                    Err(e) => log::warn!("Couldn't update wifi state subscriber: {:?}", e),
                                    _ => (),
                                }
                            }
                        }
                    }
                }

                if !mounted {
                    current_interval.store(BOOT_POLL_INTERVAL_MS as u32, Ordering::SeqCst);
                } else {
                    current_interval.store(POLL_INTERVAL_MS as u32, Ordering::SeqCst);
                }
            }),
            Some(ConnectionManagerOpcode::SubscribeWifiStats) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let sub = buffer.to_original::<WifiStateSubscription, _>().unwrap();
                let sub_cid = xous::connect(xous::SID::from_array(sub.sid))
                    .expect("couldn't connect to wifi subscriber callback");
                status_subscribers.insert(sub_cid, sub);
            }
            Some(ConnectionManagerOpcode::UnsubWifiStats) => {
                msg_blocking_scalar_unpack!(msg, s0, s1, s2, s3, {
                    // note: this routine largely untested, could have some errors around the ordering of the
                    // blocking return vs the disconnect call.
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
                        unsafe {
                            xous::disconnect(cid).expect("couldn't remove wifi status subscriber from our CID list that is limited to 32 items total. Suspect issue with ordering of disconnect vs blocking return...");
                        }
                    }
                })
            }
            Some(ConnectionManagerOpcode::FetchSsidList) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut ret_list = buffer.to_original::<SsidList, _>().unwrap();
                let mut sorted_ssids: Vec<_> = ssid_list.values().cloned().collect();
                sorted_ssids.sort_unstable();
                for (strongest_ssids, ret_list_item) in sorted_ssids.iter().zip(ret_list.list.iter_mut()) {
                    *ret_list_item = Some(SsidRecord {
                        name: xous_ipc::String::<32>::from_str(&strongest_ssids.ssid),
                        rssi: strongest_ssids.rssi,
                    });
                }
                if wifi_state == WifiState::Off
                    || wifi_state == WifiState::Error
                    || wifi_state == WifiState::Unknown
                {
                    ret_list.state = ScanState::Off;
                } else {
                    match scan_state {
                        SsidScanState::Idle(last_scan_time) => {
                            if last_scan_time.elapsed() > SSID_SCAN_AGING_THRESHOLD {
                                log::info!("scan out of date, restarting!");
                                com.set_ssid_scanning(true).unwrap();
                                scan_state = SsidScanState::Scanning;
                                scan_count = 0;
                                ret_list.state = ScanState::Updating;
                            } else {
                                log::info!("scan is {}s old", last_scan_time.elapsed().as_secs());
                                ret_list.state = ScanState::Idle;
                            }
                        }
                        SsidScanState::Invalid => {
                            log::info!("scan data is invalid, kicking off a new scan");
                            com.set_ssid_scanning(true).unwrap();
                            scan_state = SsidScanState::Scanning;
                            scan_count = 0;
                            ret_list.state = ScanState::Updating;
                        }
                        SsidScanState::Scanning => {
                            log::info!("still scanning...");
                            ret_list.state = ScanState::Updating
                        }
                    }
                }
                buffer.replace(ret_list).expect("couldn't return config");
            }
            Some(ConnectionManagerOpcode::Run) => msg_scalar_unpack!(msg, _, _, _, _, {
                if !run.swap(true, Ordering::SeqCst) {
                    if !pumping.load(Ordering::SeqCst) {
                        // avoid having multiple pump messages being sent if a user tries to rapidly toggle
                        // the run/stop switch
                        send_message(
                            run_cid,
                            Message::new_scalar(PumpOp::Pump.to_usize().unwrap(), 0, 0, 0, 0),
                        )
                        .expect("couldn't kick off next poll");
                    }
                }
            }),
            Some(ConnectionManagerOpcode::Stop) => msg_scalar_unpack!(msg, _, _, _, _, {
                run.store(false, Ordering::SeqCst);
            }),
            Some(ConnectionManagerOpcode::WifiOnAndRun) => msg_scalar_unpack!(msg, _, _, _, _, {
                com.wlan_set_on().expect("couldn't turn on wifi");
                wifi_state = WifiState::Disconnected;
                ssid_list.clear();
                com.set_ssid_scanning(true).unwrap();
                scan_state = SsidScanState::Scanning;
                intervals_without_activity = 0;
                scan_count = 0;
                // this will force the UI to transition from 'WiFi Off' -> 'Not connected'
                wifi_stats_cache = WlanStatus {
                    ssid: None,
                    link_state: com_rs::LinkState::Disconnected,
                    ipv4: com::Ipv4Conf::default(),
                };
                log::debug!("stats update: {:?}", wifi_stats_cache);
                for &sub in status_subscribers.keys() {
                    let buf = Buffer::into_buf(com::WlanStatusIpc::from_status(wifi_stats_cache))
                        .or(Err(xous::Error::InternalError))
                        .unwrap();
                    match buf.send(sub, WifiStateCallback::Update.to_u32().unwrap()) {
                        Err(e) => log::warn!("Couldn't update wifi state subscriber: {:?}", e),
                        _ => (),
                    }
                }
                if !run.swap(true, Ordering::SeqCst) {
                    if !pumping.load(Ordering::SeqCst) {
                        // avoid having multiple pump messages being sent if a user tries to rapidly toggle
                        // the run/stop switch
                        send_message(
                            run_cid,
                            Message::new_scalar(PumpOp::Pump.to_usize().unwrap(), 0, 0, 0, 0),
                        )
                        .expect("couldn't kick off next poll");
                    }
                }
            }),
            Some(ConnectionManagerOpcode::WifiOn) => msg_scalar_unpack!(msg, _, _, _, _, {
                com.wlan_set_on().expect("couldn't turn on wifi");
                wifi_state = WifiState::Disconnected;
                ssid_list.clear();
                com.set_ssid_scanning(false).unwrap();
                scan_state = SsidScanState::Invalid;
                intervals_without_activity = 0;
                scan_count = 0;
                // this will force the UI to transition from 'WiFi Off' -> 'Not connected'
                wifi_stats_cache = WlanStatus {
                    ssid: None,
                    link_state: com_rs::LinkState::Disconnected,
                    ipv4: com::Ipv4Conf::default(),
                };
                log::debug!("stats update: {:?}", wifi_stats_cache);
                for &sub in status_subscribers.keys() {
                    let buf = Buffer::into_buf(com::WlanStatusIpc::from_status(wifi_stats_cache))
                        .or(Err(xous::Error::InternalError))
                        .unwrap();
                    match buf.send(sub, WifiStateCallback::Update.to_u32().unwrap()) {
                        Err(e) => log::warn!("Couldn't update wifi state subscriber: {:?}", e),
                        _ => (),
                    }
                }
                if !run.swap(true, Ordering::SeqCst) {
                    if !pumping.load(Ordering::SeqCst) {
                        // avoid having multiple pump messages being sent if a user tries to rapidly toggle
                        // the run/stop switch
                        send_message(
                            run_cid,
                            Message::new_scalar(PumpOp::Pump.to_usize().unwrap(), 0, 0, 0, 0),
                        )
                        .expect("couldn't kick off next poll");
                    }
                }
            }),
            Some(ConnectionManagerOpcode::DisconnectAndStop) => {
                run.store(false, Ordering::SeqCst);
                wifi_state = WifiState::Off;
                com.wlan_leave().expect("couldn't issue leave command");
                ssid_list.clear();
                intervals_without_activity = 0;
                scan_count = 0;
                com.set_ssid_scanning(false).unwrap();
                scan_state = SsidScanState::Invalid;

                tt.sleep_ms(250).unwrap(); // give a moment to clean-up after leave before turning things off
                com.wlan_set_off().expect("couldn't turn off wifi");
                wifi_stats_cache = WlanStatus {
                    ssid: None,
                    link_state: com_rs::LinkState::ResetHold,
                    ipv4: com::Ipv4Conf::default(),
                };
                log::debug!("stats update: {:?}", wifi_stats_cache);
                for &sub in status_subscribers.keys() {
                    let buf = Buffer::into_buf(com::WlanStatusIpc::from_status(wifi_stats_cache))
                        .or(Err(xous::Error::InternalError))
                        .unwrap();
                    match buf.send(sub, WifiStateCallback::Update.to_u32().unwrap()) {
                        Err(e) => log::warn!("Couldn't update wifi state subscriber: {:?}", e),
                        _ => (),
                    }
                }
            }
            Some(ConnectionManagerOpcode::EcReset) => msg_scalar_unpack!(msg, _, _, _, _, {
                // this opcode is used by other processes to inform us that the net link was reset by
                // something other than us. (e.g. an update)
                wifi_state = WifiState::Disconnected;
                ssid_list.clear();
                com.set_ssid_scanning(true).unwrap();
                scan_state = SsidScanState::Scanning;
                intervals_without_activity = 0;
                scan_count = 0;
            }),
            Some(ConnectionManagerOpcode::Quit) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                send_message(
                    run_cid,
                    Message::new_blocking_scalar(PumpOp::Quit.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .expect("couldn't tell Pump to quit");
                unsafe { xous::disconnect(run_cid).ok() };
                xous::return_scalar(msg.sender, 0).unwrap();
                log::warn!("exiting connection manager");
                break;
            }),
            None => {
                log::error!("couldn't convert opcode: {:?}", msg);
            }
        }
    }
    unsafe { xous::disconnect(self_cid).ok() };
    xous::destroy_server(sid).unwrap();
}

fn get_next_ssid(
    ssid_list_map: &mut HashMap<String, SsidOrdByRssi>,
    ssid_attempted: &mut HashSet<String>,
    ap_list: HashSet<String>,
) -> Option<String> {
    log::trace!("ap_list: {:?}", ap_list);
    log::trace!("ssid_list: {:?}", ssid_list_map);
    // 0. convert the HashMap of ssid_list into a HashSet
    let mut ssid_list = HashSet::<String>::new();
    for ssid in ssid_list_map.keys() {
        ssid_list.insert(ssid.to_string());
    }
    // 1. find the intersection of ap_list and ssid_list to create a candidate_list
    let all_candidate_list_ref = ap_list.intersection(&ssid_list).collect::<HashSet<_>>();
    // this copy is required to perform the next set computation
    let mut all_candidate_list = HashSet::<String>::new();
    for c in all_candidate_list_ref {
        all_candidate_list.insert(String::from(c));
    }
    log::trace!("intersection: {:?}", all_candidate_list);

    log::trace!("ssids already attempted: {:?}", ssid_attempted);
    // 2. find the complement of ssid_attempted and candidate_list
    let untried_candidate_list_ref = all_candidate_list.difference(ssid_attempted).collect::<HashSet<_>>();
    // this copy breaks the mutability issue with changing ssid_attempted after the difference is computed
    let mut untried_candidate_list = HashSet::<String>::new();
    for c in untried_candidate_list_ref {
        untried_candidate_list.insert(String::from(c));
    }
    log::trace!("untried_candidates: {:?}", untried_candidate_list);

    if untried_candidate_list.len() > 0 {
        if let Some(candidate) = untried_candidate_list.into_iter().next() {
            ssid_attempted.insert(candidate.to_string());
            log::debug!("SSID connect attempt: {:?}", candidate);
            Some(candidate.to_string())
        } else {
            log::error!("We should have had at least one item in the candidate list, but found none.");
            None
        }
    } else {
        // clear the ssid_attempted list and start from scratch
        log::debug!("Exhausted all candidates, starting over again...");
        ssid_attempted.clear();
        if let Some(candidate) = all_candidate_list.iter().next() {
            ssid_attempted.insert(candidate.to_string());
            log::debug!("SSID connect attempt: {:?}", candidate);
            Some(candidate.to_string())
        } else {
            log::info!("No SSID candidates visible. Debug dump:");
            log::info!("ap_list: {:?}", ap_list);
            log::info!("ssid_list: {:?}", ssid_list_map);
            log::info!("candidate list: {:?}", all_candidate_list);
            log::info!("untried candidate list: {:?}", untried_candidate_list);
            None
        }
    }
}
