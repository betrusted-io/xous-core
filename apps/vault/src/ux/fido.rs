use std::thread;
use std::sync::{Arc, atomic::AtomicU32, atomic::Ordering};
use num_traits::*;
use xous::{msg_scalar_unpack, send_message, Message};
use xous_ipc::Buffer;
use locales::t;
use crate::ctap::hid::{CtapHid, KeepaliveStatus};
use usbd_human_interface_device::device::fido::RawFidoMsg;
use std::io::{Write, Read};
use crate::ux::utc_now;

// conceptual note: this UX conflates both the U2F and the FIDO2 paths.
// - U2F is a polled implementation, where the state goes from Idle->Prompt->Present
// and then immediately back to Idle. It is non-blocking and always returns a value.
// - FIDO is a deferred responder, meaning it is blocking the caller. It goes from
// Idle->Prompt->Idle again. The caller is blocked until the Prompt is answered.
// You *can* take the state machine to Present after Prompt, in which case, it will
// "remember" that a user was present for another 30 seconds. This is not actually
// the mandated behavior, it was only implemented because I got confused on part
// of the spec, but the code stubs are still around.

pub(crate) const U2F_APP_DICT: &'static str = "fido.u2fapps";

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub(crate) struct FidoRequest {
    pub channel_id: [u8; 4],
    pub desc: xous_ipc::String::<1024>,
    pub app_id: Option<[u8; 32]>,
    pub deferred: bool,
    pub granted: Option<char>,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum UxOp {
    /// sets the timeouts, and also resets and pending expirations or granted state
    SetTimers,
    /// the "generic" user interaction flow
    RequestPermission,
    /// self-referential pumping opcode to monitor timeouts, etc.
    Pump,
    /// Update or register app for U2F flow,
    U2fAppUx,
    Quit
}
#[allow(dead_code)] // presence_timeout is not used right now, but might be needed in the future if I am not understanding the spec correctly...
struct Timeouts {
    /// how long after an interaction has happened that the interaction may still be deemed valid and "consumed"
    pub presence_timeout_ms: i64,
    /// how long the prompt should stay up before it is considered to be timed out
    pub prompt_timeout_ms: i64,
}
#[derive(Eq, PartialEq, Debug)]
#[allow(dead_code)]
enum UxState {
    Idle,
    Prompt(i64),
    Present(i64),
}

static SELF_CID: AtomicU32 = AtomicU32::new(0);

pub(crate) fn set_durations(prompt: i64, presence: i64) {
    if SELF_CID.load(Ordering::SeqCst) == 0 {
        log::error!("internal error: ux thread not started");
        return;
    }
    // i mean, these really should never be bigger than this but...
    assert!(presence <= isize::MAX as i64);
    assert!(prompt <= isize::MAX as i64);
    send_message(SELF_CID.load(Ordering::SeqCst),
        Message::new_scalar(UxOp::SetTimers.to_usize().unwrap(),
            presence as usize,
            prompt as usize,
            0,
            0,
        )
    ).expect("couldn't set timers");
}
/// Some(char) => user is present, and they hit the key indicated
/// None => user was not present
pub(crate) fn request_permission_blocking(reason: String, channel_id: [u8; 4]) -> Option<char> {
    log::trace!("requesting permission (blocking)");
    log::info!("{}VAULT.PERMISSION,{}", xous::BOOKEND_START, xous::BOOKEND_END);
    request_permission_inner(reason, channel_id, true, None)
}
pub(crate) fn request_permission_polling(reason: String, application: [u8; 32]) -> bool {
    log::trace!("requesting permission (polling)");
    // reason.push_str(&format!("\nApp ID: {:x?}", application));
    request_permission_inner(reason, [0xff, 0xff, 0xff, 0xff], false, Some(application)).is_some()
}
fn request_permission_inner(reason: String, channel_id: [u8; 4], blocking: bool, app_id: Option<[u8; 32]>) -> Option<char> {
    if SELF_CID.load(Ordering::SeqCst) == 0 {
        log::error!("internal error: ux thread not started");
        return None;
    }
    let fido_req = FidoRequest {
        channel_id,
        desc: xous_ipc::String::from_str(&reason),
        deferred: blocking,
        app_id,
        granted: None
    };
    let mut buf = Buffer::into_buf(fido_req).expect("couldn't do IPC transformation");
    buf.lend_mut(
        SELF_CID.load(Ordering::SeqCst),
        UxOp::RequestPermission.to_u32().unwrap()
    ).expect("couldn't request permission");
    let ret = buf.to_original::<FidoRequest, _>().expect("couldn't do IPC transformation");
    ret.granted
}

const DEFAULT_PRESENCE_TIMEOUT_MS: i64 = 30_000;
const DEFAULT_PROMPT_TIMEOUT_MS: i64 = 30_000;
const POLLED_PROMPT_TIMEOUT_MS: i64 = 1000; // how long the screen stays up after the host "gives up" polling
const KEEPALIVE_MS: usize = 100;
pub(crate) fn start_fido_ux_thread(main_conn: xous::CID) {
    let sid = xous::create_server().unwrap();
    let cid = xous::connect(sid).expect("couldn't connect to UX thread server");
    SELF_CID.store(cid, Ordering::SeqCst);
    log::trace!("sid: {:?}, self CID: {}/{}", sid, cid, SELF_CID.load(Ordering::SeqCst));
    let _ = thread::spawn({
        let sid = sid.clone();
        let self_cid = cid.clone();
        move || {
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            let mut timeouts = Timeouts {
                presence_timeout_ms: DEFAULT_PRESENCE_TIMEOUT_MS,
                prompt_timeout_ms: DEFAULT_PROMPT_TIMEOUT_MS,
            };
            let mut ux_state = UxState::Idle;
            let mut request_str_base = String::new();
            let xns = xous_names::XousNames::new().unwrap();
            let modals = modals::Modals::new(&xns).unwrap();
            let kbhit = Arc::new(AtomicU32::new(0));
            let mut channel_id: [u8; 4] = [0xff, 0xff, 0xff, 0xff]; // this is the broadcast channel
            let usb = usb_device_xous::UsbHid::new();
            let mut deferred_req: Option::<xous::MessageEnvelope> = None;
            let mut last_timer = 0;
            let mut last_app_id: Option<[u8; 32]> = None;
            let mut app_info: Option<AppInfo> = None;
            let pddb = pddb::Pddb::new();
            pddb.is_mounted_blocking();
            #[cfg(feature="autotest")]
            modals.show_notification("WARNING: FIDO configured for autotest. Do not use for production!", None).unwrap();
            let mut num_fido_auths = 0; // only counts FIDO2 flow auths, not U2F auths
            loop {
                let mut msg = xous::receive_message(sid).unwrap();
                match FromPrimitive::from_usize(msg.body.id()) {
                    Some(UxOp::SetTimers) => msg_scalar_unpack!(msg, presence, prompt, _, _, {
                        timeouts = Timeouts {
                            presence_timeout_ms: presence as i64,
                            prompt_timeout_ms: prompt as i64,
                        };
                        ux_state = UxState::Idle;
                        kbhit.store(0, Ordering::SeqCst);
                        // in case this is called while a deferred response is pending...deny the request
                        if let Some(mut msg) = deferred_req.take() {
                            let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                            let mut fido_request = buffer.to_original::<FidoRequest, _>().unwrap();
                            fido_request.granted = None;
                            buffer.replace(fido_request).unwrap();
                        }
                    }),
                    // this can set up either a blocking request (deferred = true), or a polled request (deferred = false)
                    // u2f polls; fido2 seems to require blocking requests. This difference causes a lot of complications.
                    Some(UxOp::RequestPermission) => {
                        let deferred = {
                            let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                            let fido_request = buffer.to_original::<FidoRequest, _>().unwrap();
                            channel_id = fido_request.channel_id;
                            request_str_base.clear();
                            request_str_base.push_str(fido_request.desc.as_str().unwrap_or("UTF8 Error"));
                            // fill in the app info record if an app_id is provided, and it's different
                            // (we don't do the PDDB access every cycle as it's expensive computationally)
                            if last_app_id != fido_request.app_id {
                                app_info = if let Some(id) = fido_request.app_id {
                                    let app_id_str = hex::encode(id);
                                    log::info!("querying U2F record {}", app_id_str);
                                    // add code to query the PDDB here to look for the k/v mapping of this app ID
                                    match pddb.get(
                                        U2F_APP_DICT,
                                        &app_id_str,
                                        None, true, false,
                                        Some(256), Some(crate::basis_change)
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
                                } else {
                                    None
                                };
                            }
                            last_app_id = fido_request.app_id;
                            fido_request.deferred
                        };
                        if deferred {
                            match ux_state {
                                UxState::Idle => {}, // do the rest of the code
                                // I'm interpreting the OpenSK implementation to mean that once you have indicate your presence,
                                // you don't have to indicate it *again* for some duration interval, to avoid annoying the user.
                                // If it's the case that this is not true, then what should happen is when the original deferred request
                                // returns, the UxState immediately goes to Idle. This will trigger another pop-up the next time
                                // the call is made.
                                UxState::Present(_) => {
                                    // short circuit all the logic, and grant the presence
                                    let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                                    let mut fido_request = buffer.to_original::<FidoRequest, _>().unwrap();
                                    fido_request.granted = if kbhit.load(Ordering::SeqCst) == 0 {
                                        None
                                    } else {
                                        char::from_u32(kbhit.load(Ordering::SeqCst))
                                    };
                                    buffer.replace(fido_request).unwrap();
                                    continue;
                                }
                                UxState::Prompt(_) => {
                                    panic!("Illegal state -- the caller should have blocked on the Prompt state, making it impossible to double-initiate");
                                }
                            }
                        } else {
                            match ux_state {
                                UxState::Idle => log::info!("-->U2F SETUP<--"),
                                UxState::Present(_) => {
                                    log::info!("-->U2F USER PRESENT<--");
                                    let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                                    let mut fido_request = buffer.to_original::<FidoRequest, _>().unwrap();
                                    fido_request.granted = if kbhit.load(Ordering::SeqCst) == 0 {
                                        None
                                    } else {
                                        char::from_u32(kbhit.load(Ordering::SeqCst))
                                    };
                                    buffer.replace(fido_request).unwrap();
                                    ux_state = UxState::Idle; // in the case of U2F, there is no persistence to the presence, it goes away immediately
                                    send_message(self_cid,
                                        Message::new_scalar(UxOp::U2fAppUx.to_usize().unwrap(), 0, 0, 0, 0)
                                    ).unwrap();
                                    continue;
                                }
                                UxState::Prompt(_) => {
                                    log::trace!("-->U2F WAITING<--");
                                    let prompt_expiration_ms = (tt.elapsed_ms() as i64) + POLLED_PROMPT_TIMEOUT_MS;
                                    ux_state = UxState::Prompt(prompt_expiration_ms); // extend the prompt time out
                                    let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                                    let mut fido_request = buffer.to_original::<FidoRequest, _>().unwrap();
                                    fido_request.granted = None;
                                    buffer.replace(fido_request).unwrap();
                                    // in case of U2F, just inform that no permission is granted yet and abort
                                    continue;
                                }
                            }
                        }
                        let prompt_expiration_ms = (tt.elapsed_ms() as i64)
                            + if deferred {
                                timeouts.prompt_timeout_ms
                            } else {
                                POLLED_PROMPT_TIMEOUT_MS
                            };
                        ux_state = UxState::Prompt(prompt_expiration_ms);
                        let mut request_str = String::from(&request_str_base);
                        if let Some(info) = &app_info {
                            // we have some prior record of the app, human-format it
                            request_str.push_str(&format!("\n{}{}",
                                t!("vault.u2f.appinfo.name", xous::LANG), info.name
                            ));
                            request_str.push_str(&format!("\n{}",
                                crate::ux::atime_to_str(info.atime)
                            ));
                            request_str.push_str(&format!("\n{}{}",
                                t!("vault.u2f.appinfo.authcount", xous::LANG),
                                info.count,
                            ));
                        } else if !deferred {
                            // no record of the app. Just give the hash.
                            request_str.push_str(&format!("\n{}{}",
                                t!("vault.u2f.newapphash", xous::LANG),
                                hex::encode(last_app_id.unwrap_or([0; 32]))
                            ))
                        }
                        if deferred {
                            last_timer = 1 + (prompt_expiration_ms - tt.elapsed_ms() as i64 - 1) / 1000;
                            request_str.push_str(
                                &format!("\n\n⚠   {}{}   ⚠\n",
                                last_timer,
                                t!("vault.fido.countdown", xous::LANG)
                            ));
                        }
                        modals.dynamic_notification(
                            Some(t!("vault.u2freq", xous::LANG)),
                            Some(&request_str),
                        ).unwrap();
                        kbhit.store(0, Ordering::SeqCst);
                        let _ = thread::spawn({
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
                        send_message(self_cid,
                            Message::new_scalar(UxOp::Pump.to_usize().unwrap(), KEEPALIVE_MS, 0, 0, 0)
                        ).unwrap();
                        if deferred {
                            deferred_req = Some(msg);
                        } else { // in the case of a polled implementation, just return false for granted and move on
                            let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                            let mut fido_request = buffer.to_original::<FidoRequest, _>().unwrap();
                            fido_request.granted = None;
                            buffer.replace(fido_request).unwrap();
                        }
                    }
                    Some(UxOp::Pump) => msg_scalar_unpack!(msg, interval, _, _, _, {
                        match ux_state {
                            UxState::Idle => {
                                // don't issue another pump message, causing the loop to end
                            },
                            UxState::Present(present_expiration_ms) => {
                                // check if we timed out
                                if tt.elapsed_ms() as i64 >= present_expiration_ms {
                                    ux_state = UxState::Idle;
                                } else { // else pump the loop again
                                    send_message(self_cid,
                                        Message::new_scalar(UxOp::Pump.to_usize().unwrap(), KEEPALIVE_MS, 0, 0, 0)
                                    ).unwrap();
                                }
                            },
                            UxState::Prompt(prompt_expiration_ms) => {
                                if deferred_req.is_some() { // keepalives are only needed for deferred requests
                                    let keepalive_msg = CtapHid::keepalive(channel_id, KeepaliveStatus::UpNeeded);
                                    for pkt in keepalive_msg {
                                        let mut ka = RawFidoMsg::default();
                                        ka.packet.copy_from_slice(&pkt);
                                        let status = usb.u2f_send(ka);
                                        match status {
                                            Ok(()) => (),
                                            Err(e) => log::error!("Error sending keepalive: {:?}", e),
                                        }
                                    }
                                    let new_timer = 1 + (prompt_expiration_ms - tt.elapsed_ms() as i64) / 1000;
                                    if last_timer != new_timer {
                                        log::info!("new_timer: {}", new_timer);
                                        let mut request_str = String::from(&request_str_base);
                                        request_str.push_str(
                                            &format!("\n\n⚠   {}{}   ⚠\n",
                                            new_timer,
                                            t!("vault.fido.countdown", xous::LANG)
                                        ));
                                        modals.dynamic_notification_update(
                                            Some(t!("vault.u2freq", xous::LANG)),
                                            Some(&request_str),
                                        ).unwrap();
                                        last_timer = new_timer;
                                    }
                                }
                                let key_hit = kbhit.load(Ordering::SeqCst);
                                if key_hit != 0 && key_hit != 0x11 { // 0x11 is the F1 character
                                    num_fido_auths += 1;
                                    log::info!("got user presence! count: {}", num_fido_auths);
                                    if let Some(mut msg) = deferred_req.take() {
                                        let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                                        let mut fido_request = buffer.to_original::<FidoRequest, _>().unwrap();
                                        fido_request.granted = char::from_u32(key_hit);
                                        log::info!("returning {:?}", fido_request.granted);
                                        buffer.replace(fido_request).unwrap();
                                        ux_state = UxState::Idle; // comment out this line if you want presence to persist after touch for FIDO2
                                    } else {
                                        // this is the U2F flow
                                        ux_state = UxState::Present(tt.elapsed_ms() as i64 + timeouts.presence_timeout_ms);
                                    }

                                    modals.dynamic_notification_close().unwrap();
                                    send_message(self_cid,
                                        Message::new_scalar(UxOp::Pump.to_usize().unwrap(), KEEPALIVE_MS, 0, 0, 0)
                                    ).unwrap();
                                } else {
                                    tt.sleep_ms(interval).unwrap();
                                    if key_hit != 0x11 {
                                        // check if we timed out
                                        if tt.elapsed_ms() as i64 >= prompt_expiration_ms {
                                            num_fido_auths += 1; // bump it so we can pass the "don't touch it" test
                                            log::info!("timed out");
                                            ux_state = UxState::Idle;
                                            modals.dynamic_notification_close().unwrap();
                                            // unblock the listener with a `None` response
                                            if let Some(mut msg) = deferred_req.take() {
                                                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                                                let mut fido_request = buffer.to_original::<FidoRequest, _>().unwrap();
                                                fido_request.granted = None;
                                                log::trace!("timeout returning {:?}", fido_request.granted);
                                                buffer.replace(fido_request).unwrap();
                                            }
                                        } else { // else pump the loop again
                                            send_message(self_cid,
                                                Message::new_scalar(UxOp::Pump.to_usize().unwrap(), KEEPALIVE_MS, 0, 0, 0)
                                            ).unwrap();
                                        }
                                    } else {
                                        log::info!("aborted");
                                        modals.dynamic_notification_close().unwrap();
                                        tt.sleep_ms(interval).unwrap();
                                        modals.dynamic_notification(
                                            Some(t!("vault.u2f.wait_abort", xous::LANG)),
                                            None,
                                        ).unwrap();
                                        kbhit.store(0, Ordering::SeqCst); // reset the waiting key...
                                        send_message(self_cid,
                                            Message::new_scalar(UxOp::Pump.to_usize().unwrap(), KEEPALIVE_MS, 0, 0, 0)
                                        ).unwrap();
                                    }
                                }
                            }
                        }
                    }),
                    Some(UxOp::U2fAppUx) => {
                        // this flow only triggers on U2F queries, which present an app id hash
                        if let Some(id) = last_app_id {
                            let app_id_str = hex::encode(id);
                            let ser = if let Some(info) = &mut app_info {
                                // if an appinfo was found, just update it
                                info.atime = utc_now().timestamp() as u64;
                                info.count = info.count.saturating_add(1);
                                serialize_app_info(info)
                            } else {
                                // otherwise, create it
                                match modals
                                    .alert_builder(t!("vault.u2f.give_app_name", xous::LANG))
                                    .field(None, None)
                                    .build()
                                {
                                    Ok(name) => {
                                        let info = AppInfo {
                                            name: name.content()[0].content.to_string(),
                                            notes: t!("vault.notes", xous::LANG).to_string(),
                                            id,
                                            ctime: utc_now().timestamp() as u64,
                                            atime: 0,
                                            count: 0,
                                        };
                                        serialize_app_info(&info)
                                    }
                                        _ => {
                                            log::error!("couldn't get name for app");
                                            continue;
                                        }
                                }
                            };
                            // this get determines which basis the key is in
                            let basis = match pddb.get(
                                U2F_APP_DICT,
                                &app_id_str,
                                None, true, true,
                                Some(256), Some(crate::basis_change)
                            ) {
                                Ok(app_data) => {
                                    let attr = app_data.attributes().expect("couldn't get attributes");
                                    attr.basis
                                }
                                Err(e) => {
                                    log::error!("error updating app atime: {:?}", e);
                                    continue;
                                }
                            };
                            pddb.delete_key(U2F_APP_DICT, &app_id_str, Some(&basis)).ok();
                            match pddb.get(
                                U2F_APP_DICT,
                                &app_id_str,
                                Some(&basis), true, true,
                                Some(256), Some(crate::basis_change)
                            ) {
                                Ok(mut app_data) => {
                                    app_data.write(&ser).expect("couldn't update atime");
                                }
                                _ => log::error!("Error updating app atime"),
                            }
                            pddb.sync().ok();

                            // force a redraw of the screen so the new record is rendered,
                            // but for some reason this isn't working...
                            log::info!("sycing UI state...");
                            send_message(
                                main_conn,
                                Message::new_scalar(
                                    crate::VaultOp::ReloadDbAndFullRedraw.to_usize().unwrap(),
                                    0, 0, 0, 0)
                            ).unwrap();
                        } else {
                            log::error!("Illegal state for U2F registration!");
                        }
                        last_app_id = None;
                        app_info = None;
                    }
                    Some(UxOp::Quit) => {
                        break;
                    }
                    None => {
                        log::error!("got unknown message: {:?}", msg);
                    }
                }
            }
        }
    });
}

/// app info format:
///
/// name: free form text string until newline
/// hash: app hash in hex string, lowercase
/// created: decimal number representing epoch of the creation date
/// last auth: decimal number representing epoch of the last auth time
pub(crate) struct AppInfo {
    pub name: String,
    pub id: [u8; 32],
    pub notes: String,
    pub ctime: u64,
    pub atime: u64,
    pub count: u64,
}

pub(crate) fn deserialize_app_info(descriptor: Vec::<u8>) -> Option::<AppInfo> {
    if let Ok(desc_str) = String::from_utf8(descriptor) {
        let mut appinfo = AppInfo {
            name: String::new(),
            notes: String::new(),
            id: [0u8; 32],
            ctime: 0,
            atime: 0,
            count: 0,
        };
        let lines = desc_str.split('\n');
        for line in lines {
            if let Some((tag, data)) = line.split_once(':') {
                match tag {
                    "name" => {
                        appinfo.name.push_str(data);
                    }
                    "notes" => appinfo.notes.push_str(data),
                    "id" => {
                        if let Ok(id) = hex::decode(data) {
                            appinfo.id.copy_from_slice(&id);
                        } else {
                            return None;
                        }
                    }
                    "ctime" => {
                        if let Ok(ctime) = u64::from_str_radix(data, 10) {
                            appinfo.ctime = ctime;
                        } else {
                            return None;
                        }
                    }
                    "atime" => {
                        if let Ok(atime) = u64::from_str_radix(data, 10) {
                            appinfo.atime = atime;
                        } else {
                            return None;
                        }
                    }
                    "count" => {
                        if let Ok(count) = u64::from_str_radix(data, 10) {
                            appinfo.count = count;
                        }
                        // count was added later, so, we don't fail if we don't see the record.
                    }
                    _ => {
                        log::warn!("unexpected tag {} encountered parsing app info, aborting", tag);
                        return None;
                    }
                }
            } else {
                log::trace!("invalid line skipped: {:?}", line);
            }
        }
        #[cfg(any(feature="precursor", feature="renode"))]
        if appinfo.name.len() > 0
        && appinfo.id != [0u8; 32]
        && appinfo.ctime != 0 { // atime can be 0 - indicates never used
            Some(appinfo)
        } else {
            None
        }
        #[cfg(not(target_os = "xous"))]
        if appinfo.name.len() > 0
        && appinfo.id != [0u8; 32] { // atime can be 0 - indicates never used. In hosted mode, ctime is 0.
            Some(appinfo)
        } else {
            None
        }
    } else {
        None
    }
}

pub(crate) fn serialize_app_info<'a>(appinfo: &AppInfo) -> Vec::<u8> {
    format!("{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n",
        "name", appinfo.name,
        "id", hex::encode(appinfo.id),
        "ctime", appinfo.ctime,
        "atime", appinfo.atime,
        "count", appinfo.count,
    ).into_bytes()
}
