use core::sync::atomic::{AtomicU32, Ordering};

use num_traits::*;
use utralib::*;
use xous::{CID, Message, msg_blocking_scalar_unpack, msg_scalar_unpack, send_message, sender::Sender};
use xous_api_susres::*;
use xous_ipc::Buffer;

static TIMEOUT_TIME: AtomicU32 = AtomicU32::new(5000);

#[derive(Copy, Clone, Debug)]
struct ScalarCallback {
    server_to_cb_cid: CID,
    cb_to_client_cid: CID,
    cb_to_client_id: u32,
    ready_to_suspend: bool,
    token: u32,
    failed_to_suspend: bool,
    order: xous_api_susres::SuspendOrder,
}

pub fn start_susres_service() {
    std::thread::spawn(move || {
        susres_service();
    });
}

fn susres_service() {
    let xns = xous_names::XousNames::new().unwrap();
    // unlimited connections allowed
    let susres_sid = xns.register_name(api::SERVER_NAME_SUSRES, None).expect("can't register server");
    let susres_conn = xous::connect(susres_sid).unwrap();

    let mut clk_mgr = bao1x_hal::clocks::ClockManager::new().unwrap();
    let measured = clk_mgr.measured_freqs();
    log::info!("computed frequencies:");
    log::info!("  vco: {}", clk_mgr.vco_freq);
    log::info!("  fclk: {}", clk_mgr.fclk);
    log::info!("  aclk: {}", clk_mgr.aclk);
    log::info!("  hclk: {}", clk_mgr.hclk);
    log::info!("  iclk: {}", clk_mgr.iclk);
    log::info!("  pclk: {}", clk_mgr.pclk);
    log::info!("  per: {}", clk_mgr.perclk);
    log::info!("measured frequencies:");
    for (name, freq) in measured {
        log::info!("  {}: {} MHz", name, freq);
    }
    let hal = bao1x_hal_service::Hal::new();

    let timeout_sid = xous::create_server().unwrap();
    let timeout_outgoing_conn = xous::connect(timeout_sid).unwrap();
    std::thread::spawn({
        let timeout_sid = timeout_sid.clone();
        // safety: this thread will read-only from the susres timer fields, and thus its operations
        // are thread-safe
        let susres_base = unsafe { clk_mgr.susres_base() };
        move || {
            let hw = CSR::new(susres_base as *mut u32);
            loop {
                // *any* message triggers the timer
                let _msg = xous::receive_message(timeout_sid).unwrap();

                // we have to re-implement the ticktimer time reading here because as we wait for
                // the timeout, the ticktimer goes away! so we use the susres local copy with direct hardware
                // ops to keep track of time in this phase
                fn get_hw_time(hw: &CSR<u32>) -> u64 {
                    hw.r(utra::susres::TIME0) as u64 | ((hw.r(utra::susres::TIME1) as u64) << 32)
                }
                let start = get_hw_time(&hw);
                let timeout = TIMEOUT_TIME.load(Ordering::Relaxed); // ignore updates to timeout once we're waiting
                while ((get_hw_time(&hw) - start) as u32) < timeout {
                    // log::info!("delta t: {}", (get_hw_time(hw) - start) as u32);
                    xous::yield_slice();
                }
                log::trace!("HW timeout reached");
                send_message(
                    susres_conn,
                    Message::new_scalar(Opcode::SuspendTimeout.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .ok();
            }
        }
    });
    let mut suspend_requested: Option<Sender> = None;
    let mut timeout_pending = false;
    let mut reboot_requested: bool = false;
    let mut allow_suspend = true;

    let mut suspend_subscribers = Vec::<ScalarCallback>::new();
    let mut current_op_order = xous_api_susres::SuspendOrder::Early;

    let mut gated_pids = Vec::<xous::MessageSender>::new();
    loop {
        let msg = xous::receive_message(susres_sid).unwrap();
        if reboot_requested {
            match FromPrimitive::from_usize(msg.body.id()) {
                Some(Opcode::RebootCpuConfirm) => {
                    clk_mgr.reboot(false);
                }
                Some(Opcode::RebootSocConfirm) => {
                    clk_mgr.reboot(true);
                }
                _ => reboot_requested = false,
            }
        } else {
            let opcode = FromPrimitive::from_usize(msg.body.id());
            log::debug!("{:?}", opcode);
            match opcode {
                Some(Opcode::RebootRequest) => {
                    reboot_requested = true;
                }
                Some(Opcode::RebootCpuConfirm) => {
                    log::error!("RebootCpuConfirm, but no prior Request. Ignoring.");
                }
                Some(Opcode::RebootSocConfirm) => {
                    log::error!("RebootSocConfirm, but no prior Request. Ignoring.");
                }
                Some(Opcode::RebootVector) => unimplemented!(),
                Some(Opcode::SuspendEventSubscribe) => {
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let hookdata = buffer.to_original::<ScalarHook, _>().unwrap();
                    do_hook(hookdata, &mut suspend_subscribers);
                }
                Some(Opcode::SuspendingNow) => {
                    if suspend_requested.is_none() {
                        // this is harmless, it means a process' execution gate came a bit later than
                        // expected, so just ignore and tell it to resume
                        // the execution gate is only requested until *after* a process has checked in and
                        // said it is ready to suspend, anyways.
                        log::warn!(
                            "exec gate message received late from pid {:?}, ignoring",
                            msg.sender.pid()
                        );
                        xous::return_scalar(msg.sender, 0)
                            .expect("couldn't return dummy message to unblock execution");
                    } else {
                        gated_pids.push(msg.sender);
                    }
                }
                Some(Opcode::SuspendReady) => msg_scalar_unpack!(msg, token, _, _, _, {
                    log::debug!("SuspendReady with token {}", token);
                    if suspend_requested.is_none() {
                        log::error!(
                            "received a SuspendReady message when a suspend wasn't pending from token {}",
                            token
                        );
                        continue;
                    }
                    if token >= suspend_subscribers.len() {
                        panic!("received a SuspendReady token that's out of range");
                    }
                    let scb = &mut suspend_subscribers[token];
                    if scb.ready_to_suspend {
                        log::error!("received a duplicate SuspendReady token: {} from {:?}", token, scb);
                        continue;
                    }
                    scb.ready_to_suspend = true;

                    // DEBUG NOTES:
                    // "<-- use this to debug s/r" in the lib.rs file and switch that to an "info" level
                    // in order to get the best-quality debug info (lib-side hook can give us caller PID)
                    let mut all_ready = true;
                    for sub in suspend_subscribers.iter() {
                        if sub.order == current_op_order {
                            if !sub.ready_to_suspend {
                                log::info!("  -> NOT READY token: {}", sub.token);
                                all_ready = false;
                                break;
                            }
                        }
                    }
                    // note: we must have at least one `Last` subscriber for this logic to work!
                    if all_ready && current_op_order == xous_api_susres::SuspendOrder::Last {
                        // turn off preemption
                        hal.set_preemption(false);
                        log::info!("all callbacks reporting in, doing suspend");
                        timeout_pending = false;

                        clk_mgr.wfi();

                        // ~~~ time passes, but we're on carbonite so we don't notice ~~~

                        clk_mgr.restore_wfi();

                        // when wfi() returns, it means we've resumed
                        let sender = suspend_requested
                            .take()
                            .expect("suspend was requested, but no requestor is on record!");

                        log_server::resume(); // log server is a special case, in order to avoid circular dependencies

                        // this now allows all other threads to commence
                        log::info!("low-level resume done, restoring execution");
                        for pid in gated_pids.drain(..) {
                            xous::return_scalar(pid, 0)
                                .expect("couldn't return dummy message to unblock execution");
                        }
                        // restore preemption
                        hal.set_preemption(true);
                        // this unblocks the requestor of the suspend
                        xous::return_scalar(sender, 1).ok();
                    } else if all_ready {
                        log::info!("finished with {:?} going to next round", current_op_order);
                        // the current order is finished, send the next tranche
                        current_op_order = current_op_order.next();
                        let mut at_least_one_event_sent = false;
                        while !at_least_one_event_sent {
                            let (send_success, next_op_order) =
                                send_event(&suspend_subscribers, current_op_order);
                            if !send_success {
                                current_op_order = next_op_order;
                            }
                            at_least_one_event_sent = send_success;
                        }
                        log::info!("Now waiting on {:?} stage", current_op_order);
                        // let the events fire
                        xous::yield_slice();
                    } else {
                        log::trace!("still waiting on callbacks, returning to main loop");
                    }
                }),
                Some(Opcode::SuspendRequest) => {
                    log::info!("registered suspend listeners:");
                    for sub in suspend_subscribers.iter() {
                        log::info!("{:?}", sub);
                    }
                    // if the 2-second timeout is still pending from a previous suspend, deny the suspend
                    // request. ...just don't suspend that quickly after resuming???
                    if allow_suspend && !timeout_pending {
                        suspend_requested = Some(msg.sender);

                        // clear the ready to suspend flag and failed to suspend flag
                        for sub in suspend_subscribers.iter_mut() {
                            sub.ready_to_suspend = false;
                            sub.failed_to_suspend = false;
                        }
                        // do we want to start the timeout before or after sending the notifications? hmm. 🤔
                        timeout_pending = true;
                        // any message to this server will trigger it - it only has one function
                        send_message(timeout_outgoing_conn, Message::new_scalar(0, 0, 0, 0, 0))
                            .expect("couldn't initiate timeout before suspend!");

                        current_op_order = xous_api_susres::SuspendOrder::Early;
                        let mut at_least_one_event_sent = false;
                        while !at_least_one_event_sent {
                            let (send_success, next_op_order) =
                                send_event(&suspend_subscribers, current_op_order);
                            if !send_success {
                                current_op_order = next_op_order;
                            }
                            at_least_one_event_sent = send_success;
                        }
                        // let the events fire
                        xous::yield_slice();
                    } else {
                        log::warn!(
                            "suspend requested, but the system was not allowed to suspend. Ignoring request."
                        );
                        xous::return_scalar(msg.sender, 0).ok();
                    }
                }
                Some(Opcode::SuspendTimeout) => {
                    if timeout_pending {
                        // record which tokens had not reported in
                        for sub in suspend_subscribers.iter_mut() {
                            sub.failed_to_suspend = !sub.ready_to_suspend;
                        }
                        timeout_pending = false;
                        log::warn!(
                            "Suspend timed out, forcing an unclean suspend at stage {:?}",
                            current_op_order
                        );
                        for sub in suspend_subscribers.iter() {
                            if sub.order == current_op_order {
                                if !sub.ready_to_suspend {
                                    // note to debugger: you will get a token number, which is in itself not
                                    // useful. There should be, at least
                                    // once in the debug log, printed on the very first suspend cycle,
                                    // a list of PID->tokens. Tokens are assigned in the order that the
                                    // registration happens to the susres
                                    // server. Empirically, this list is generally stable for every build,
                                    // and is guaranteed to be stable across a single cold boot.
                                    log::warn!("  -> NOT READY TOKEN: {}", sub.token);
                                }
                            }
                        }

                        let sender = suspend_requested
                            .take()
                            .expect("suspend was requested, but no requestor is on record!");
                        for pid in gated_pids.drain(..) {
                            xous::return_scalar(pid, 0)
                                .expect("couldn't return dummy message to unblock execution");
                        }

                        // this unblocks the requestor of the suspend
                        xous::return_scalar(sender, 0).ok();
                    } else {
                        log::info!("clean suspend timeout received, ignoring");
                        // this means we did a clean suspend, we've resumed, and the timeout came back after
                        // the resume just ignore the message.
                    }
                }
                Some(Opcode::WasSuspendClean) => msg_blocking_scalar_unpack!(msg, token, _, _, _, {
                    let mut clean = true;
                    for sub in suspend_subscribers.iter() {
                        if sub.token == token as u32 && sub.failed_to_suspend {
                            clean = false;
                        }
                    }
                    if clean {
                        xous::return_scalar(msg.sender, 1).expect("couldn't return WasSuspendClean result");
                    } else {
                        xous::return_scalar(msg.sender, 0).expect("couldn't return WasSuspendClean result");
                    }
                }),
                Some(Opcode::SuspendAllow) => {
                    allow_suspend = true;
                }
                Some(Opcode::SuspendDeny) => {
                    allow_suspend = false;
                }
                Some(Opcode::PowerOff) => msg_scalar_unpack!(msg, shipmode, _, _, _, {
                    if shipmode != 0 {
                        // this should be the full power down - no RTC, no nothing - disconnect the battery
                        // for shipment. Battery life should be "years" in this mode.
                        // clk_mgr.force_power_off();
                        todo!("implement force power off")
                    } else {
                        // this should be the deep sleep mode - battery is still connected, RTC running
                        // battery life is 100+ hours in this mode but not long enough for safe shipping
                        todo!("implement deep sleep")
                    }
                }),
                Some(Opcode::Quit) => break,
                None => {
                    log::error!("couldn't convert opcode");
                }
            }
        }
    }
}

fn send_event(
    cb_conns: &Vec<ScalarCallback>,
    order: xous_api_susres::SuspendOrder,
) -> (bool, xous_api_susres::SuspendOrder) {
    let mut at_least_one_event_sent = false;
    log::info!("Sending suspend to {:?} stage", order);
    /*
    // abortive attempt to get suspend to shut down the system. Doesn't work, results in a panic because too many messages are still moving around.
    #[cfg(not(target_os = "xous"))]
    {
        if order == crate::api::SuspendOrder::Last {
            let tt_conn = xous::connect(xous::SID::from_bytes(b"ticktimer-server").unwrap()).unwrap();
            send_message(
                tt_conn,
                xous::Message::new_blocking_scalar(
                    1,
                    1000,
                    0,
                    0,
                    0,
                ),
            )
            .map(|_| ()).unwrap();
            xous::rsyscall(xous::SysCall::Shutdown).expect("unable to quit");
        }
    }*/
    for scb in cb_conns.iter() {
        if scb.order == order {
            at_least_one_event_sent = true;
            xous::send_message(
                scb.server_to_cb_cid,
                xous::Message::new_scalar(
                    SuspendEventCallback::Event.to_usize().unwrap(),
                    scb.cb_to_client_cid as usize,
                    scb.cb_to_client_id as usize,
                    scb.token as usize,
                    0,
                ),
            )
            .unwrap();
        }
    }
    (at_least_one_event_sent, order.next())
}

fn do_hook(hookdata: ScalarHook, cb_conns: &mut Vec<ScalarCallback>) {
    let (s0, s1, s2, s3) = hookdata.sid;
    let sid = xous::SID::from_u32(s0, s1, s2, s3);
    let server_to_cb_cid = xous::connect(sid).unwrap();
    let cb_dat = ScalarCallback {
        server_to_cb_cid,
        cb_to_client_cid: hookdata.cid,
        cb_to_client_id: hookdata.id,
        ready_to_suspend: false,
        token: cb_conns.len() as u32,
        failed_to_suspend: false,
        order: hookdata.order,
    };
    log::trace!("hooking {:?}", cb_dat);
    cb_conns.push(cb_dat);
}
