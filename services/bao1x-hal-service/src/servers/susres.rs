use core::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, atomic::AtomicBool};

use bao1x_api::divide_by_fd;
use bao1x_hal::clocks::{ClockOp, fd_from_frequency};
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

    let mut clk_mgr = bao1x_hal::clocks::ClockManagerImpl::new().unwrap();
    let measured = clk_mgr.measured_freqs();
    log::debug!("computed frequencies:");
    log::debug!("  vco: {}", clk_mgr.vco_freq);
    log::debug!("  fclk: {}", clk_mgr.fclk);
    log::debug!("  aclk: {}", clk_mgr.aclk);
    log::debug!("  hclk: {}", clk_mgr.hclk);
    log::debug!("  iclk: {}", clk_mgr.iclk);
    log::debug!("  pclk: {}", clk_mgr.pclk);
    log::debug!("  per: {}", clk_mgr.perclk);
    log::debug!("measured frequencies:");
    for (name, freq) in measured {
        log::debug!("  {}: {} MHz", name, freq);
    }
    let hal = bao1x_hal_service::Hal::new();

    let timeout_sid = xous::create_server().unwrap();
    let timeout_outgoing_conn = xous::connect(timeout_sid).unwrap();
    let timeout_pending = Arc::new(AtomicBool::new(false));
    std::thread::spawn({
        let timeout_pending = timeout_pending.clone();
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
                let mut reached_timeout = true;
                while ((get_hw_time(&hw) - start) as u32) < timeout {
                    // log::info!("delta t: {}", (get_hw_time(hw) - start) as u32);
                    xous::yield_slice();
                    if !timeout_pending.load(Ordering::SeqCst) {
                        // remember the check here so we don't have inconsistent state with the send_message
                        // later
                        reached_timeout = false;
                        break;
                    }
                }

                if reached_timeout {
                    log::trace!("HW timeout reached");
                    send_message(
                        susres_conn,
                        Message::new_scalar(Opcode::SuspendTimeout.to_usize().unwrap(), 0, 0, 0, 0),
                    )
                    .ok();
                }
            }
        }
    });
    let mut suspend_requested: Option<Sender> = None;
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
                        // this stops the timeout timer from going
                        timeout_pending.store(false, Ordering::SeqCst);
                        // allow the state to propagate to the timer
                        xous::yield_slice();

                        // turn off preemption
                        hal.set_preemption(false);
                        log::info!("all callbacks reporting in, doing suspend");

                        use bao1x_api::bio::BioApi;
                        let mut bio = bao1x_hal::bio::Bio::new();

                        // hard-coded to the value of the external crystal - this is a hardware
                        // reference value that should never change; eg, USB doesn't work if this
                        // isn't 48 MHz.
                        let fclk_fd_backup = clk_mgr.fclk_fd();

                        // TODO: this is a hack to get past some issue with clock rate changes
                        //
                        // In theory:
                        //   - the set_clk_fd(0xff) call should reset the fclk divider to 1:1
                        //   - this would mean the bio_freq is getting 48MHz
                        // In practice:
                        //   - the fd seems "stuck" at /2 for fd settings from 0xff-0x3f
                        //      - so the pulse width outputs on BIO is the same for 0xff, 0x7f, 0x3f
                        //   - if I reduce reduce fd setting to 0x1f, then, I can see the clock rate is 1/8th
                        // What has been tried:
                        //   - Forcing the clocks to a fixed 0xFF setting regardless of input - I can see that
                        //     BIO clocks, after wfi, are incorrect in this case. Which means that set_fclk_fd
                        //     is "getting through"
                        //   - Adding wait states, etc. around the CGUSET call does not seem to change the
                        //     situation
                        //
                        // The compromise for now is to detect the "fast_bio" setting at init by heuristically
                        // looking at the fd_backup field, and then specifying an offset frequency to the
                        // update_bio_freq that results in the correct waveform.
                        //
                        // Possible cause:
                        //   - The update_bio_freq routine is being "too clever" and compensating for the fd
                        //     setting. But it seems like the routine just takes the putative fclk setting as
                        //     the argument to update_bio_freq(), I don't see any compensation happening...
                        bio.prep_freq_change(true);
                        bio.update_bio_freq(48_000_000);
                        clk_mgr.wfi();

                        // ~~~ time passes, but we're on carbonite so we don't notice ~~~

                        bio.prep_freq_change(false);
                        clk_mgr.set_fclk_fd(fclk_fd_backup); // restore the FD for fclk, as it is side-effected by WFI
                        // fast PLL setting
                        clk_mgr.restore_wfi();
                        bio.update_bio_freq(clk_mgr.fclk);

                        // when wfi() returns, it means we've resumed
                        let sender = suspend_requested
                            .take()
                            .expect("suspend_requested was checked to be Some() at entry to the loop, but now somehow it's now None!");

                        log_server::resume(); // log server is a special case, in order to avoid circular dependencies

                        // this now allows all other threads to commence
                        log::info!("low-level resume done, restoring execution");
                        for pid in gated_pids.drain(..) {
                            xous::return_scalar(pid, 0).ok();
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
                Some(Opcode::SuspendRequest) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                    log::debug!("registered suspend listeners:");
                    for sub in suspend_subscribers.iter() {
                        log::debug!("{:?}", sub);
                    }
                    // if the timeout is still pending from a previous suspend, deny the suspend
                    // request. ...just don't suspend that quickly after resuming???
                    //
                    // note that short-circuit eval means timeout_pending is not side-effected if
                    // allow_spend is false. Do not change the order of these operations!
                    if allow_suspend && !timeout_pending.swap(true, Ordering::SeqCst) {
                        suspend_requested = Some(msg.sender);

                        // clear the ready to suspend flag and failed to suspend flag
                        for sub in suspend_subscribers.iter_mut() {
                            sub.ready_to_suspend = false;
                            sub.failed_to_suspend = false;
                        }

                        // any message to the timeout server will trigger it - it only has one function
                        xous::try_send_message(timeout_outgoing_conn, Message::new_scalar(0, 0, 0, 0, 0))
                            .ok();

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
                }),
                Some(Opcode::SuspendTimeout) => {
                    if timeout_pending.swap(false, Ordering::SeqCst) {
                        // record which tokens had not reported in
                        for sub in suspend_subscribers.iter_mut() {
                            sub.failed_to_suspend = !sub.ready_to_suspend;
                        }
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

                        for pid in gated_pids.drain(..) {
                            xous::return_scalar(pid, 0)
                                .expect("couldn't return dummy message to unblock execution");
                        }

                        // this unblocks the requestor of the suspend
                        if let Some(sender) = suspend_requested.take() {
                            xous::return_scalar(sender, 0).ok();
                        } else {
                            log::error!("Suspend requester disappeared - race condition somewhere!");
                        }
                    } else {
                        log::warn!("clean suspend timeout received, ignoring");
                        // this means we did a clean suspend, we've resumed, and the timeout came back after
                        // the resume just ignore the message. It's a warning because this message should,
                        // in practice, never be generated due to the timeout_pending semaphore.
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
                Some(Opcode::PlatformSpecific) => {
                    if let xous::Message::BlockingScalar(xous::ScalarMessage {
                        id: _id,
                        arg1: op,
                        arg2,
                        arg3: _arg3,
                        arg4: _arg4,
                    }) = msg.body
                    {
                        let platform_op = FromPrimitive::from_usize(op);
                        match platform_op {
                            Some(ClockOp::GetVco) => {
                                xous::return_scalar(msg.sender, clk_mgr.vco_freq as usize).unwrap()
                            }
                            Some(ClockOp::GetFclk) => {
                                xous::return_scalar(msg.sender, clk_mgr.fclk as usize).unwrap()
                            }
                            Some(ClockOp::GetAclk) => {
                                xous::return_scalar(msg.sender, clk_mgr.aclk as usize).unwrap()
                            }
                            Some(ClockOp::GetHclk) => {
                                xous::return_scalar(msg.sender, clk_mgr.hclk as usize).unwrap()
                            }
                            Some(ClockOp::GetIclk) => {
                                xous::return_scalar(msg.sender, clk_mgr.iclk as usize).unwrap()
                            }
                            Some(ClockOp::GetPclk) => {
                                xous::return_scalar(msg.sender, clk_mgr.pclk as usize).unwrap()
                            }
                            Some(ClockOp::GetPer) => {
                                xous::return_scalar(msg.sender, clk_mgr.perclk as usize).unwrap()
                            }
                            Some(ClockOp::SetFclk) => {
                                const MAX_DEVIATION: f32 = 0.05;
                                let requested_freq = arg2 as u32;
                                let fclk_fd = fd_from_frequency(requested_freq, clk_mgr.clktop);
                                let achievable_freq = divide_by_fd(fclk_fd, clk_mgr.clktop);
                                if (1.0 - (achievable_freq as f32 / requested_freq as f32)).abs()
                                    < MAX_DEVIATION
                                {
                                    clk_mgr.set_fclk_fd(fclk_fd as u8);
                                    clk_mgr.update_clocks();
                                    log::info!("Changed fclk to {}", clk_mgr.fclk);
                                    xous::return_scalar2(msg.sender, 1, achievable_freq as usize).unwrap();
                                } else {
                                    // let user know closest achievable frequency
                                    xous::return_scalar2(msg.sender, 0, achievable_freq as usize).unwrap();
                                }
                            }
                            Some(ClockOp::ResetReason) => {
                                xous::return_scalar(msg.sender, clk_mgr.reset_reason().raw_value() as usize)
                                    .unwrap()
                            }
                            _ => panic!("Incorrect PlatformOp"),
                        }
                    }
                    if let xous::Message::Scalar(xous::ScalarMessage {
                        id: _id,
                        arg1: op,
                        arg2: _arg2,
                        arg3: _arg3,
                        arg4: _arg4,
                    }) = msg.body
                    {
                        let platform_op = FromPrimitive::from_usize(op);
                        match platform_op {
                            Some(ClockOp::DeepSleep) => {
                                clk_mgr.deep_sleep();
                                // system is shut down after this point, no code runs
                            }
                            _ => panic!("Incorrect PlatformOp"),
                        }
                    }
                }
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
    log::debug!("Sending suspend to {:?} stage", order);
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
