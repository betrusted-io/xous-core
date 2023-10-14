#![cfg_attr(not(target_os = "none"), allow(dead_code))]
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use xous_api_ticktimer::*;
#[cfg(feature = "timestamp")]
mod version;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use log::{error, info};

mod platform;
use platform::implementation::*;
use platform::*;

#[cfg(not(any(target_arch = "arm", feature = "cramium-soc", feature = "cramium-fpga")))]
use susres::SuspendOrder;

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    #[cfg(feature = "timestamp")]
    {
        log::info!("****************************************************************");
        log::info!("Welcome to Xous {}", version::SEMVER);
        log::info!("Built on {}", version::TIMESTAMP);
        log::info!("****************************************************************");
    }
    #[cfg(not(feature = "timestamp"))]
    {
        log::info!("****************************************************************");
        log::info!("Welcome to Xous");
        log::info!("Reproducible build without timestamps");
        log::info!("****************************************************************");
    }

    let ticktimer_server = xous::create_server_with_address(b"ticktimer-server")
        .expect("Couldn't create Ticktimer server");
    info!("Server started with SID {:?}", ticktimer_server);

    // Connect to our own server so we can send the "Recalculate" message
    let ticktimer_client = xous::connect(xous::SID::from_bytes(b"ticktimer-server").unwrap())
        .expect("couldn't connect to self");

    // Create a new ticktimer object
    let mut ticktimer = XousTickTimer::new(ticktimer_client);
    ticktimer.init(); // hooks interrupts; should be called only once.

    ticktimer.reset(); // reset the time to 0

    // register a suspend/resume listener
    #[cfg(not(any(target_arch = "arm", feature = "cramium-soc", feature = "cramium-fpga")))]
    let xns = xous_names::XousNames::new().unwrap();

    #[cfg(not(any(target_arch = "arm", feature = "cramium-soc", feature = "cramium-fpga")))]
    let sr_cid =
        xous::connect(ticktimer_server).expect("couldn't create suspend callback connection");

    #[cfg(not(any(target_arch = "arm", feature = "cramium-soc", feature = "cramium-fpga")))]
    let mut susres = susres::Susres::new(
        Some(SuspendOrder::Last),
        &xns,
        api::Opcode::SuspendResume as u32,
        sr_cid,
    )
    .expect("couldn't create suspend/resume object");

    // A list of all sleep requests in the system, sorted by the time at which it
    // expires. That is, if a request comes in to sleep for 1000 ms, and the ticktimer
    // is currently at 900, the Request will be `1900`.
    let mut sleep_heap: BTreeMap<TimeoutExpiry, TimerRequest> = BTreeMap::new();

    // A list of mutexes that should be allowed to run immediately, because the
    // thread they were waiting on has already unlocked the mutex. This occurs
    // when a thread attempts to Lock a Mutex and fails, then gets preempted
    // before it has a chance to send the `LockMutex` message to us.
    let mut mutex_ready_table: HashMap<Option<xous::PID>, HashSet<usize>> = HashMap::new();

    // A list of message IDs that are waiting to receive a Notification. This queue is drained
    // by threads sending `NotifyCondition` to us, or by a condvar timing out.
    let mut notify_table: HashMap<
        Option<xous::PID>,
        HashMap<usize, VecDeque<xous::MessageSender>>,
    > = HashMap::new();

    // Keep track of which notifications have timeouts. This is used to determine which
    // notifications have already been responded to by the interrupt, as well as to
    // improve performance when recalculating timeouts.
    let mut notifications_with_timeouts: HashSet<usize> = HashSet::new();

    // A list of message IDs that are waiting to lock a Mutex. These are processes
    // that have attempted to lock a Mutex and failed, and have sent us the `LockMutex`
    // message. This queue is drained by threads sending `UnlockMutex` to us.
    let mut mutex_hash: HashMap<Option<xous::PID>, HashMap<usize, VecDeque<xous::MessageSender>>> =
        HashMap::new();

    let mut msg_opt = None;
    let mut return_type = 0;
    loop {
        #[cfg(feature = "watchdog")]
        ticktimer.reset_wdt();
        //#[cfg(feature = "watchdog")] // for debugging the watchdog
        //ticktimer.check_wdt();

        xous::reply_and_receive_next_legacy(ticktimer_server, &mut msg_opt, &mut return_type)
            .unwrap();
        let msg = msg_opt.as_mut().unwrap();
        let opcode = num_traits::FromPrimitive::from_usize(msg.body.id())
            .unwrap_or(api::Opcode::InvalidCall);
        log::trace!("msg {:?}: {:x?}", opcode, msg);
        match opcode {
            api::Opcode::ElapsedMs => {
                if let Some(scalar) = msg.body.scalar_message_mut() {
                    let time = ticktimer.elapsed_ms() as i64;
                    scalar.arg1 = (time & 0xFFFF_FFFFi64) as usize;
                    scalar.arg2 = ((time >> 32) & 0xFFF_FFFFi64) as usize;
                    scalar.id = 0;

                    // API calls expect a `Scalar2` value in response
                    return_type = 2;
                }
            }

            api::Opcode::SleepMs => {
                if let Some(scalar) = msg.body.scalar_message_mut() {
                    let ms = scalar.arg1 as i64;
                    let sender = msg.sender;

                    // Forget the contents of the message, replacing it with `None`. This
                    // will prevent it from getting dropped, which (under normal circumstances)
                    // would cause it to be responded to automatically.
                    //
                    // The ideal approach here would be to have the struct -- in this case the
                    // TimerRequest -- keep track of the element itself. That way, it could
                    // return the value as part of its response routine. However, due to the
                    // legacy split of `BlockingScalars`, these are allowed to have an explicit
                    // return path, so we need to forget the original message here to prevent
                    // it from getting returned early.
                    //
                    // (n.b. this auto-response hasn't been enabled yet for Scalar messages,
                    // but this definitely would need to be done for e.g. Memory messages).
                    core::mem::forget(msg_opt.take());

                    // let timeout_queue = timeout_heap.entry(msg.sender.pid()).or_default();
                    ticktimer.recalculate_sleep(
                        &mut sleep_heap,
                        Some(TimerRequest {
                            msec: ms.into(),
                            sender,
                            kind: RequestKind::Sleep,
                            data: 0,
                        }),
                    );
                }
            }

            api::Opcode::RecalculateSleep => {
                if msg.sender.pid().map(|p| p.get()).unwrap_or_default() as u32
                    != xous::process::id()
                {
                    log::error!("got a RecalculateSleep message from a process other than the ticktimer server");
                    continue;
                }

                let Some(args) = msg.body.scalar_message() else {
                    continue;
                };

                // If this is a Timeout message that fired, remove it from the Notification list
                let sender_id = args.arg1;
                let sender = xous::MessageSender::from_usize(sender_id);
                let condvar = args.arg3;
                ticktimer.stop_sleep(&mut sleep_heap);
                if notifications_with_timeouts.remove(&sender_id) {
                    // Check to make sure this isn't in the sleep heap. It shouldn't be,
                    // since the timer just fired.
                    let len_before = sleep_heap.len();
                    sleep_heap.retain(|_, v| v.sender != sender);
                    let len_after = sleep_heap.len();
                    assert!(len_before == len_after);

                    // It should, however, still be in the notify hash. Remove it.
                    let awaiting = notify_table
                        .entry(sender.pid())
                        .or_default()
                        .entry(condvar)
                        .or_default();
                    let len_before = awaiting.len();
                    awaiting.retain(|v| *v != sender);
                    let len_after = awaiting.len();
                    assert!(len_before != len_after);

                    // Note multiple events elapsing since the last recalculate. Theorized to be harmless.
                    if ticktimer.last_response() != sender {
                        log::warn!("Multiple events triggered before we could recalculate sleep: ticktimer.last_response() {:?} != sender {:?}; sleep_heap.len(): {}",
                            ticktimer.last_response(), sender, sleep_heap.len()
                        );
                    }
                }

                // Recalculate sleep with the newly-adjusted hash and re-enable
                // the sleep interrupt.
                unsafe { ticktimer.recalculate_sleep_offline(&mut sleep_heap, None) };
                ticktimer.start_sleep(&mut sleep_heap);
            }

            api::Opcode::SuspendResume => xous::msg_scalar_unpack!(msg, _token, _, _, _, {
                ticktimer.suspend();
                #[cfg(not(any(
                    target_arch = "arm",
                    feature = "cramium-soc",
                    feature = "cramium-fpga"
                )))]
                susres
                    .suspend_until_resume(_token)
                    .expect("couldn't execute suspend/resume");
                ticktimer.resume();
            }),

            api::Opcode::PingWdt => {
                #[cfg(feature = "watchdog")]
                ticktimer.reset_wdt();
            }

            api::Opcode::GetVersion => {
                let mut buf = unsafe {
                    xous_ipc::Buffer::from_memory_message_mut(
                        msg.body.memory_message_mut().unwrap(),
                    )
                };
                #[cfg(feature = "timestamp")]
                buf.replace(version::get_version()).unwrap();
                #[cfg(not(feature = "timestamp"))]
                {
                    let v = crate::api::VersionString {
                        version: xous_ipc::String::from_str("--no-timestamp requested for build"),
                    };
                    buf.replace(v).unwrap();
                }
            }

            api::Opcode::LockMutex => {
                let Some(scalar) = msg.body.scalar_message_mut() else {
                    log::error!("sender made LockMutex request that was not blocking");
                    continue;
                };

                let pid = msg.sender.pid();
                let mutex_id = scalar.arg1;

                // If this item is in the Ready list, then someone unlocked this Mutex after the
                // sender locked it but before they got a chance to send this message. Return
                // immediately without blocking.
                if mutex_ready_table.entry(pid).or_default().remove(&mutex_id) {
                    scalar.id = 0;
                    continue;
                }

                // Add this to the end of the list of entries to call so that when `UnlockMutex` is sent
                // the message will get a response.
                mutex_hash
                    .entry(pid)
                    .or_default()
                    .entry(mutex_id)
                    .or_default()
                    .push_back(msg.sender);

                // We've saved the `msg.sender` above and will return the lock as part of the mutex
                // work thread. Forget the contents of `msg_opt` without running its destructor.
                core::mem::forget(msg_opt.take());
            }

            api::Opcode::UnlockMutex => {
                // if msg.body.is_blocking() {
                //     log::error!("sender made UnlockMutex request that was blocking");
                //     continue;
                // }
                let Some(scalar) = msg.body.scalar_message() else {
                    log::error!("made a call to UnlockMutex with a non-scalar message");
                    continue;
                };

                let pid = msg.sender.pid();

                // If there's something waiting in the queue, respond to that message
                let mut returned = false;
                if let Some(sender) = mutex_hash
                    .entry(pid)
                    .or_default()
                    .entry(scalar.arg1)
                    .or_default()
                    .pop_front()
                {
                    xous::return_scalar(sender, 0).unwrap();
                    returned = true;
                } else {
                    log::warn!(
                        "Process {} Attempted to unlock a mutex {:08x} that has not yet been locked",
                        pid.map(|v| v.get()).unwrap_or_default(),
                        scalar.arg1
                    );
                    if !mutex_ready_table
                        .entry(pid)
                        .or_default()
                        .insert(scalar.arg1)
                    {
                        log::error!("attempted to ready mutex {:08x} for PID {:?}, but there was already one waiting", scalar.arg1, pid);
                    }
                }

                if let Some(scalar) = msg.body.scalar_message_mut() {
                    scalar.id = if returned { 1 } else { 0 };
                }
            }

            api::Opcode::FreeMutex => {
                let Some(scalar) = msg.body.scalar_message() else {
                    log::error!("sender tried to free a mutex using a non-scalar message");
                    continue;
                };

                let pid = msg.sender.pid();

                // Remove all instances of this mutex from the mutex hash
                if let Some(responders) = mutex_hash.entry(pid).or_default().remove(&scalar.arg1) {
                    if responders.len() != 0 {
                        log::error!("When freeing mutex {:08x}, there were {} mutexes awaiting to be unlocked", scalar.arg1, responders.len());
                    }
                }

                if mutex_ready_table
                    .entry(pid)
                    .or_default()
                    .remove(&scalar.arg1)
                {
                    log::error!(
                        "When freeing mutex {:08x}, there was one pending unlock request that hasn't been answered",
                        scalar.arg1,
                    );
                }
            }

            api::Opcode::WaitForCondition => {
                let pid = msg.sender.pid();
                let Some(scalar) = msg.body.scalar_message_mut() else {
                    log::trace!(
                        "sender made WaitForCondition request that wasn't a BlockingScalar Message"
                    );
                    continue;
                };

                let condvar = scalar.arg1;
                let timeout = scalar.arg2;

                log::trace!(
                    "sender in pid {:?} is waiting on a condition {:08x} with a timeout of {}",
                    pid,
                    condvar,
                    timeout
                );

                // If there's a `timeout` argument, schedule a response. Also add the response
                // to a list of notifications that have timeouts, in order to speed up
                // recalculations in the future and to avoid sending duplicate responses.
                if timeout != 0 {
                    ticktimer.stop_sleep(&mut sleep_heap);
                    notifications_with_timeouts.insert(msg.sender.to_usize());

                    // Safety: The ticktimer must be stopped, and we have done that above.
                    unsafe {
                        ticktimer.recalculate_sleep_offline(
                            &mut sleep_heap,
                            Some(TimerRequest {
                                msec: timeout.into(),
                                sender: msg.sender,
                                kind: RequestKind::Timeout,
                                data: condvar,
                            }),
                        );
                    }
                }

                // Add this sender to the global notification table. This table is used
                // when notifying senders, which will unblock them and wake them up.
                notify_table
                    .entry(pid)
                    .or_default()
                    .entry(condvar)
                    .or_default()
                    .push_back(msg.sender);

                // If there is a timeout, then the ticktimer was paused above. Resume
                // the ticktimer now that everything is settled.
                if timeout != 0 {
                    ticktimer.start_sleep(&mut sleep_heap);
                }

                // log::trace!("New waiting senders: {:?}", notify_hash.get(&pid).unwrap().get(&condvar));

                // The message will be responded to as part of the notification hash
                // when the condvar is unlocked. Forget the contents of the message
                // in order to prevent it from being responded to early.
                core::mem::forget(msg_opt.take());
            }

            api::Opcode::NotifyCondition => {
                let pid = msg.sender.pid();
                let Some(scalar) = msg.body.scalar_message() else {
                    log::error!("sender made NotifyCondition request that wasn't Scalar");
                    continue;
                };

                let condvar = scalar.arg1;
                let mut requested_count: usize = scalar.arg2;

                let awaiting = notify_table
                    .entry(pid)
                    .or_default()
                    .entry(condvar)
                    .or_default();

                // As a special case, if 0 conditions are requested to wake, notify all
                // waiting conditions.
                if requested_count == 0 {
                    requested_count = awaiting.len();
                }

                // Wake threads, ensuring we don't run off the end.
                let count_to_notify = core::cmp::min(requested_count, awaiting.len());

                log::trace!(
                    "sender in pid {:?} is notifying {} entries for condition {:08x} -- there are {} entries waiting, so we'll wake up {}",
                    pid,
                    requested_count,
                    condvar,
                    awaiting.len(),
                    count_to_notify,
                );

                // Stop the ticktimer interrupt, in case the condvar that we're waking up
                // had a timeout associated with it.
                ticktimer.stop_sleep(&mut sleep_heap);

                let mut notified_count = 0;
                for entry in awaiting.drain(..count_to_notify) {
                    // If this entry had a timeout associated with it, see if it needs to
                    // be removed from the sleep heap.
                    if notifications_with_timeouts.remove(&entry.to_usize()) {
                        // Remove the entries from the sleep heap. There will either be no entries
                        // removed, in which case we're waiting for a `RecalculateSleep` message
                        // to be processed, or there will be exactly one entry missing.

                        let len_before = sleep_heap.len();
                        sleep_heap.retain(|_, v| v.sender != entry);
                        let len_after = sleep_heap.len();
                        assert!((len_before == len_after) || (len_before == len_after + 1));

                        // If the lengths are not equal, then our entry was in the sleep heap.
                        // Presumably, it was responded to in the interrim, we just haven't
                        // recalculated the sleep heap since then.
                        if len_before != len_after {
                            // assert!(ticktimer.last_response() != entry);
                            // We removed an item from the sleep heap. This means that the entry
                            // didn't time out, and we should respond to it here.
                        } else {
                            // assert!(ticktimer.last_response() == entry);
                            // If no entry was removed, then the entry wasn't in the sleep
                            // heap because it was queued to fire, and has already fired.
                            // Don't respond to it here a second time.
                            continue;
                        }
                    }

                    // Wake up the process that was waiting on this entry. Be sure to send
                    // a value of `0`, which indicates success.
                    match xous::return_scalar(entry, 0) {
                        Ok(_) => {
                            notified_count += 1;
                        }
                        Err(xous::Error::ProcessNotFound) => {
                            log::error!(
                                "process {} exited -- removing remaining entries for PID {:?}/condvar {:08x}",
                                pid.map(|v| v.get()).unwrap_or_default(), pid, condvar
                            );
                            mutex_hash.remove(&pid);
                        }
                        Err(xous::Error::DoubleFree) => {
                            panic!("tried to wake up a thread twice");
                        }
                        Err(e) => panic!("unexpected error responding to scalar: {:?}", e),
                    }
                }

                // Resume sleeping, which re-enables interrupts and queues the
                // next timer event to fire.
                ticktimer.start_sleep(&mut sleep_heap);

                // Return the number of conditions that were notified
                if let Some(return_value) = msg.body.scalar_message_mut() {
                    return_value.id = notified_count;
                }
            }

            api::Opcode::FreeCondition => {
                let pid = msg.sender.pid();
                let Some(scalar) = msg.body.scalar_message() else {
                    info!(
                        "sender made FreeCondition request that wasn't a Scalar or BlockingScalar Message"
                    );
                    continue;
                };

                if msg.body.is_blocking() {
                    return_type = 0;
                }

                let condvar = scalar.arg1;

                let awaiting = notify_table
                    .entry(pid)
                    .or_default()
                    .entry(condvar)
                    .or_default();

                if !awaiting.is_empty() {
                    log::info!(
                        "PID {}: freeing condvar {:08x} -- {} entries waiting",
                        pid.map(|v| v.get()).unwrap_or_default(),
                        condvar,
                        awaiting.len()
                    );
                }

                // Free all entries in the sleep heap that are waiting for this condition
                ticktimer.stop_sleep(&mut sleep_heap);
                let mut altered_sleep_heap = false;
                for entry in awaiting.drain(..) {
                    // Remove each entry in the timeout set
                    if notifications_with_timeouts.remove(&entry.to_usize()) {
                        let len_before = sleep_heap.len();
                        sleep_heap.retain(|_, v| v.sender != entry);
                        let len_after = sleep_heap.len();
                        assert!((len_before == len_after) || (len_before == len_after + 1));

                        // If the lengths are not equal, then our entry was in the sleep heap.
                        // Presumably, it was responded to in the interrim, we just haven't
                        // recalculated the sleep heap since then.
                        if len_before != len_after {
                            assert!(ticktimer.last_response() != entry);
                            altered_sleep_heap = true;
                        } else {
                            assert!(ticktimer.last_response() == entry);
                            log::error!(
                                "got a request to destroy a condvar ({:08x}) that we just responded to with entry {:08x} -- suspect there's a thread waiting in the server queue to clean it up",
                            condvar, entry.to_usize());
                            continue;
                        }
                    }

                    // Wake up any responders that are waiting on the condvar
                    xous::return_scalar(entry, 0).ok();
                }

                // Remove all instances of this condvar
                notify_table.entry(pid).or_default().remove(&condvar);

                // If we removed entries from the sleep heap, recalculate it before resuming
                // sleep.
                if altered_sleep_heap {
                    unsafe { ticktimer.recalculate_sleep_offline(&mut sleep_heap, None) };
                }

                // Resume sleeping
                ticktimer.start_sleep(&mut sleep_heap);
            }

            api::Opcode::InvalidCall => {
                error!("couldn't convert opcode");
            }
        }
    }
}
