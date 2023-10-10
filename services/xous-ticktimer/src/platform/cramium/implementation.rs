const TICKS_PER_MS: u64 = 1;

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::collections::BTreeMap;

use crate::TimeoutExpiry;
use crate::TimerRequest;

#[cfg(feature="susres")]
use susres::{RegManager, RegOrField, SuspendResume};
use utralib::generated::*;

use xous::definitions::MessageSender;

/// Latency slack may be necessary for hardware implementations that can't handle
/// events that happened in the past. However, for Precursor hardware, alarms will
/// trigger based on a "less than or equal to current time" basis, so even if an
/// alarm happens in the "past" due to slippage between the current time reading
/// and the alarm setting, the alarm should still trigger.
///
/// Note that setting this number larger than 0 will degrade scheduler performance.
const LATENCY_SLACK_MS: i64 = 0;

pub struct XousTickTimer {
    csr: utralib::CSR<u32>,
    current_response: Option<TimerRequest>,
    #[cfg(feature="susres")]
    ticktimer_sr_manager: RegManager<{ utra::ticktimer::TICKTIMER_NUMREGS }>,
    #[cfg(feature="watchdog")]
    wdt_sr_manager: RegManager<{ utra::wdt::WDT_NUMREGS }>,
    #[cfg(feature="watchdog")]
    wdt: utralib::CSR<u32>,
}

/// Connection ID to the ticktimer server, for use in interrupt contexts.
static TICKTIMER_CONNECTION: AtomicU32 = AtomicU32::new(0);

/// The Message ID of the last message we responded to
static LAST_RESPONDER: AtomicUsize = AtomicUsize::new(0);

/// This is incremented every time the ticktimer IRQ fires. It's used to
/// determine how many events were missed during processing of new
/// ticktimer events.
static TICKTIMER_SEQUENCE_NUMBER: AtomicUsize = AtomicUsize::new(0);

fn handle_irq(_irq_no: usize, arg: *mut usize) {
    let xtt: &mut XousTickTimer = unsafe { &mut *(arg as *mut XousTickTimer) };
    let connection = TICKTIMER_CONNECTION.load(Ordering::Relaxed);
    // println!("In IRQ, connection: {}", xtt.connection);

    // Safe because we're in an interrupt, and this interrupt is only
    // enabled when this value is not None. Furthermore, the value is
    // only ever updated when interrupts are disabled.
    let response = xtt.current_response.take().unwrap();
    xous::return_scalar(response.sender, response.kind as usize).ok();

    // Disable the timer
    xtt.csr.wfo(utra::ticktimer::EV_ENABLE_ALARM, 0);
    xtt.csr.wfo(utra::ticktimer::EV_PENDING_ALARM, 1);

    // This is dangerous and may return an error if the queue is full.
    // Which is fine, because the queue is always recalculated any time a message arrives.
    use num_traits::ToPrimitive;
    xous::try_send_message(
        connection,
        xous::Message::Scalar(xous::ScalarMessage {
            id: crate::api::Opcode::RecalculateSleep.to_usize().unwrap(),
            arg1: response.sender.to_usize(),
            arg2: response.kind as usize,
            arg3: response.data,
            arg4: 0,
        }),
    )
    .ok();

    // Remember what the last message was that we responded to. This will prevent
    // double-responding to messages.
    LAST_RESPONDER.store(response.sender.to_usize(), Ordering::Relaxed);

    // Note that we've handled another IRQ event.
    TICKTIMER_SEQUENCE_NUMBER.fetch_add(1, Ordering::Relaxed);
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
        #[cfg(feature="watchdog")]
        let wdt = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::wdt::HW_WDT_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map Watchdog timer CSR range");
        #[cfg(feature="susres")]
        let ticktimer_sr_manager = RegManager::new(csr.as_mut_ptr() as *mut u32);
        #[cfg(feature="watchdog")]
        let wdt_sr_manager = RegManager::new(wdt.as_mut_ptr() as *mut u32);

        #[allow(unused_mut)] // sometimes mut, sometimes not based on feature flags.
        let mut xtt = XousTickTimer {
            csr: CSR::new(csr.as_mut_ptr() as *mut u32),
            current_response: None,
            #[cfg(feature="susres")]
            ticktimer_sr_manager,
            #[cfg(feature="watchdog")]
            wdt_sr_manager,
            #[cfg(feature="watchdog")]
            wdt: CSR::new(wdt.as_mut_ptr() as *mut u32),
        };

        #[cfg(feature = "watchdog")]
        {
            xtt.wdt.wfo(utra::wdt::PERIOD_PERIOD, 0x7FFF_FFFF); // about 30 seconds +/- 50%
            xtt.wdt.wfo(utra::wdt::WATCHDOG_ENABLE, 1);
            // this is a write-once field that is lost later on, so it must be explicitly managed
            // xtt.wdt_sr_manager.push(RegOrField::Field(utra::wdt::WATCHDOG_ENABLE), None);
        }

        // Keep a copy of the connection around so we can use it in the interrupt.
        TICKTIMER_CONNECTION.store(connection.into(), Ordering::Relaxed);

        xtt
    }

    pub fn last_response(&self) -> MessageSender {
        MessageSender::from_usize(LAST_RESPONDER.load(Ordering::Relaxed))
    }

    /// Return the number of times the ticktimer IRQ has fired.
    ///
    /// # Safety
    ///
    /// This function is safe to call only if interrupts are disabled. Otherwise,
    /// the value may change during processing.
    pub unsafe fn sequence_number() -> usize {
        TICKTIMER_SEQUENCE_NUMBER.load(Ordering::Relaxed)
    }

    pub fn init(&mut self) {
        xous::claim_interrupt(
            utra::ticktimer::TICKTIMER_IRQ,
            handle_irq,
            self as *mut XousTickTimer as *mut usize,
        )
        .expect("couldn't claim irq");

        #[cfg(feature="susres")]
        {
            self.ticktimer_sr_manager
                .push(RegOrField::Reg(utra::ticktimer::MSLEEP_TARGET0), None);
            self.ticktimer_sr_manager
                .push(RegOrField::Reg(utra::ticktimer::MSLEEP_TARGET1), None);
            self.ticktimer_sr_manager
                .push_fixed_value(RegOrField::Reg(utra::ticktimer::EV_PENDING), 0xFFFF_FFFF);
            self.ticktimer_sr_manager
                .push(RegOrField::Reg(utra::ticktimer::EV_ENABLE), None);
        }
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
        let irq_target = request.msec.to_i64().wrapping_add(LATENCY_SLACK_MS);
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
    #[cfg(feature="watchdog")]
    pub fn reset_wdt(&mut self) {
        self.wdt.wfo(utra::wdt::WATCHDOG_RESET_WDT, 1);
    }

    #[allow(dead_code)]
    #[cfg(feature="watchdog")]
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
        #[cfg(feature="susres")]
        self.ticktimer_sr_manager.suspend();
        #[cfg(feature="watchdog")]
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

         #[cfg(feature="watchdog")]
        self.wdt_sr_manager.resume();
        #[cfg(feature="susres")]
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

    /// Disable the sleep interrupt and remove the currently-pending sleep item.
    /// If the sleep item has fired, then there will be no existing sleep item
    /// remaining.
    ///
    /// # Returns
    ///
    /// The serial number of the ticktimer handler at the point where
    /// interrupts were disabled.
    pub(crate) fn stop_sleep(
        &mut self,
        sleep_heap: &mut BTreeMap<TimeoutExpiry, TimerRequest>, // min-heap with Reverse
    ) -> usize {
        // If there's a sleep request ongoing now, grab it.
        if let Some(current) = self.stop_interrupt() {
            #[cfg(feature = "debug-print")]
            log::info!("Existing request was {:?}", current);
            sleep_heap.insert(current.msec, current);
        } else {
            #[cfg(feature = "debug-print")]
            log::info!("There was no existing sleep() request");
        }
        TICKTIMER_SEQUENCE_NUMBER.load(Ordering::Relaxed)
    }

    pub(crate) fn start_sleep(
        &mut self,
        sleep_heap: &mut BTreeMap<TimeoutExpiry, TimerRequest>, // min-heap with Reverse
    ) {
        // If there are items in the sleep heap, take the next item that will expire.
        if let Some((_msec, next_response)) = sleep_heap.pop_first() {
            #[cfg(feature = "debug-print")]
            log::info!(
                "scheduling a response at {} to {} (heap: {:?})",
                next_response.msec,
                next_response.sender,
                sleep_heap
            );

            self.schedule_response(next_response);
        } else {
            #[cfg(feature = "debug-print")]
            log::info!(
                "not scheduling a response since the sleep heap is empty ({:?})",
                sleep_heap
            );
        }
    }

    /// Recalculate sleep without starting and stopping the sleep timer.
    ///
    /// # Safety
    ///
    /// This must be called with the sleep timer already stopped
    pub(crate) unsafe fn recalculate_sleep_offline(
        &mut self,
        sleep_heap: &mut BTreeMap<TimeoutExpiry, TimerRequest>,
        new: Option<TimerRequest>,
    ) {
        log::trace!("Elapsed: {}", self.elapsed_ms());

        // If we have a new sleep request, add it to the heap.
        if let Some(mut request) = new {
            #[cfg(feature = "debug-print")]
            log::info!("New sleep request was: {:?}", request);

            // Ensure that each timeout only exists once inside the tree
            request.msec += self.elapsed_ms() as i64;
            while sleep_heap.contains_key(&request.msec) {
                request.msec += 1;
            }

            #[cfg(feature = "debug-print")]
            log::info!("Modified, the request was: {:?}", request);
            sleep_heap.insert(request.msec, request);
        } else {
            #[cfg(feature = "debug-print")]
            log::info!("No new sleep request");
        }
    }

    /// Recalculate the sleep timer, optionally adding a new Request to the list of available
    /// sleep events. This involves stopping the timer, recalculating the newest item, then
    /// restarting the timer.
    ///
    /// Note that interrupts are always enabled, which is why we must stop the timer prior to
    /// reordering the list.
    pub(crate) fn recalculate_sleep(
        &mut self,
        sleep_heap: &mut BTreeMap<TimeoutExpiry, TimerRequest>,
        new: Option<TimerRequest>,
    ) {
        self.stop_sleep(sleep_heap);
        unsafe { self.recalculate_sleep_offline(sleep_heap, new) }
        self.start_sleep(sleep_heap);
    }
}
