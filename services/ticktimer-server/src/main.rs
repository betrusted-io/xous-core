#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;

use heapless::binary_heap::{BinaryHeap, Min};

use log::{error, info};

#[derive(Eq)]
pub struct SleepRequest {
    msec: i64,
    sender: xous::MessageSender,
}

impl core::fmt::Display for SleepRequest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "SleepRequest {{ msec: {}, {} }}", self.msec, self.sender)
    }
}

impl core::fmt::Debug for SleepRequest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "SleepRequest {{ msec: {}, {} }}", self.msec, self.sender)
    }
}

impl core::cmp::Ord for SleepRequest {
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

impl core::cmp::PartialOrd for SleepRequest {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl core::cmp::PartialEq for SleepRequest {
    fn eq(&self, other: &Self) -> bool {
        self.msec == other.msec && self.sender == other.sender
    }
}

#[cfg(target_os = "none")]
mod implementation {
    const TICKS_PER_MS: u64 = 1;
    use super::SleepRequest;
    use utralib::generated::*;
    use susres::{RegManager, RegOrField, SuspendResume};

    pub struct XousTickTimer {
        csr: utralib::CSR<u32>,
        current_response: Option<SleepRequest>,
        connection: xous::CID,
        ticktimer_sr_manager: RegManager::<{utra::ticktimer::TICKTIMER_NUMREGS}>,
        wdt_sr_manager: RegManager::<{utra::wdt::WDT_NUMREGS}>,
        wdt: utralib::CSR<u32>,
    }

    fn handle_wdt(_irq_no: usize, arg: *mut usize) {
        let xtt = unsafe { &mut *(arg as *mut XousTickTimer) };
        // disarm the WDT -- do it in an interrupt context, to make sure we aren't interrupted while doing this.

        // This WDT is a query/response type. There are two possible states, and to unlock it,
        // the CPU must query the state and provide the right response. Why this is the case:
        //  - the WDT is triggered on a "ring oscillator" that's entirely internal to the SoC
        //    (so you can't defeat the WDT by just pausing the external clock sourc)
        //  - the ring oscillator has a tolerance band of 65MHz +/- 50%
        //  - the CPU runs at 100MHz with a tight tolerance
        //  - thus it is impossible to guarantee sync between the domains, so we do a two-step query/response interlock
        #[cfg(feature = "watchdog")]
        if xtt.wdt.rf(utra::wdt::STATE_ENABLED) == 1 {
            if xtt.wdt.rf(utra::wdt::STATE_ARMED1) != 0 {
                xtt.wdt.wfo(utra::wdt::WATCHDOG_RESET_CODE, 0x600d);
            }
            if xtt.wdt.rf(utra::wdt::STATE_ARMED2) != 0 {
                xtt.wdt.wfo(utra::wdt::WATCHDOG_RESET_CODE, 0xc0de);
            }
        }
        // Clear the interrupt
        xtt.wdt.wfo(utra::wdt::EV_PENDING_SOFT_INT, 1);
    }

    fn handle_irq(_irq_no: usize, arg: *mut usize) {
        let xtt = unsafe { &mut *(arg as *mut XousTickTimer) };
        // println!("In IRQ, connection: {}", xtt.connection);

        // Safe because we're in an interrupt, and this interrupt is only
        // enabled when this value is not None.
        let response = xtt.current_response.take().unwrap();

        xous::return_scalar(response.sender, 0).expect("couldn't send response");

        // Disable the timer
        xtt.csr.wfo(utra::ticktimer::EV_ENABLE_ALARM, 0);
        xtt.csr.wfo(utra::ticktimer::EV_PENDING_ALARM, 1);

        // This is dangerous and may return an error if the queue is full.
        // Which is fine, because the queue is always recalculated any time a message arrives.
        use num_traits::ToPrimitive;
        xous::try_send_message(xtt.connection,
            xous::Message::Scalar(xous::ScalarMessage {
                id: crate::api::Opcode::RecalculateSleep.to_usize().unwrap(),
                arg1: 0, arg2: 0, arg3: 0, arg4: 0
            })
        ).ok();
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

            xous::claim_interrupt(
                utra::wdt::WDT_IRQ,
                handle_wdt,
                (&mut xtt) as *mut XousTickTimer as *mut usize,
            )
            .expect("couldn't claim irq");

            #[cfg(feature = "watchdog")]
            {
                xtt.wdt.wfo(utra::wdt::EV_ENABLE_SOFT_INT, 1);
                xtt.wdt_sr_manager.push(RegOrField::Reg(utra::wdt::EV_ENABLE), None);
            }
            #[cfg(not(feature = "watchdog"))]
            {
                xtt.wdt.wfo(utra::wdt::EV_ENABLE_SOFT_INT, 0);
                xtt.wdt_sr_manager.push(RegOrField::Reg(utra::wdt::EV_ENABLE), None);
            }

            xtt.ticktimer_sr_manager.push(RegOrField::Reg(utra::ticktimer::MSLEEP_TARGET0), None);
            xtt.ticktimer_sr_manager.push(RegOrField::Reg(utra::ticktimer::MSLEEP_TARGET1), None);
            xtt.ticktimer_sr_manager.push_fixed_value(RegOrField::Reg(utra::ticktimer::EV_PENDING), 0xFFFF_FFFF);
            xtt.ticktimer_sr_manager.push(RegOrField::Reg(utra::ticktimer::EV_ENABLE), None);

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

        pub fn stop_interrupt(&mut self) -> Option<SleepRequest> {
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

        pub fn schedule_response(&mut self, request: SleepRequest) {
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
            // this triggers an interrupt, and the handler of the interrupt does the actual reset
            // this is done because we don't want the WDT reset to be interrupted
            self.wdt.wfo(utra::wdt::INTERRUPT_INTERRUPT, 1);
        }

        #[allow(dead_code)]
        pub fn check_wdt(&mut self) {
            let state = self.wdt.r(utra::wdt::STATE);
            if state & self.wdt.ms(utra::wdt::STATE_DISARMED, 1) == 0 {
                log::info!("{} WDT is not disarmed, state: 0x{:x}", self.elapsed_ms(), state);
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
                self.wdt.wfo(utra::wdt::EV_PENDING_SOFT_INT, 1);
                self.wdt.wfo(utra::wdt::EV_ENABLE_SOFT_INT, 1);
                self.wdt.wfo(utra::wdt::WATCHDOG_ENABLE, 1);
            }

            // manually clear any pending ticktimer events. This is mainly releveant for a "touch-and-go" simulated suspend.
            self.csr.wfo(utra::ticktimer::EV_PENDING_ALARM, 1);

            self.wdt_sr_manager.resume();
            self.ticktimer_sr_manager.resume();

            #[cfg(feature = "watchdog")]
            self.wdt.wfo(utra::wdt::INTERRUPT_INTERRUPT, 1);

            log::trace!("ticktimer enable: {}", self.csr.r(utra::ticktimer::EV_ENABLE));
            log::trace!("ticktimer time/target: {}/{}", self.csr.r(utra::ticktimer::TIME0), self.csr.r(utra::ticktimer::MSLEEP_TARGET0));
        }
    }
}

#[cfg(not(target_os = "none"))]
mod implementation {
    use super::SleepRequest;
    use std::convert::TryInto;
    use num_traits::ToPrimitive;

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
        time_remaining_receiver: std::sync::mpsc::Receiver<Option<SleepRequest>>,
    }

    impl XousTickTimer {
        pub fn new(cid: xous::CID) -> XousTickTimer {
            let (sleep_sender, sleep_receiver) = std::sync::mpsc::channel();
            let (time_remaining_sender, time_remaining_receiver) = std::sync::mpsc::channel();
            xous::create_thread(move || {
                let mut timeout = None;
                let mut current_response: Option<SleepRequest> = None;
                loop {
                    let result = match timeout {
                        None => sleep_receiver
                            .recv()
                            .map_err(|_| std::sync::mpsc::RecvTimeoutError::Disconnected),
                        Some(s) => sleep_receiver.recv_timeout(s),
                    };
                    match result {
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            let sender = current_response.take().unwrap().sender;
                            #[cfg(feature = "debug-print")]
                            log::info!("Returning scalar to {}", sender);
                            xous::return_scalar(sender, 0).expect("couldn't send response");

                            // This is dangerous and may panic if the queue is full.
                            xous::try_send_message(
                                cid,
                                xous::Message::Scalar(xous::ScalarMessage {
                                    id: crate::api::Opcode::RecalculateSleep.to_usize().unwrap(),
                                    arg1: 0, arg2: 0, arg3: 0, arg4: 0
                                })
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
                            current_response = Some(SleepRequest {
                                sender: new_sender,
                                msec: expiry,
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

        pub fn stop_interrupt(&mut self) -> Option<SleepRequest> {
            self.sleep_comms.send(SleepComms::InterruptSleep).unwrap();
            self.time_remaining_receiver.recv().unwrap()
        }

        pub fn schedule_response(&mut self, request: SleepRequest) {
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
        pub fn register_suspend_listener(&self, _opcode: u32, _cid: xous::CID) -> Result<(), xous::Error> {
            Ok(())
        }
        pub fn suspend(&self) {
        }
        pub fn resume(&self) {
        }

    }
}

use implementation::*;

fn recalculate_sleep(
    ticktimer: &mut XousTickTimer,
    sleep_heap: &mut BinaryHeap<SleepRequest, Min, 64>,
    new: Option<SleepRequest>,
) {
    // If there's a sleep request ongoing now, grab it.
    if let Some(current) = ticktimer.stop_interrupt() {
        #[cfg(feature = "debug-print")]
        info!("Existing request was {:?}", current);
        sleep_heap.push(current).expect("couldn't push to heap")
    } else {
        #[cfg(feature = "debug-print")]
        info!("There was no existing request");
    }

    // If we have a new sleep request, add it to the heap.
    if let Some(mut request) = new {
        #[cfg(feature = "debug-print")]
        info!("New sleep request was: {:?}", request);

        request.msec += ticktimer.elapsed_ms() as i64;

        #[cfg(feature = "debug-print")]
        info!("Modified, the request was: {:?}", request);
        sleep_heap
            .push(request)
            .expect("couldn't push new sleep to heap");
    } else {
        #[cfg(feature = "debug-print")]
        info!("No new sleep request");
    }

    // If there are items in the sleep heap, take the next item that will expire.
    if let Some(next_response) = sleep_heap.pop() {
        #[cfg(feature = "debug-print")]
        info!(
            "scheduling a response at {} to {} (heap: {:?})",
            next_response.msec, next_response.sender, sleep_heap
        );
        ticktimer.schedule_response(next_response);
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    let mut sleep_heap: BinaryHeap<SleepRequest, Min, 64> = BinaryHeap::new();

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

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
    let sr_cid = xous::connect(ticktimer_server).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(&xns, api::Opcode::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");

    loop {
        #[cfg(feature = "watchdog")]
        ticktimer.reset_wdt();
        //#[cfg(feature = "watchdog")] // for debugging the watchdog
        //ticktimer.check_wdt();

        let msg = xous::receive_message(ticktimer_server).unwrap();
        log::trace!("msg: {:?}", msg);
        match num_traits::FromPrimitive::from_usize(msg.body.id()) {
            Some(api::Opcode::ElapsedMs) => {
                let time = ticktimer.elapsed_ms() as i64;
                xous::return_scalar2(
                    msg.sender,
                    (time & 0xFFFF_FFFFi64) as usize,
                    ((time >> 32) & 0xFFF_FFFFi64) as usize,
                )
                .expect("couldn't return time request");
            }
            Some(api::Opcode::SleepMs) => xous::msg_blocking_scalar_unpack!(msg, ms, _, _, _, {
                    recalculate_sleep(
                        &mut ticktimer,
                        &mut sleep_heap,
                        Some(SleepRequest {
                            msec: ms as i64,
                            sender: msg.sender,
                        }),
                    )
            }),
            Some(api::Opcode::RecalculateSleep) => {
                recalculate_sleep(&mut ticktimer, &mut sleep_heap, None);
            },
            Some(api::Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                ticktimer.suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                ticktimer.resume();
            }),
            None => {
                error!("couldn't convert opcode");
                break
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xous::destroy_server(ticktimer_server).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0);
}
