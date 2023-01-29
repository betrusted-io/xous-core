#![cfg_attr(not(target_os = "none"), allow(dead_code))]
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use xous_api_ticktimer::*;
#[cfg(feature = "timestamp")]
mod version;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use log::{error, info};

type TimeoutExpiry = i64;

#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub enum RequestKind {
    Sleep = 0,
    Timeout = 1,
}

#[derive(Eq)]
pub struct TimerRequest {
    msec: TimeoutExpiry,
    sender: xous::MessageSender,
    kind: RequestKind,
    data: usize,
}

impl core::fmt::Display for TimerRequest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "TimerRequest {{ msec: {}, {} }}", self.msec, self.sender)
    }
}

impl core::fmt::Debug for TimerRequest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "TimerRequest {{ msec: {}, {} }}", self.msec, self.sender)
    }
}

impl core::cmp::Ord for TimerRequest {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        if self.msec < other.msec {
            core::cmp::Ordering::Less
        } else if self.msec > other.msec {
            core::cmp::Ordering::Greater
        } else {
            self.sender.cmp(&other.sender)
        }
    }
}

impl core::cmp::PartialOrd for TimerRequest {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl core::cmp::PartialEq for TimerRequest {
    fn eq(&self, other: &Self) -> bool {
        self.msec == other.msec && self.sender == other.sender
    }
}

#[cfg(any(feature = "precursor", feature = "renode"))]
mod implementation {
    const TICKS_PER_MS: u64 = 1;
    use super::TimerRequest;
    use susres::{RegManager, RegOrField, SuspendResume};
    use utralib::generated::*;

    pub struct XousTickTimer {
        csr: utralib::CSR<u32>,
        current_response: Option<TimerRequest>,
        connection: xous::CID,
        ticktimer_sr_manager: RegManager<{ utra::ticktimer::TICKTIMER_NUMREGS }>,
        wdt_sr_manager: RegManager<{ utra::wdt::WDT_NUMREGS }>,
        wdt: utralib::CSR<u32>,
    }

    fn handle_irq(_irq_no: usize, arg: *mut usize) {
        let xtt = unsafe { &mut *(arg as *mut XousTickTimer) };
        // println!("In IRQ, connection: {}", xtt.connection);

        // Safe because we're in an interrupt, and this interrupt is only
        // enabled when this value is not None.
        let response = xtt.current_response.take().unwrap();
        xous::return_scalar(response.sender, response.kind as usize).ok();

        // Disable the timer
        xtt.csr.wfo(utra::ticktimer::EV_ENABLE_ALARM, 0);
        xtt.csr.wfo(utra::ticktimer::EV_PENDING_ALARM, 1);

        // This is dangerous and may return an error if the queue is full.
        // Which is fine, because the queue is always recalculated any time a message arrives.
        use num_traits::ToPrimitive;
        xous::try_send_message(
            xtt.connection,
            xous::Message::Scalar(xous::ScalarMessage {
                id: crate::api::Opcode::RecalculateSleep.to_usize().unwrap(),
                arg1: response.sender.to_usize(),
                arg2: response.kind as usize,
                arg3: response.data,
                arg4: 0,
            }),
        )
        .ok();
    }

    impl XousTickTimer {
        pub fn new(connection: xous::CID) -> XousTickTimer {
            // println!("Connection: {}", connection);
            let csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::ticktimer::HW_TICKTIMER_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map Tick Timer CSR range");
            let wdt = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::wdt::HW_WDT_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map Watchdog timer CSR range");

            let ticktimer_sr_manager = RegManager::new(csr.as_mut_ptr() as *mut u32);
            let wdt_sr_manager = RegManager::new(wdt.as_mut_ptr() as *mut u32);

            let mut xtt = XousTickTimer {
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
                current_response: None,
                connection,
                ticktimer_sr_manager,
                wdt_sr_manager,
                wdt: CSR::new(wdt.as_mut_ptr() as *mut u32),
            };

            #[cfg(feature = "watchdog")]
            {
                xtt.wdt.wfo(utra::wdt::PERIOD_PERIOD, 0x7FFF_FFFF); // about 30 seconds +/- 50%
                xtt.wdt.wfo(utra::wdt::WATCHDOG_ENABLE, 1);
                // this is a write-once field that is lost later on, so it must be explicitly managed
                // xtt.wdt_sr_manager.push(RegOrField::Field(utra::wdt::WATCHDOG_ENABLE), None);
            }

            xous::claim_interrupt(
                utra::ticktimer::TICKTIMER_IRQ,
                handle_irq,
                (&mut xtt) as *mut XousTickTimer as *mut usize,
            )
            .expect("couldn't claim irq");

            xtt.ticktimer_sr_manager
                .push(RegOrField::Reg(utra::ticktimer::MSLEEP_TARGET0), None);
            xtt.ticktimer_sr_manager
                .push(RegOrField::Reg(utra::ticktimer::MSLEEP_TARGET1), None);
            xtt.ticktimer_sr_manager
                .push_fixed_value(RegOrField::Reg(utra::ticktimer::EV_PENDING), 0xFFFF_FFFF);
            xtt.ticktimer_sr_manager
                .push(RegOrField::Reg(utra::ticktimer::EV_ENABLE), None);

            xtt
        }

        pub fn reset(&mut self) {
            self.csr.wfo(utra::ticktimer::CONTROL_RESET, 0b1);
            self.csr.wo(utra::ticktimer::CONTROL, 0); // not paused, not reset -> free-run
        }

        pub fn raw_ticktime(&self) -> u64 {
            let mut time: u64 = self.csr.r(utra::ticktimer::TIME0) as u64;
            time |= (self.csr.r(utra::ticktimer::TIME1) as u64) << 32;

            time
        }

        pub fn elapsed_ms(&self) -> u64 {
            self.raw_ticktime() / TICKS_PER_MS
        }

        pub fn stop_interrupt(&mut self) -> Option<TimerRequest> {
            // Disable the timer
            self.csr.wfo(utra::ticktimer::EV_ENABLE_ALARM, 0);

            // Now that the interrupt is disabled, we can see if the interrupt handler has a current response.
            // If it exists, then that means that an interrupt did NOT fire, and an existing interrupt
            // is in place.
            if let Some(sr) = self.current_response.take() {
                #[cfg(feature = "debug-print")]
                {
                    log::info!(
                        "Stopping currently-running timer sr.msec: {}  elapsed_ms: {}",
                        sr.msec,
                        self.elapsed_ms()
                    );
                }
                Some(sr)
            } else {
                None
            }
        }

        pub fn schedule_response(&mut self, request: TimerRequest) {
            let irq_target = request.msec;
            log::trace!(
                "setting a response at {} ms (current time: {} ms)",
                irq_target,
                self.elapsed_ms()
            );

            // Disable the timer interrupt
            assert!(self.csr.rf(utra::ticktimer::EV_ENABLE_ALARM) == 0);
            self.csr.wfo(utra::ticktimer::EV_ENABLE_ALARM, 0);

            // Save a copy of the current sleep request
            self.current_response = Some(request);

            // Set the new target time
            self.csr
                .wo(utra::ticktimer::MSLEEP_TARGET1, (irq_target >> 32) as _);
            self.csr
                .wo(utra::ticktimer::MSLEEP_TARGET0, irq_target as _);

            // Clear previous interrupt (if any)
            self.csr.wfo(utra::ticktimer::EV_PENDING_ALARM, 1);

            // Enable the interrupt
            self.csr.wfo(utra::ticktimer::EV_ENABLE_ALARM, 1);
        }

        #[allow(dead_code)]
        pub fn reset_wdt(&mut self) {
            self.wdt.wfo(utra::wdt::WATCHDOG_RESET_WDT, 1);
        }

        #[allow(dead_code)]
        pub fn check_wdt(&mut self) {
            let state = self.wdt.r(utra::wdt::STATE);
            if state & self.wdt.ms(utra::wdt::STATE_DISARMED, 1) == 0 {
                log::info!(
                    "{} WDT is not disarmed, state: 0x{:x}",
                    self.elapsed_ms(),
                    state
                );
            }
        }

        // the ticktimer suspend/resume routines are a bit trickier than normal, so this isn't a great
        // example of a generic suspend/resume template
        pub fn suspend(&mut self) {
            log::trace!("suspending");
            self.ticktimer_sr_manager.suspend();
            self.wdt_sr_manager.suspend();

            // by writing this after suspend(), resume will get the prior value
            self.csr.wfo(utra::ticktimer::EV_ENABLE_ALARM, 0);
        }
        pub fn resume(&mut self) {
            // this is a write-once bit that's later erased, so it can't be managed automatically
            // thus we have to restore in manually on a resume
            #[cfg(feature = "watchdog")]
            {
                self.wdt.wfo(utra::wdt::WATCHDOG_ENABLE, 1);
            }

            // manually clear any pending ticktimer events. This is mainly releveant for a "touch-and-go" simulated suspend.
            self.csr.wfo(utra::ticktimer::EV_PENDING_ALARM, 1);

            self.wdt_sr_manager.resume();
            self.ticktimer_sr_manager.resume();

            log::trace!(
                "ticktimer enable: {}",
                self.csr.r(utra::ticktimer::EV_ENABLE)
            );
            log::trace!(
                "ticktimer time/target: {}/{}",
                self.csr.r(utra::ticktimer::TIME0),
                self.csr.r(utra::ticktimer::MSLEEP_TARGET0)
            );
        }
    }
}

#[cfg(any(
    not(target_os = "xous"),
    not(any(feature = "precursor", feature = "renode", not(target_os = "xous")))
))]
mod implementation {
    use crate::RequestKind;

    use super::TimerRequest;
    use num_traits::ToPrimitive;
    use std::convert::TryInto;

    #[derive(Debug)]
    enum SleepComms {
        InterruptSleep,
        StartSleep(
            xous::MessageSender,
            i64, /* ms */
            u64, /* elapsed */
        ),
    }
    pub struct XousTickTimer {
        start: std::time::Instant,
        sleep_comms: std::sync::mpsc::Sender<SleepComms>,
        time_remaining_receiver: std::sync::mpsc::Receiver<Option<TimerRequest>>,
    }

    impl XousTickTimer {
        pub fn new(cid: xous::CID) -> XousTickTimer {
            let (sleep_sender, sleep_receiver) = std::sync::mpsc::channel();
            let (time_remaining_sender, time_remaining_receiver) = std::sync::mpsc::channel();
            xous::create_thread(move || {
                let mut timeout = None;
                let mut current_response: Option<TimerRequest> = None;
                loop {
                    let result = match timeout {
                        None => sleep_receiver
                            .recv()
                            .map_err(|_| std::sync::mpsc::RecvTimeoutError::Disconnected),
                        Some(s) => sleep_receiver.recv_timeout(s),
                    };
                    match result {
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            let response = current_response.take().unwrap();
                            #[cfg(feature = "debug-print")]
                            log::info!("Returning scalar to {}", response.sender);
                            xous::return_scalar(response.sender, response.kind as usize)
                                .expect("couldn't send response");

                            // This is dangerous and may panic if the queue is full.
                            xous::try_send_message(
                                cid,
                                xous::Message::Scalar(xous::ScalarMessage {
                                    id: crate::api::Opcode::RecalculateSleep.to_usize().unwrap(),
                                    arg1: response.sender.to_usize(),
                                    arg2: response.kind as usize,
                                    arg3: response.data,
                                    arg4: 0,
                                }),
                            )
                            .unwrap();
                            timeout = None;
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                            return;
                        }
                        Ok(SleepComms::InterruptSleep) => {
                            timeout = None;
                            time_remaining_sender.send(current_response.take()).unwrap()
                        }
                        Ok(SleepComms::StartSleep(new_sender, expiry, elapsed)) => {
                            let mut duration = expiry - (elapsed as i64);
                            if duration > 0 {
                                #[cfg(feature = "debug-print")]
                                log::info!(
                                    "Starting sleep for {} ms, returning to {}",
                                    duration,
                                    new_sender
                                );
                            } else {
                                #[cfg(feature = "debug-print")]
                                log::info!(
                                    "Clamping duration to 0 (was: {})m returning to {}",
                                    duration,
                                    new_sender
                                );
                                duration = 0;
                            }
                            timeout = Some(std::time::Duration::from_millis(
                                duration.try_into().unwrap(),
                            ));
                            current_response = Some(TimerRequest {
                                sender: new_sender,
                                msec: expiry,
                                kind: RequestKind::Sleep,
                                data: 0,
                            });
                        }
                    }
                }
            })
            .unwrap();

            XousTickTimer {
                start: std::time::Instant::now(),
                time_remaining_receiver,
                sleep_comms: sleep_sender,
            }
        }

        pub fn reset(&mut self) {
            self.start = std::time::Instant::now();
        }

        pub fn elapsed_ms(&self) -> u64 {
            self.start.elapsed().as_millis().try_into().unwrap()
        }

        pub fn stop_interrupt(&mut self) -> Option<TimerRequest> {
            self.sleep_comms.send(SleepComms::InterruptSleep).unwrap();
            self.time_remaining_receiver.recv().unwrap()
        }

        pub fn schedule_response(&mut self, request: TimerRequest) {
            #[cfg(feature = "debug-print")]
            log::info!(
                "request.msec: {}  self.elapsed_ms: {}  returning to: {}",
                request.msec,
                self.elapsed_ms(),
                request.sender
            );
            self.sleep_comms
                .send(SleepComms::StartSleep(
                    request.sender,
                    request.msec as i64,
                    self.elapsed_ms(),
                ))
                .unwrap();
        }

        #[allow(dead_code)]
        pub fn reset_wdt(&self) {
            // dummy function, does nothing
        }
        pub fn register_suspend_listener(
            &self,
            _opcode: u32,
            _cid: xous::CID,
        ) -> Result<(), xous::Error> {
            Ok(())
        }
        pub fn suspend(&self) {}
        pub fn resume(&self) {}
    }
}

use implementation::*;
use susres::SuspendOrder;

/// Disable the sleep interrupt and remove the currently-pending sleep item.
/// If the sleep item has fired, then there will be no existing sleep item
/// remaining.
fn stop_sleep(
    ticktimer: &mut XousTickTimer,
    sleep_heap: &mut BTreeMap<TimeoutExpiry, TimerRequest>, // min-heap with Reverse
) {
    // If there's a sleep request ongoing now, grab it.
    if let Some(current) = ticktimer.stop_interrupt() {
        #[cfg(feature = "debug-print")]
        info!("Existing request was {:?}", current);
        sleep_heap.insert(current.msec, current);
    } else {
        #[cfg(feature = "debug-print")]
        info!("There was no existing sleep() request");
    }
}

fn start_sleep(
    ticktimer: &mut XousTickTimer,
    sleep_heap: &mut BTreeMap<TimeoutExpiry, TimerRequest>, // min-heap with Reverse
) {
    // If there are items in the sleep heap, take the next item that will expire.
    // TODO: Replace this with `.min()` when it's stabilized:
    // https://github.com/rust-lang/rust/issues/62924
    let next_timeout_msec = sleep_heap.iter().min().map(|(msec, _)| *msec);
    if let Some(msec) = next_timeout_msec {
        let next_response = sleep_heap.remove(&msec).unwrap();
        #[cfg(feature = "debug-print")]
        info!(
            "scheduling a response at {} to {} (heap: {:?})",
            next_response.msec, next_response.sender, sleep_heap
        );
        ticktimer.schedule_response(next_response);
    } else {
        #[cfg(feature = "debug-print")]
        info!(
            "not scheduling a response since the sleep heap is empty ({:?})",
            sleep_heap
        );
    }
}

/// Recalculate the sleep timer, optionally adding a new Request to the list of available
/// sleep events. This involves stopping the timer, recalculating the newest item, then
/// restarting the timer.
///
/// Note that interrupts are always enabled, which is why we must stop the timer prior to
/// reordering the list.
fn recalculate_sleep(
    ticktimer: &mut XousTickTimer,
    sleep_heap: &mut BTreeMap<TimeoutExpiry, TimerRequest>, // min-heap with Reverse
    new: Option<TimerRequest>,
) {
    stop_sleep(ticktimer, sleep_heap);

    // If we have a new sleep request, add it to the heap.
    if let Some(mut request) = new {
        #[cfg(feature = "debug-print")]
        info!("New sleep request was: {:?}", request);

        // Ensure that each timeout only exists once inside the tree
        request.msec += ticktimer.elapsed_ms() as i64;
        while sleep_heap.contains_key(&request.msec) {
            request.msec += 1;
        }

        #[cfg(feature = "debug-print")]
        info!("Modified, the request was: {:?}", request);
        sleep_heap.insert(request.msec, request);
    } else {
        #[cfg(feature = "debug-print")]
        info!("No new sleep request");
    }

    start_sleep(ticktimer, sleep_heap);
}

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
    ticktimer.reset(); // make sure the time starts from zero

    // register a suspend/resume listener
    let xns = xous_names::XousNames::new().unwrap();
    let sr_cid =
        xous::connect(ticktimer_server).expect("couldn't create suspend callback connection");
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
                    recalculate_sleep(
                        &mut ticktimer,
                        &mut sleep_heap,
                        Some(TimerRequest {
                            msec: ms,
                            sender: sender,
                            kind: RequestKind::Sleep,
                            data: 0,
                        }),
                    );
                }
            }
            api::Opcode::RecalculateSleep => {
                // let timeout_queue = timeout_heap.entry(msg.sender.pid()).or_default();
                if let Some(args) = msg.body.scalar_message() {
                    // If this is a Timeout message that fired, remove it from the Notification list
                    let sender = args.arg1;
                    let request_kind = args.arg2;
                    let condvar = args.arg3;
                    let sender_pid = xous::MessageSender::from_usize(sender).pid();

                    // If we're being asked to recalculate due to a timeout expiring, drop the sent
                    // message from the `entries` list.
                    // the first check confirms that the origin of the RecalculateSleep message is the Ticktimer,
                    // to prevent third-party servers from issuing the command and thus distorting the sleep
                    // calculations (since this is a public API, anything could happen).
                    if (msg.sender.pid().map(|p| p.get()).unwrap_or_default() as u32)
                        == xous::process::id()
                        && (request_kind == RequestKind::Timeout as usize)
                        && (sender > 0)
                    {
                        let entries = notify_hash
                            .entry(sender_pid)
                            .or_default()
                            .entry(condvar)
                            .or_default();
                        let mut idx = None;
                        for (i, val) in entries.iter().enumerate() {
                            if val.to_usize() == sender {
                                idx = Some(i);
                                break;
                            }
                        }
                        if let Some(idx) = idx {
                            entries.remove(idx);
                        }
                        // log::trace!("new entries for PID {:?}/condvar {:08x}: {:?}", sender_pid, condvar, notify_hash.get(&sender_pid).unwrap().get(&condvar));
                    }
                }
                recalculate_sleep(&mut ticktimer, &mut sleep_heap, None);
            }
            api::Opcode::SuspendResume => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                ticktimer.suspend();
                susres
                    .suspend_until_resume(token)
                    .expect("couldn't execute suspend/resume");
                ticktimer.resume();
            }),
            api::Opcode::PingWdt => {
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
                let pid = msg.sender.pid();
                if let Some(scalar) = msg.body.scalar_message_mut() {
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
                } else {
                    info!("sender made LockMutex request that was not blocking");
                }
            }
            api::Opcode::UnlockMutex => {
                let pid = msg.sender.pid();
                if msg.body.is_blocking() {
                    info!("sender made UnlockMutex request that was blocking");
                    continue;
                }
                if let Some(scalar) = msg.body.scalar_message() {
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
            }
            api::Opcode::WaitForCondition => {
                let pid = msg.sender.pid();
                if let Some(scalar) = msg.body.scalar_message_mut() {
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

                    // If there's a `timeout` argument, schedule a response.
                    if timeout != 0 {
                        recalculate_sleep(
                            &mut ticktimer,
                            &mut sleep_heap,
                            Some(TimerRequest {
                                msec: timeout as i64,
                                sender: msg.sender,
                                kind: RequestKind::Timeout,
                                data: condvar,
                            }),
                        )
                    }

                    // Add this to the list of entries waiting for a response.
                    notify_hash
                        .entry(pid)
                        .or_default()
                        .entry(condvar)
                        .or_default()
                        .push_back(msg.sender);

                    // log::trace!("New waiting senders: {:?}", notify_hash.get(&pid).unwrap().get(&condvar));

                    // The message will be responded to as part of the notification hash
                    // when the condvar is unlocked. Forget the contents of the message
                    // in order to prevent it from being responded to early.
                    core::mem::forget(msg_opt.take());
                } else {
                    info!(
                        "sender made WaitForCondition request that wasn't a BlockingScalar Message"
                    );
                }
            }
            api::Opcode::NotifyCondition => {
                let pid = msg.sender.pid();
                if msg.body.is_blocking() {
                    info!("sender made NotifyCondition request that was blocking");
                    continue;
                }
                if let Some(scalar) = msg.body.scalar_message() {
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

                    stop_sleep(&mut ticktimer, &mut sleep_heap);
                    for entry in awaiting.drain(..available_count) {
                        // Remove each entry in the timeout set
                        sleep_heap.retain(|_, v| if v.sender == entry { false } else { true });
                        xous::return_scalar(entry, 0).expect("couldn't send response");
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
                    start_sleep(&mut ticktimer, &mut sleep_heap);
                } else {
                    info!(
                        "sender made WaitForCondition request that wasn't a BlockingScalar Message"
                    );
                }
            }
            api::Opcode::InvalidCall => {
                error!("couldn't convert opcode");
            }
        }
    }
}
