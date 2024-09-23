pub use self::storage::XousStorage;
use crate::api::attestation_store::AttestationStore;
use crate::api::connection::{HidConnection, SendOrRecvError, SendOrRecvResult, SendOrRecvStatus};
use crate::api::customization::{CustomizationImpl, DEFAULT_CUSTOMIZATION};
use crate::api::firmware_protection::FirmwareProtection;
use crate::api::user_presence::{UserPresence, UserPresenceError, UserPresenceResult};
use crate::api::{attestation_store, key_store};
use crate::KEEPALIVE_DELAY_MS;
use crate::env::Env;
use core::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use persistent_store::Store;
use ctap_crypto::rng256::XousRng256;
use xous::try_send_message;
use xous_names::XousNames;
use crate::env::xous::storage::XousUpgradeStorage;
use xous_usb_hid::device::fido::*;
use modals::Modals;
use locales::t;
use std::io::{Read, Write};
use num_traits::*;

use crate::ctap::hid::{CtapHid, KeepaliveStatus, ProcessedPacket, CtapHidCommand};
use crate::{basis_change, deserialize_app_info, AppInfo, serialize_app_info};

pub const U2F_APP_DICT: &'static str = "fido.u2fapps";
const KEEPALIVE_DELAY: Duration = Duration::from_millis(KEEPALIVE_DELAY_MS);
mod storage;

pub struct XousHidConnection {
    pub endpoint: usb_device_xous::UsbHid,
}
impl XousHidConnection {
    pub fn recv_with_timeout(
        &mut self,
        buf: &mut [u8; 64],
        timeout_delay: Duration,
    ) -> SendOrRecvStatus {
        match self.endpoint.u2f_wait_incoming_timeout(timeout_delay.as_millis() as u64) {
            Ok(msg) => {
                buf.copy_from_slice(&msg.packet);
                SendOrRecvStatus::Received
            }
            Err(_e) => {
                SendOrRecvStatus::Timeout
            }
        }
    }
    pub fn u2f_wait_incoming(&self) -> Result<RawFidoReport, xous::Error> {
        self.endpoint.u2f_wait_incoming()
    }
    pub fn u2f_send(&self, msg: RawFidoReport) -> Result<(), xous::Error> {
        self.endpoint.u2f_send(msg)
    }
}

impl HidConnection for XousHidConnection {
    fn send_and_maybe_recv(
        &mut self,
        buf: &mut [u8; 64],
        _timeout: Duration,
    ) -> SendOrRecvResult {
        let mut reply = RawFidoReport::default();
        reply.packet.copy_from_slice(buf);
        match self.endpoint.u2f_send(reply) {
            Ok(()) => Ok(SendOrRecvStatus::Sent),
            Err(e) => {
                log::error!("FIDO error in sending: {:?}", e);
                Err(SendOrRecvError)
            }
        }
    }
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct Ctap1Request {
    reason: String,
    app_id: [u8; 32],
    approved: bool,
}
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
enum Ctap1Op {
    PollPermission,
    UpdateAppInfo,
    ForceTimeout,
    Invalid,
}
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
enum Ctap1TimeoutOp {
    Run,
    Pump,
    Stop,
    Invalid,
}
pub struct XousEnv {
    rng: XousRng256,
    store: Store<XousStorage>,
    main_connection: XousHidConnection,
    #[cfg(feature = "vendor_hid")]
    vendor_connection: XousHidConnection,
    modals: Modals,
    last_user_presence_request: Option::<Instant>,
    ctap1_cid: xous::CID,
    lefty_mode: Arc<AtomicBool>,
}

impl XousEnv {
    /// Returns the unique instance of the Xous environment.
    /// Blocks until the PDDB is mounted
    pub fn new(conn: xous::CID, lefty_mode: Arc<AtomicBool>) -> Self {
        // We rely on `take_storage` to ensure that this function is called only once.
        let storage = XousStorage {};
        let store = Store::new(storage).ok().unwrap();
        let xns = XousNames::new().unwrap();
        let ctap1_sid = xous::create_server().unwrap();
        let ctap1_cid = xous::connect(ctap1_sid).unwrap();

        let ctap1_timeout_sid = xous::create_server().unwrap();
        let ctap1_timeout_cid = xous::connect(ctap1_timeout_sid).unwrap();
        std::thread::spawn({
            const MARGIN_MS: u128 = 2000; // auto-clears the box 2 seconds after the timeout deadline.
            // some margin is desired because this is basically a huge race condition.
            move || {
                let tt = ticktimer_server::Ticktimer::new().unwrap();
                let mut msg_opt = None;
                let mut _return_type = 0;
                let ctap1_timeout_cid = ctap1_timeout_cid.clone();
                let ctap1_cid = ctap1_cid.clone();
                let mut start_time = Instant::now();
                let mut run = false;
                loop {
                    xous::reply_and_receive_next_legacy(ctap1_timeout_sid, &mut msg_opt, &mut _return_type)
                    .unwrap();
                    let msg = msg_opt.as_mut().unwrap();
                    log::debug!("msg: {:x?}", msg);
                    match num_traits::FromPrimitive::from_usize(msg.body.id())
                        .unwrap_or(Ctap1TimeoutOp::Invalid)
                    {
                        Ctap1TimeoutOp::Run => {
                            if !run { // only kick off the loop on the first request to run
                                start_time = Instant::now();
                                try_send_message(ctap1_timeout_cid, xous::Message::new_scalar(
                                    Ctap1TimeoutOp::Pump.to_usize().unwrap(), 0, 0, 0, 0)).ok();
                            }
                            run = true;
                        },
                        Ctap1TimeoutOp::Pump => {
                            let elapsed = Instant::now().duration_since(start_time);
                            if run {
                                if elapsed.as_millis() > crate::ctap::U2F_UP_PROMPT_TIMEOUT.as_millis() + MARGIN_MS {
                                    run = false;
                                    // timed out, force the dialog box to close
                                    try_send_message(ctap1_cid, xous::Message::new_scalar(
                                        Ctap1Op::ForceTimeout.to_usize().unwrap(), 0, 0, 0, 0)).ok();
                                    // no pump message, so the thread should stop running.
                                } else {
                                    // no timeout, keep pinging
                                    tt.sleep_ms(1000).unwrap();
                                    try_send_message(ctap1_timeout_cid, xous::Message::new_scalar(
                                        Ctap1TimeoutOp::Pump.to_usize().unwrap(), 0, 0, 0, 0)).ok();
                                }
                            }
                        },
                        Ctap1TimeoutOp::Stop => {
                            // this can come asynchronously from the ctap1 thread once the box is ack'd.
                            run = false;
                            // this will stop the pump from running, and the thread will block at waiting for incoming messages
                        },
                        Ctap1TimeoutOp::Invalid => {
                            log::error!("invalid opcode received: {:?}", msg);
                        }
                    }
                }
            }
        });

        std::thread::spawn({
            let main_cid = conn.clone();
            let lefty_mode = lefty_mode.clone();
            move || {
                let xns = xous_names::XousNames::new().unwrap();
                let pddb = pddb::Pddb::new();
                let modals = modals::Modals::new(&xns).unwrap();
                let tt = ticktimer_server::Ticktimer::new().unwrap();

                let mut msg_opt = None;
                let mut _return_type = 0;
                let mut current_id: Option<[u8; 32]> = None;
                let mut denied_id: Option<[u8; 32]> = None;
                let mut request_str = String::new();
                let mut request_start = 0u64;
                let mut last_remaining = 0u64;
                let kbhit = Arc::new(AtomicU32::new(0));
                loop {
                    xous::reply_and_receive_next_legacy(ctap1_sid, &mut msg_opt, &mut _return_type)
                    .unwrap();
                    let msg = msg_opt.as_mut().unwrap();
                    log::trace!("msg: {:x?}", msg);
                    match num_traits::FromPrimitive::from_usize(msg.body.id())
                        .unwrap_or(Ctap1Op::Invalid)
                    {
                        Ctap1Op::PollPermission => {
                            let mut buf = unsafe {
                                xous_ipc::Buffer::from_memory_message_mut(
                                    msg.body.memory_message_mut().unwrap(),
                                )
                            };
                            let mut request = buf.to_original::<Ctap1Request, _>().unwrap();
                            request.approved = false;
                            if let Some(id) = &current_id {
                                log::trace!("poll of existing ID");
                                if *id != request.app_id {
                                    log::error!("Request ID changed while request is in progress. Ignoring request");
                                    buf.replace(request).ok();
                                    xous::send_message(ctap1_timeout_cid, xous::Message::new_scalar(
                                        Ctap1TimeoutOp::Stop.to_usize().unwrap(), 0, 0, 0, 0)).ok();
                                    modals.dynamic_notification_close().ok();
                                    current_id = None;
                                    continue;
                                } else {
                                    let key = kbhit.load(Ordering::SeqCst);
                                    if key != 0 &&
                                    ((!lefty_mode.load(Ordering::SeqCst) && (key != 0x11))
                                    || (lefty_mode.load(Ordering::SeqCst) && (key != 0x14))
                                    ) { // approved
                                        log::trace!("approved");
                                        request.approved = true;
                                        current_id = None;
                                        xous::send_message(ctap1_timeout_cid, xous::Message::new_scalar(
                                            Ctap1TimeoutOp::Stop.to_usize().unwrap(), 0, 0, 0, 0)).ok();
                                        modals.dynamic_notification_close().ok();
                                    } else if
                                    (!lefty_mode.load(Ordering::SeqCst) && (key == 0x11))
                                    || (lefty_mode.load(Ordering::SeqCst) && (key == 0x14)) { // denied
                                        log::trace!("denied");
                                        request.approved = false;
                                        denied_id = current_id.take();
                                        xous::send_message(ctap1_timeout_cid, xous::Message::new_scalar(
                                            Ctap1TimeoutOp::Stop.to_usize().unwrap(), 0, 0, 0, 0)).ok();
                                        modals.dynamic_notification_close().ok();
                                    } else {
                                        log::trace!("waiting");
                                        // keep waiting for an interaction, with a timeout
                                        let now = tt.elapsed_ms();
                                        if (crate::ctap::U2F_UP_PROMPT_TIMEOUT.as_millis() as u64) > (now - request_start) {
                                            let new_remaining = ((crate::ctap::U2F_UP_PROMPT_TIMEOUT.as_millis() as u64) - (now - request_start)) / 1000;
                                            if new_remaining != last_remaining {
                                                log::info!("countdown: {}", new_remaining);
                                                last_remaining = new_remaining;
                                                let mut to_request_str = request_str.to_string();
                                                to_request_str.push_str(
                                                    &format!("\n\n⚠   {}{}   ⚠\n",
                                                    last_remaining,
                                                    t!("vault.fido.countdown", locales::LANG)
                                                ));
                                                modals.dynamic_notification_update(
                                                    Some(
                                                        if lefty_mode.load(Ordering::SeqCst) {
                                                            t!("vault.u2freq_lefty", locales::LANG)
                                                        } else {
                                                            t!("vault.u2freq", locales::LANG)
                                                        }
                                                    ),
                                                    Some(&to_request_str),
                                                ).unwrap();
                                            }
                                        } else {
                                            log::trace!("timed out");
                                            // timed out
                                            current_id = None;
                                            denied_id = None;
                                            request.approved = false;
                                            xous::send_message(ctap1_timeout_cid, xous::Message::new_scalar(
                                                Ctap1TimeoutOp::Stop.to_usize().unwrap(), 0, 0, 0, 0)).ok();
                                            modals.dynamic_notification_close().ok();
                                        }
                                    }
                                    buf.replace(request).ok();
                                    continue;
                                }
                            } else {
                                log::debug!("setup new ID query: {:?}", request.app_id);
                                if let Some(denied) = denied_id.take() {
                                    if tt.elapsed_ms() - request_start < (crate::ctap::U2F_UP_PROMPT_TIMEOUT.as_millis() as u64) {
                                        if denied == request.app_id {
                                            // this is a repeat request for a denied ID
                                            request.approved = false;
                                            buf.replace(request).ok();
                                            denied_id = Some(denied);
                                            continue;
                                        } else {
                                            // it's a different request, carry on; the denied_id will be reset as we handle this new request
                                        }
                                    } else {
                                        // denial should automatically reset due to .take()
                                    }
                                }
                                // determine the application info string
                                let app_id_str = hex::encode(request.app_id);
                                if let Some(info) = {
                                    // fetch the application info, if it exists
                                    log::debug!("querying U2F record {}", app_id_str);
                                    // add code to query the PDDB here to look for the k/v mapping of this app ID
                                    match pddb.get(
                                        U2F_APP_DICT,
                                        &app_id_str,
                                        None, true, false,
                                        Some(256), Some(basis_change)
                                    ) {
                                        Ok(mut app_data) => {
                                            let app_attr = app_data.attributes().unwrap();
                                            if app_attr.len != 0 {
                                                let mut descriptor = Vec::<u8>::new();
                                                match app_data.read_to_end(&mut descriptor) {
                                                    Ok(_) => {
                                                        deserialize_app_info(descriptor)
                                                    }
                                                    Err(e) => {log::error!("Couldn't read app info: {:?}", e); None}
                                                }
                                            } else {
                                                None
                                            }
                                        }
                                        _ => {
                                            log::info!("couldn't find key {}", app_id_str);
                                            None
                                        }
                                    }
                                } {
                                    request_str.clear();
                                    // we have some prior record of the app, human-format it
                                    request_str.push_str(&format!("\n{}{}",
                                        t!("vault.u2f.appinfo.name", locales::LANG), info.name
                                    ));
                                    request_str.push_str(&format!("\n{}",
                                        crate::atime_to_str(info.atime)
                                    ));
                                    request_str.push_str(&format!("\n{}{}",
                                        t!("vault.u2f.appinfo.authcount", locales::LANG),
                                        info.count,
                                    ));
                                } else {
                                    request_str.clear();
                                    // request approval of the new app ID
                                    request_str.push_str(&format!("\n{}\nApp ID: {:x?}",
                                        t!("vault.u2f.appinfo.newapp", locales::LANG), request.app_id
                                    ));
                                }
                                let mut to_request_str = request_str.to_string();
                                request_start = tt.elapsed_ms();
                                last_remaining = ((crate::ctap::U2F_UP_PROMPT_TIMEOUT.as_millis() as u64)
                                    - (tt.elapsed_ms() - request_start)) / 1000;
                                to_request_str.push_str(
                                    &format!("\n\n⚠   {}{}   ⚠\n",
                                    last_remaining,
                                    t!("vault.fido.countdown", locales::LANG)
                                ));

                                modals.dynamic_notification(
                                    Some(
                                        if lefty_mode.load(Ordering::SeqCst) {
                                            t!("vault.u2freq_lefty", locales::LANG)
                                        } else {
                                            t!("vault.u2freq", locales::LANG)
                                        }
                                    ),
                                    Some(&to_request_str),
                                ).unwrap();
                                // start a keyboard listener
                                kbhit.store(0, Ordering::SeqCst);
                                let _ = std::thread::spawn({
                                    let token = modals.token().clone();
                                    let conn = modals.conn().clone();
                                    let kbhit = kbhit.clone();
                                    move || {
                                        // note that if no key is hit, we get None back on dialog box close automatically
                                        match modals::dynamic_notification_blocking_listener(token, conn) {
                                            Ok(Some(c)) => {
                                                log::trace!("kbhit got {}", c);
                                                kbhit.store(c as u32, Ordering::SeqCst)
                                            },
                                            Ok(None) => {
                                                log::trace!("kbhit exited or had no characters");
                                                kbhit.store(0, Ordering::SeqCst)
                                            },
                                            Err(e) => log::error!("error waiting for keyboard hit from blocking listener: {:?}", e),
                                        }
                                    }
                                });
                                current_id = Some(request.app_id);
                                // start the timeout watchdog, because if another token responds to the request,
                                // we will be left hanging.
                                xous::try_send_message(ctap1_timeout_cid, xous::Message::new_scalar(
                                    Ctap1TimeoutOp::Run.to_usize().unwrap(), 0, 0, 0, 0)).ok();
                                denied_id = None; // reset the denial timers, too
                                request.approved = false;
                                buf.replace(request).ok();
                            }
                        }
                        Ctap1Op::UpdateAppInfo => {
                            let buf = unsafe {
                                xous_ipc::Buffer::from_memory_message(
                                    msg.body.memory_message().unwrap(),
                                )
                            };
                            let update = buf.to_original::<Ctap1Request, _>().unwrap();
                            let app_id_str = hex::encode(update.app_id);
                            if update.app_id != [0u8; 32] {
                                // note the access
                                let mut info = {
                                    // fetch the application info, if it exists
                                    log::info!("Updating U2F record {}", app_id_str);
                                    // add code to query the PDDB here to look for the k/v mapping of this app ID
                                    match pddb.get(
                                        U2F_APP_DICT,
                                        &app_id_str,
                                        None, true, false,
                                        Some(256), Some(basis_change)
                                    ) {
                                        Ok(mut app_data) => {
                                            let app_attr = app_data.attributes().unwrap();
                                            if app_attr.len != 0 {
                                                let mut descriptor = Vec::<u8>::new();
                                                match app_data.read_to_end(&mut descriptor) {
                                                    Ok(_) => {
                                                        deserialize_app_info(descriptor)
                                                    }
                                                    Err(e) => {log::error!("Couldn't read app info: {:?}", e); None}
                                                }
                                            } else {
                                                None
                                            }
                                        }
                                        _ => {
                                            log::info!("couldn't find key {}", app_id_str);
                                            None
                                        }
                                    }
                                }.unwrap_or_else(
                                    || {
                                        // otherwise, create it
                                    match modals
                                        .alert_builder(t!("vault.u2f.give_app_name", locales::LANG))
                                        .field(None, None)
                                        .build()
                                        {
                                            Ok(name) => {
                                                let info = AppInfo {
                                                    name: name.content()[0].content.to_string(),
                                                    notes: t!("vault.notes", locales::LANG).to_string(),
                                                    id: update.app_id,
                                                    ctime: crate::utc_now().timestamp() as u64,
                                                    atime: 0,
                                                    count: 0,
                                                };
                                                info
                                            }
                                            _ => {
                                                log::error!("couldn't get name for app");
                                                panic!("couldn't get name for app");
                                            }
                                        }
                                    }
                                );

                                info.atime = crate::utc_now().timestamp() as u64;
                                info.count = info.count.saturating_add(1);
                                let ser = serialize_app_info(&info);

                                // update the access time, by deleting the key and writing it back into the PDDB
                                pddb.delete_key(U2F_APP_DICT, &app_id_str, None).ok();
                                match pddb.get(
                                    U2F_APP_DICT,
                                    &app_id_str,
                                    None, true, true,
                                    Some(256), Some(basis_change)
                                ) {
                                    Ok(mut app_data) => {
                                        app_data.write(&ser).expect("couldn't update atime");
                                    }
                                    _ => log::error!("Error updating app atime"),
                                }
                                pddb.sync().ok();
                            } else {
                                log::warn!("app_id is all 0's; bypassing name association");
                            }

                            log::debug!("sycing UI state...");
                            xous::send_message(
                                main_cid,
                                xous::Message::new_scalar(
                                    crate::VaultOp::ReloadDbAndFullRedraw.to_usize().unwrap(),
                                    0, 0, 0, 0)
                            ).unwrap();
                        }
                        Ctap1Op::ForceTimeout => {
                            log::info!("polling timed out");
                            current_id.take(); // clear out the ID token
                            // close the dynamic notification box
                            modals.dynamic_notification_close().ok();
                        }
                        Ctap1Op::Invalid => {
                            log::error!("got invalid opcode: {}, ignoring", msg.body.id());
                        }
                    }
                }
            }
        });

        XousEnv {
            rng: XousRng256::new(&xns),
            store,
            main_connection: XousHidConnection {
                endpoint: usb_device_xous::UsbHid::new(),
            },
            #[cfg(feature = "vendor_hid")]
            vendor_connection: XousHidConnection {
                endpoint: UsbEndpoint::VendorHid,
            },
            modals: modals::Modals::new(&xns).unwrap(),
            last_user_presence_request: None,
            ctap1_cid,
            lefty_mode,
        }
    }
    /// Checks if the SoC is compatible with USB drivers (older versions of Precursor's FPGA don't have the USB device core)
    pub fn is_soc_compatible(&self) -> bool {
        self.main_connection.endpoint.is_soc_compatible()
    }

    fn send_keepalive_up_needed(
        &mut self,
        timeout: Duration,
        cid: [u8; 4]
    ) -> Result<(), UserPresenceError> {
        let keepalive_msg = CtapHid::keepalive(cid, KeepaliveStatus::UpNeeded);
        for mut pkt in keepalive_msg {
            match self.main_connection.send_and_maybe_recv(&mut pkt, timeout) {
                Ok(SendOrRecvStatus::Timeout) => {
                    log::debug!("Sending a KEEPALIVE packet timed out");
                    // TODO: abort user presence test?
                }
                Err(_) => panic!("Error sending KEEPALIVE packet"),
                Ok(SendOrRecvStatus::Sent) => {
                    log::trace!("Sent KEEPALIVE packet");
                }
                Ok(SendOrRecvStatus::Received) => {
                    // We only parse one packet, because we only care about CANCEL.
                    let (received_cid, processed_packet) = CtapHid::process_single_packet(&pkt);
                    if received_cid != cid {
                        log::debug!(
                            "Received a packet on channel ID {:?} while sending a KEEPALIVE packet",
                            received_cid,
                        );
                        return Ok(());
                    }
                    match processed_packet {
                        ProcessedPacket::InitPacket { cmd, .. } => {
                            if cmd == CtapHidCommand::Cancel as u8 {
                                // We ignore the payload, we can't answer with an error code anyway.
                                log::debug!("User presence check cancelled");
                                return Err(UserPresenceError::Canceled);
                            } else {
                                log::debug!(
                                    "Discarded packet with command {} received while sending a KEEPALIVE packet",
                                    cmd,
                                );
                            }
                        }
                        ProcessedPacket::ContinuationPacket { .. } => {
                            log::debug!(
                                "Discarded continuation packet received while sending a KEEPALIVE packet",
                            );
                        }
                    }
                }
            }
        }
        Ok(())
    }

}

impl UserPresence for XousEnv {
    fn check_init(&mut self) {

    }
    /// Implements FIDO behavior (CTAP2 protocol)
    fn wait_with_timeout(&mut self, timeout: Duration, reason: Option::<String>, cid: [u8; 4]) -> UserPresenceResult {
        log::info!("{}VAULT.PERMISSION,{}", xous::BOOKEND_START, xous::BOOKEND_END);
        let reason = reason.unwrap_or(String::new());
        let kbhit = Arc::new(AtomicU32::new(0));
        let expiration = Instant::now().checked_add(timeout).expect("duration bug");
        self.modals.dynamic_notification(
            Some(
                if self.lefty_mode.load(Ordering::SeqCst) {
                    t!("vault.u2freq_lefty", locales::LANG)
                } else {
                    t!("vault.u2freq", locales::LANG)
                }
            ),
            None,
        ).unwrap();
        // start the keyboard hit listener thread
        let _ = std::thread::spawn({
            let token = self.modals.token().clone();
            let conn = self.modals.conn().clone();
            let kbhit = kbhit.clone();
            move || {
                // note that if no key is hit, we get None back on dialog box close automatically
                match modals::dynamic_notification_blocking_listener(token, conn) {
                    Ok(Some(c)) => {
                        log::trace!("kbhit got {}", c);
                        kbhit.store(c as u32, Ordering::SeqCst)
                    },
                    Ok(None) => {
                        log::trace!("kbhit exited or had no characters");
                        kbhit.store(0, Ordering::SeqCst)
                    },
                    Err(e) => log::error!("error waiting for keyboard hit from blocking listener: {:?}", e),
                }
            }
        });

        let mut last_remaining = u64::MAX;
        loop {
            let mut request_str = String::from(&reason);
            let remaining = expiration.duration_since(Instant::now()).as_secs();
            if last_remaining != remaining {
                log::info!("countdown: {}", remaining);
                // only update the UX once per second
                request_str.push_str(
                    &format!("\n\n⚠   {}{}   ⚠\n",
                    remaining,
                    t!("vault.fido.countdown", locales::LANG)
                ));
                self.modals.dynamic_notification_update(
                    Some(
                        if self.lefty_mode.load(Ordering::SeqCst) {
                            t!("vault.u2freq_lefty", locales::LANG)
                        } else {
                            t!("vault.u2freq", locales::LANG)
                        }
                    ),
                    Some(&request_str),
                ).unwrap();
                last_remaining = remaining;
            }

            // handle exit cases
            if remaining == 0 {
                self.modals.dynamic_notification_close().ok();
                return Err(UserPresenceError::Timeout)
            }
            let key_hit = kbhit.load(Ordering::SeqCst);
            if key_hit != 0
            && ( // approve
                (!self.lefty_mode.load(Ordering::SeqCst) && (key_hit != 0x11)) // 0x11 is the F1 key
                || (self.lefty_mode.load(Ordering::SeqCst) && (key_hit != 0x14)) // 0x14 is the F4 key
            )
            {
                self.modals.dynamic_notification_close().ok();
                return Ok(())
            } else if // deny
                (!self.lefty_mode.load(Ordering::SeqCst) && (key_hit == 0x11))
                || (self.lefty_mode.load(Ordering::SeqCst) && (key_hit == 0x14))
            {
                self.modals.dynamic_notification_close().ok();
                return Err(UserPresenceError::Declined)
            }

            // delay, and keepalive
            self.send_keepalive_up_needed(KEEPALIVE_DELAY, cid)
            .map_err(|e| e.into())?;
            std::thread::sleep(KEEPALIVE_DELAY);
        }
    }

    /// A ctap1-specific call to see if a request was recently made
    fn recently_requested(&mut self) -> bool {
        if let Some(last_req) = self.last_user_presence_request {
            if Instant::now().duration_since(last_req).as_millis() < crate::ctap::U2F_UP_PROMPT_TIMEOUT.as_millis() {
                true
            } else {
                false
            }
        } else {
            false
        }
    }
    /// Wait for user approval of a CTAP1-type credential
    /// In this flow, we try to map the `app_id` to a user-provided, memorable string
    /// We also give users the option to abort out of approving.
    fn poll_approval_ctap1(&mut self, reason: String, app_id: [u8; 32]) -> bool {
        // update the user presence request parameter
        self.last_user_presence_request = Some(Instant::now());

        let request = Ctap1Request {
            reason: String::from(&reason),
            app_id,
            approved: false
        };
        let mut buf = xous_ipc::Buffer::into_buf(request).expect("couldn't convert IPC structure");
        buf.lend_mut(self.ctap1_cid, Ctap1Op::PollPermission.to_u32().unwrap()).expect("couldn't make CTAP1 poll request");
        let response = buf.to_original::<Ctap1Request, _>().expect("couldn't convert IPC structure");
        if response.approved {
            // serialize the response straight back to the app info update path
            // we can't "block" this path because if we take too long to respond, the web client will assume we have failed
            let buf = xous_ipc::Buffer::into_buf(response).expect("couldn't convert IPC structure");
            buf.send(self.ctap1_cid, Ctap1Op::UpdateAppInfo.to_u32().unwrap()).expect("couldn't make CTAP1 info update");
            true
        } else {
            false
        }
    }

    fn check_complete(&mut self) {

    }
}

impl FirmwareProtection for XousEnv {
    fn lock(&mut self) -> bool {
        false
    }
}

impl key_store::Helper for XousEnv {}

impl AttestationStore for XousEnv {
    fn get(
        &mut self,
        id: &attestation_store::Id,
    ) -> Result<Option<attestation_store::Attestation>, attestation_store::Error> {
        if !matches!(id, attestation_store::Id::Batch) {
            return Err(attestation_store::Error::NoSupport);
        }
        attestation_store::helper_get(self)
    }

    fn set(
        &mut self,
        id: &attestation_store::Id,
        attestation: Option<&attestation_store::Attestation>,
    ) -> Result<(), attestation_store::Error> {
        if !matches!(id, attestation_store::Id::Batch) {
            return Err(attestation_store::Error::NoSupport);
        }
        attestation_store::helper_set(self, attestation)
    }
}

use core::fmt;
pub struct Console {
}
impl Console {
    pub fn new() -> Self {
        Console {  }
    }
}
impl fmt::Write for Console {
    fn write_str(&mut self, string: &str) -> Result<(), fmt::Error> {
        log::info!("{}", string);
        Ok(())
    }
}

impl Env for XousEnv {
    type Rng = XousRng256;
    type UserPresence = Self;
    type Storage = XousStorage;
    type KeyStore = Self;
    type AttestationStore = Self;
    type FirmwareProtection = Self;
    type Write = Console;
    type Customization = CustomizationImpl;
    type HidConnection = XousHidConnection;
    type UpgradeStorage = XousUpgradeStorage;

    fn rng(&mut self) -> &mut Self::Rng {
        &mut self.rng
    }

    fn user_presence(&mut self) -> &mut Self::UserPresence {
        self
    }

    fn store(&mut self) -> &mut Store<Self::Storage> {
        &mut self.store
    }

    fn key_store(&mut self) -> &mut Self {
        self
    }

    fn attestation_store(&mut self) -> &mut Self {
        self
    }

    fn upgrade_storage(&mut self) -> Option<&mut Self::UpgradeStorage> {
        None
    }

    fn firmware_protection(&mut self) -> &mut Self::FirmwareProtection {
        self
    }

    fn write(&mut self) -> Self::Write {
        Console::new()
    }

    fn customization(&self) -> &Self::Customization {
        &DEFAULT_CUSTOMIZATION
    }

    fn main_hid_connection(&mut self) -> &mut Self::HidConnection {
        &mut self.main_connection
    }

    #[cfg(feature = "vendor_hid")]
    fn vendor_hid_connection(&mut self) -> &mut Self::HidConnection {
        &mut self.vendor_connection
    }
}

pub const KEEPALIVE_DELAY_XOUS: Duration = Duration::from_millis(KEEPALIVE_DELAY_MS);

