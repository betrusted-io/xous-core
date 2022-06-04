use std::thread;
use std::sync::{Arc, atomic::AtomicU32, atomic::Ordering};
use num_traits::*;
use xous::{msg_scalar_unpack, send_message, Message, msg_blocking_scalar_unpack};
use xous_ipc::Buffer;
use locales::t;
use crate::ctap::hid::{CtapHid, KeepaliveStatus};
use usbd_human_interface_device::device::fido::FidoMsg;

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub(crate) struct FidoRequest {
    pub channel_id: [u8; 4],
    pub desc: xous_ipc::String::<1024>,
    pub deferred: bool,
    pub granted: bool,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum UxOp {
    // sets the timeouts, and also resets and pending expirations or granted state
    SetTimers,
    RequestPermission,
    Pump,
    Quit
}
struct Timeouts {
    /// how long after an interaction has happened that the interaction may still be deemed valid and "consumed"
    pub presence_timeout_ms: i64,
    /// how long the prompt should stay up before it is considered to be timed out
    pub prompt_timeout_ms: i64,
}
#[derive(Eq, PartialEq, Debug)]
enum UxState {
    Idle,
    Prompt(i64),
    Present(i64),
}

static SELF_CID: AtomicU32 = AtomicU32::new(0);
/*
/// This must be proceeded by a call to request permission, to set up the scenario
pub(crate) fn poll_consume_permission() -> bool {
    if SELF_CID.load(Ordering::SeqCstg) == 0 {
        log::error!("internal error: ux thread not started");
        return false;
    }
    match send_message(SELF_CID.load(Ordering::SeqCst),
        Message::new_blocking_scalar(UxOp::ConsumePermission.to_usize.unwrap(), 0, 0, 0, 0)
    ).expect("couldn't query ux thread") {
        xous::Result::Scalar1(r) => {
            if r == 1 {
                true
            } else {
                false
            }
        }
        _ => log::error!("Internal error: wrong return type"),
    }
}*/
pub(crate) fn set_durations(prompt: i64, presence: i64) {
    if SELF_CID.load(Ordering::SeqCst) == 0 {
        log::error!("internal error: ux thread not started");
        return;
    }
    // i mean, these really should never be bigger than this but...
    assert!(presence <= usize::MAX as i64);
    assert!(prompt <= usize::MAX as i64);
    send_message(SELF_CID.load(Ordering::SeqCst),
        Message::new_scalar(UxOp::SetTimers.to_usize().unwrap(),
            presence as usize,
            prompt as usize,
            0,
            0,
        )
    ).expect("couldn't set timers");
}
pub(crate) fn request_permission_blocking(reason: String, channel_id: [u8; 4]) -> bool {
    log::info!("requesting permission (blocking");
    request_permission_inner(reason, channel_id, true)
}
pub(crate) fn request_permission_polling(reason: String) -> bool {
    log::info!("requesting permission (polling)");
    request_permission_inner(reason, [0xff, 0xff, 0xff, 0xff], false)
}
fn request_permission_inner(reason: String, channel_id: [u8; 4], blocking: bool) -> bool {
    if SELF_CID.load(Ordering::SeqCst) == 0 {
        log::error!("internal error: ux thread not started");
        return false;
    }
    let fido_req = FidoRequest {
        channel_id,
        desc: xous_ipc::String::from_str(&reason),
        deferred: blocking,
        granted: false
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
const DEFAULT_PROMPT_TIMEOUT_MS: i64 = 10_000;
const POLLED_PROMPT_TIMEOUT_MS: i64 = 1000; // how long the screen stays up after the host "gives up" polling
const KEEPALIVE_MS: usize = 100;
pub(crate) fn start_ux_thread() {
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
            loop {
                let mut msg = xous::receive_message(sid).unwrap();
                match FromPrimitive::from_usize(msg.body.id()) {
                    Some(UxOp::SetTimers) => msg_scalar_unpack!(msg, presence, prompt, _, _, {
                        timeouts = Timeouts {
                            presence_timeout_ms: presence as i64,
                            prompt_timeout_ms: prompt as i64,
                        };
                        ux_state = UxState::Idle;
                        // in case this is called while a deferred response is pending...deny the request
                        if let Some(mut msg) = deferred_req.take() {
                            let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                            let mut fido_request = buffer.to_original::<FidoRequest, _>().unwrap();
                            fido_request.granted = false;
                            buffer.replace(fido_request).unwrap();
                        }
                    }),
                    /*
                    // this is a polled request, used by legacy u2f transactions
                    Some(UxOp::ConsumePermission) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        match ux_state {
                            UxState::Idle | UxState::Prompt(_) => {
                                // 0 = not present
                                xous::return_scalar(msg.sender, 0).unwrap();
                            },
                            UxState::Present(_) => {
                                // 1 = present
                                xous::return_scalar(msg.sender, 1).unwrap();
                                ux_state = UxState::Idle;
                                // these two paths shouldn't be mixed-and-matched, but let's just enforce that.
                                assert!(deferred_req.is_none(), "polled request succeeded when a blocking request was also in progress");
                            }
                        }
                    }), */
                    // this can set up either a blocking request (deferred = true), or a polled request (deferred = false)
                    // u2f polls; fido2 seems to require blocking requests. This difference causes a lot of complications.
                    Some(UxOp::RequestPermission) => {
                        let deferred = {
                            let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                            let fido_request = buffer.to_original::<FidoRequest, _>().unwrap();
                            channel_id = fido_request.channel_id;
                            request_str_base.clear();
                            request_str_base.push_str(fido_request.desc.as_str().unwrap_or("UTF8 Error"));
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
                                    fido_request.granted = true;
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
                                    fido_request.granted = true;
                                    buffer.replace(fido_request).unwrap();
                                    ux_state = UxState::Idle; // in the case of U2F, there is no persistence to the presence, it goes away immediately
                                    continue;
                                }
                                UxState::Prompt(_) => {
                                    log::info!("-->U2F WAITING<--");
                                    let prompt_expiration_ms = (tt.elapsed_ms() as i64) + POLLED_PROMPT_TIMEOUT_MS;
                                    ux_state = UxState::Prompt(prompt_expiration_ms); // extend the prompt time out
                                    let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                                    let mut fido_request = buffer.to_original::<FidoRequest, _>().unwrap();
                                    fido_request.granted = false;
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
                        if deferred {
                            last_timer = 1 + (prompt_expiration_ms - tt.elapsed_ms() as i64) / 1000;
                            request_str.push_str(
                                &format!("{}\n{}s remaining",
                                &request_str_base,
                                last_timer)
                            );
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
                                        log::info!("kbhit got {}", c);
                                        kbhit.store(c as u32, Ordering::SeqCst)
                                    },
                                    Ok(None) => {
                                        log::info!("kbhit exited or had no characters");
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
                            fido_request.granted = false;
                            buffer.replace(fido_request).unwrap();
                        }
                    }
                    Some(UxOp::Pump) => msg_scalar_unpack!(msg, interval, _, _, _, {
                        match ux_state {
                            UxState::Idle => {
                                // don't issue another pump message, causing the loop to end
                            },
                            UxState::Present(present_expiration_ms) => {
                                if let Some(mut msg) = deferred_req.take() {
                                    let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                                    let mut fido_request = buffer.to_original::<FidoRequest, _>().unwrap();
                                    fido_request.granted = true;
                                    buffer.replace(fido_request).unwrap();
                                }
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
                                        let mut ka = FidoMsg::default();
                                        ka.packet.copy_from_slice(&pkt);
                                        let status = usb.u2f_send(ka);
                                        match status {
                                            Ok(()) => (),
                                            Err(e) => log::error!("Error sending keepalive: {:?}", e),
                                        }
                                    }
                                    let new_timer = 1 + (prompt_expiration_ms - tt.elapsed_ms() as i64) / 1000;
                                    if last_timer != new_timer {
                                        let mut request_str = String::from(&request_str_base);
                                        request_str.push_str(
                                            &format!("{}\n{}s remaining",
                                            &request_str_base,
                                            new_timer)
                                        );
                                        modals.dynamic_notification_update(
                                            None,
                                            Some(&request_str),
                                        ).unwrap();
                                        last_timer = new_timer;
                                    }
                                }
                                // check if we got a hit
                                if kbhit.load(Ordering::SeqCst) != 0 {
                                    ux_state = UxState::Present(tt.elapsed_ms() as i64 + timeouts.presence_timeout_ms);
                                    send_message(self_cid,
                                        Message::new_scalar(UxOp::Pump.to_usize().unwrap(), KEEPALIVE_MS, 0, 0, 0)
                                    ).unwrap();
                                } else {
                                    tt.sleep_ms(interval).unwrap();
                                    // check if we timed out
                                    if tt.elapsed_ms() as i64 >= prompt_expiration_ms {
                                        ux_state = UxState::Idle;
                                        modals.dynamic_notification_close().unwrap();
                                    } else { // else pump the loop again
                                        send_message(self_cid,
                                            Message::new_scalar(UxOp::Pump.to_usize().unwrap(), KEEPALIVE_MS, 0, 0, 0)
                                        ).unwrap();
                                    }
                                }
                            }
                        }
                    }),
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