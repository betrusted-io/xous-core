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

#[cfg(not(any(target_arch = "arm", feature="cramium-soc", feature="cramium-fpga")))]
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
    #[cfg(not(any(target_arch = "arm", feature="cramium-soc", feature="cramium-fpga")))]
    let xns = xous_names::XousNames::new().unwrap();

    #[cfg(not(any(target_arch = "arm", feature="cramium-soc", feature="cramium-fpga")))]
    let sr_cid =
        xous::connect(ticktimer_server).expect("couldn't create suspend callback connection");

    #[cfg(not(any(target_arch = "arm", feature="cramium-soc", feature="cramium-fpga")))]
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

    // A list of message IDs that are waiting to receive a Notification. This queue is drained
    // by threads sending `NotifyCondition` to us, or by a condvar timing out.
    let mut notify_hash: HashMap<Option<xous::PID>, HashMap<usize, VecDeque<xous::MessageSender>>> =
        HashMap::new();

    // There is a small chance that a client sends a `notify_one()` or `notify_all()` before
    // the other threads have a chance to recover. This is due to a non-threadsafe use of
    // Mutex<T> within the standard library. Keep track of any excess `notify_one()` or
    // `notify_all()` messages for when the Mutex<T> is successfully locked.
    let mut immedaite_notifications: HashMap<Option<xous::PID>, HashMap<usize, usize>> =
        HashMap::new();

    // // A list of timeouts for a given condvar. This list serves two purposes:
    // //      1. When a Notification is sent to a condvar, it is removed from this hash (based on the Message ID)
    // //      2. When the timer event hits, a response is sent to this condvar
    // // Therefore, this must be indexable by two keys: Message ID and
    // let mut timeout_heap: std::collections::VecDeque<TimerRequest> = VecDeque::new();

    // A list of mutexes that should be allowed to run immediately, because the
    // thread they were waiting on has already unlocked the mutex. This occurs
    // when a thread attempts to Lock a Mutex and fails, then gets preempted
    // before it has a chance to send the `LockMutex` message to us.
    let mut mutex_ready_hash: HashMap<Option<xous::PID>, HashSet<usize>> = HashMap::new();

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
        log::trace!("msg: {:x?}", msg);
        match num_traits::FromPrimitive::from_usize(msg.body.id())
            .unwrap_or(api::Opcode::InvalidCall)
        {
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
                            msec: ms,
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
                    ticktimer.recalculate_sleep(&mut sleep_heap, None);
                    continue;
                }

                let Some(args) = msg.body.scalar_message() else {
                    continue
                };

                // If this is a Timeout message that fired, remove it from the Notification list
                let sender = args.arg1;
                let request_kind = args.arg2;
                let condvar = args.arg3;
                let sender_pid = xous::MessageSender::from_usize(sender).pid();

                // If we're being asked to recalculate due to a timeout expiring, drop the sent
                // message from the `entries` list.
                if (request_kind == RequestKind::Timeout as usize) && (sender != 0) {
                    let mut expired_values = 0;
                    let awaiting = notify_hash
                        .entry(sender_pid)
                        .or_default()
                        .entry(condvar)
                        .or_default();

                    awaiting.retain(|e| {
                        if e.to_usize() == sender {
                            expired_values += 1;
                            false
                        } else {
                            true
                        }
                    });

                    // Remove each entry in the timeout set
                    for (_msecs, timer_request) in sleep_heap.iter() {
                        if timer_request.sender.to_usize() == sender {
                            log::error!("we were just notified of PID {:?}/condvar {:08x} expiring, yet it's still in the sleep heap with sender {:08x}", sender_pid, condvar, timer_request.sender.to_usize());
                        }
                    }

                    // log::info!(
                    //     "removed {} entries for PID {:?}/condvar {:08x}, {} to start, {} remain",
                    //     expired_values,
                    //     sender_pid,
                    //     condvar,
                    //     len_before,
                    //     awaiting.len(),
                    // );
                    // log::trace!("new entries for PID {:?}/condvar {:08x}: {:?}", sender_pid, condvar, notify_hash.get(&sender_pid).unwrap().get(&condvar));
                }
                ticktimer.recalculate_sleep(&mut sleep_heap, None);
            }

            api::Opcode::SuspendResume => xous::msg_scalar_unpack!(msg, _token, _, _, _, {
                ticktimer.suspend();
                #[cfg(not(any(target_arch = "arm", feature="cramium-soc", feature="cramium-fpga")))]
                susres
                    .suspend_until_resume(_token)
                    .expect("couldn't execute suspend/resume");
                ticktimer.resume();
            }),

            api::Opcode::PingWdt => {
                #[cfg(feature="watchdog")]
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

                let ready = mutex_ready_hash.entry(pid).or_default();

                // If this item is in the Ready list, return right away without blocking
                if ready.remove(&scalar.arg1) {
                    scalar.id = 0;
                    continue;
                }

                // This item is not in the Ready list, so add our sender to the list of processes
                // to get called when UnlockMutex is invoked
                let awaiting = mutex_hash.entry(pid).or_default();

                // Add this to the end of the list of entries to call so that when `UnlockMutex` is sent
                // the message will get a response.
                let mutex_entry = awaiting.entry(scalar.arg1).or_default();
                mutex_entry.push_back(msg.sender);

                // We've saved the `msg.sender` above and will return the lock as part of the mutex
                // work thread. Forget the contents of `msg_opt` without running its destructor.
                core::mem::forget(msg_opt.take());
            }

            api::Opcode::UnlockMutex => {
                if msg.body.is_blocking() {
                    log::error!("sender made UnlockMutex request that was blocking");
                    continue;
                }
                let Some(scalar) = msg.body.scalar_message() else {
                    log::error!("made a call to UnlockMutex with a non-scalar message");
                    continue;
                };

                let pid = msg.sender.pid();

                // Get a list of awaiting mutexes for this process
                let awaiting = mutex_hash.entry(pid).or_default();

                // Get the vector of awaiting mutex entries.
                let mutex_entry = awaiting.entry(scalar.arg1).or_default();

                // If there's something waiting in the queue, respond to that message
                if let Some(sender) = mutex_entry.pop_front() {
                    xous::return_scalar(sender, 0).unwrap();
                } else {
                    // Otherwise, mark this scalar as being ready to run
                    mutex_ready_hash.entry(pid).or_default().insert(scalar.arg1);
                }
            }

            api::Opcode::FreeMutex => {
                let Some(scalar) = msg.body.scalar_message() else {
                    log::error!("sender tried to free a mutex using a non-scalar message");
                    continue;
                };

                // log::info!(
                //     "PID {}: freeing mutex {}",
                //     msg.sender.pid().map(|v| v.get()).unwrap_or_default(),
                //     scalar.arg1
                // );

                // Remove all instances of this mutex from the mutex hash
                mutex_hash
                    .entry(msg.sender.pid())
                    .or_default()
                    .remove(&scalar.arg1);
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

                // If there's a condition waiting already, decrement the total list
                // and return immediately.
                if let Some(excess) = immedaite_notifications
                    .entry(pid)
                    .or_default()
                    .get_mut(&condvar)
                {
                    if *excess > 0 {
                        *excess -= 1;
                        scalar.id = 0;

                        // Rust libstd expects a `Scalar1` return type for this call
                        return_type = 1;
                        continue;
                    }
                }

                // Add this to the list of entries waiting for a response.
                notify_hash
                    .entry(pid)
                    .or_default()
                    .entry(condvar)
                    .or_default()
                    .push_back(msg.sender);

                // If there's a `timeout` argument, schedule a response.
                if timeout != 0 {
                    ticktimer.recalculate_sleep(
                        &mut sleep_heap,
                        Some(TimerRequest {
                            msec: timeout as i64,
                            sender: msg.sender,
                            kind: RequestKind::Timeout,
                            data: condvar,
                        }),
                    )
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
                    log::error!(
                        "sender made WaitForCondition request that wasn't a Scalar or BlockingScalar Message"
                    );
                    continue;
                };

                if msg.body.is_blocking() {
                    return_type = 0;
                }

                let condvar = scalar.arg1;

                log::trace!(
                    "sender in pid {:?} is notifying {} entries for condition {:08x}",
                    pid,
                    scalar.arg2,
                    condvar,
                );

                let awaiting = notify_hash
                    .entry(pid)
                    .or_default()
                    .entry(condvar)
                    .or_default();

                // Wake threads, ensuring we don't run off the end.
                let requested_count = scalar.arg2;
                let available_count = core::cmp::min(requested_count, awaiting.len());
                let remaining_count = awaiting.len() - available_count;

                if remaining_count > 0 {
                    log::error!("requested to wake {} entries, which is more than the current {} waiting entries", requested_count, awaiting.len());
                    if let Some(entry) = ticktimer.last_response() {
                        log::error!("but there is a last_response present");
                        if entry.data == condvar {
                            log::error!("...which matches the condvar that was being unlocked");
                        }
                    }
                }
                if requested_count == 0 {
                    log::error!("requested to wake no entries!");
                }

                ticktimer.stop_sleep(&mut sleep_heap);
                for entry in awaiting.drain(..available_count) {
                    // Remove each entry in the timeout set
                    sleep_heap.retain(|_, v| v.sender != entry);

                    if let Some(last_response) = ticktimer.last_response() {
                        if last_response.sender == entry {
                            log::error!(
                                "got a request to wake a condvar ({:08x}) that we just responded to with entry {:08x} -- suspect there's a thread waiting in the server queue to clean it up",
                            condvar, entry.to_usize());
                            continue;
                        }
                    }

                    // If there's an error waking up the sender, deal with it
                    match xous::return_scalar(entry, 0) {
                        Ok(_) => {}
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

                // If there are leftover requested, add them to the list of
                // notofications that will be responded to immediately.
                if available_count - requested_count > 0 {
                    #[cfg(feature = "debug-print")]
                    log::trace!(
                        "Adding {} spare sleep requests to immediate_notifications list",
                        available_count - requested_count
                    );
                    *immedaite_notifications
                        .entry(pid)
                        .or_default()
                        .entry(condvar)
                        .or_default() += available_count - requested_count;
                }

                // Resume sleeping, which re-enables interrupts and queues the
                // next timer event to fire.
                ticktimer.start_sleep(&mut sleep_heap);
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

                let awaiting = notify_hash
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
                for entry in awaiting.drain(..) {
                    // Remove each entry in the timeout set
                    sleep_heap.retain(|_, v| v.sender != entry);

                    if let Some(last_response) = ticktimer.last_response() {
                        if last_response.sender == entry {
                            log::error!(
                                "got a request to destroy a condvar ({:08x}) that we just responded to with entry {:08x} -- suspect there's a thread waiting in the server queue to clean it up",
                            condvar, entry.to_usize());
                            continue;
                        }
                    }

                    // If there's an error waking up the sender, deal with it
                    xous::return_scalar(entry, 0).ok();
                }

                // Remove all instances of this condvar
                notify_hash.entry(pid).or_default().remove(&condvar);

                let immediate_notifications = immedaite_notifications
                    .entry(pid)
                    .or_default()
                    .remove(&condvar)
                    .unwrap_or_default();
                if immediate_notifications != 0 {
                    log::error!(
                        "there were {} threads that were notified without first waiting",
                        immediate_notifications
                    );
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
