use std::collections::BTreeMap;
use atsama5d27::tc::Tc;
#[cfg(feature = "debug-print")]
use log::{info, trace};
use log::error;
use utralib::*;
use xous::arch::irq::IrqNumber;

use crate::platform::{TimeoutExpiry, TimerRequest};

const MASTER_CLOCK_SPEED: u32 = 164000000 / 2;
const TICKS_PER_MS: u32 = MASTER_CLOCK_SPEED / 128 / 1000;

pub struct XousTickTimer {
    timer: Tc,
    /// Since SAMA5D2's timer is 32-bit, we're using a counter as a reference
    /// and only run the timer for the duration of the next delay
    running_counter: u64,
    current_response: Option<TimerRequest>,
    last_response: Option<TimerRequest>,
    connection: xous::CID,
}

/// *NOTE*: Avoid panics and prints inside an IRQ handler as they may result in forbidden syscalls
fn handle_irq(_irq_no: usize, arg: *mut usize) {
    let xtt = unsafe { &mut *(arg as *mut XousTickTimer) };
    xtt.timer.status(); // Acknowledge the interrupt by reading status register
    xtt.timer.stop();

    // Safe because we're in an interrupt, and this interrupt is only
    // enabled when this value is not None.
    let response = xtt.current_response.take();
    if let Some(response) = response {
        xous::return_scalar(response.sender, response.kind as usize).ok();

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

        // Save the response so we can be sure we don't double-return messages.
        xtt.last_response = Some(response);
    } else {
        unsafe { core::arch::asm!("bkpt") };
    }
}

impl XousTickTimer {
    pub fn new(connection: xous::CID) -> XousTickTimer {
        let csr = xous::syscall::map_memory(
            xous::MemoryAddress::new(HW_TC0_BASE),
            None,
            0x1000, // 16K
            xous::MemoryFlags::R | xous::MemoryFlags::W | xous::MemoryFlags::DEV,
        )
            .expect("couldn't map TC0 CSR range");

        let mut timer = Tc::with_alt_base_addr(csr.as_ptr() as u32);
        timer.init();

        let mut xtt = XousTickTimer {
            timer,
            running_counter: 0,
            current_response: None,
            last_response: None,
            connection,
        };

        xous::claim_interrupt(
            IrqNumber::Tc0 as usize,
            handle_irq,
            (&mut xtt) as *mut XousTickTimer as *mut usize,
        )
            .expect("couldn't claim irq");

        xtt
    }

    pub fn reset(&mut self) {
        // no-op, the TC0 timer resets itself every time it's started
    }

    pub fn last_response(&self) -> &Option<TimerRequest> {
        &self.last_response
    }

    pub fn clear_last_response(&mut self) {
        self.last_response = None;
    }
    pub fn raw_ticktime(&self) -> u32 {
        let ticktime = self.timer.counter();
        #[cfg(feature = "debug-print")]
        trace!("Raw ticktime: {:08x}", ticktime);
        ticktime
    }

    pub fn elapsed_ms(&self) -> u32 {
        let elapsed_ms = self.raw_ticktime() / TICKS_PER_MS;
        #[cfg(feature = "debug-print")]
        trace!("Elapsed ms: {}", elapsed_ms);
        elapsed_ms
    }

    pub fn stop_interrupt(&mut self) -> Option<TimerRequest> {
        // Disable the timer and interrupt
        self.timer.stop();
        self.timer.set_interrupt(false);

        // Now that the interrupt is disabled, we can see if the interrupt handler has a current response.
        // If it exists, then that means that an interrupt did NOT fire, and an existing interrupt
        // is in place.
        if let Some(sr) = self.current_response.take() {
            #[cfg(feature = "debug-print")]
            {
                info!(
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
        let irq_target = request.msec.saturating_sub(self.running_counter as i64);

        if irq_target > u32::MAX as i64 {
            error!(
                "Invalid sleep target: {} can't be more than {} ms",
                irq_target,
                u32::MAX
            );
            return;
        }

        #[cfg(feature = "debug-print")]
        trace!(
            "setting a response at {} ms (current time: {} ms, counter: {})",
            irq_target,
            self.elapsed_ms(),
            self.running_counter
        );

        self.timer.set_period(irq_target as u32 * TICKS_PER_MS);

        // Save a copy of the current sleep request
        self.current_response = Some(request);

        // Reset and enable interrupt
        self.timer.set_interrupt(true);
        self.timer.start();
    }

    // the ticktimer suspend/resume routines are a bit trickier than normal, so this isn't a great
    // example of a generic suspend/resume template
    pub fn suspend(&mut self) {
        #[cfg(feature = "debug-print")]
        trace!("suspending");
    }

    pub fn resume(&mut self) {
        #[cfg(feature = "debug-print")]
        trace!("resuming");
    }

    #[allow(dead_code)]
    pub fn reset_wdt(&mut self) {
        // TODO: reset watchdog timer
    }

    /// Disable the sleep interrupt and remove the currently-pending sleep item.
    /// If the sleep item has fired, then there will be no existing sleep item
    /// remaining.
    pub fn stop_sleep(
        &mut self,
        sleep_heap: &mut BTreeMap<TimeoutExpiry, TimerRequest>, // min-heap with Reverse
    ) {
        // If there's a sleep request ongoing now, grab it.
        if let Some(current) = self.stop_interrupt() {
            #[cfg(feature = "debug-print")]
            info!("Existing request was {:?}", current);
            sleep_heap.insert(current.msec, current);
        } else {
            #[cfg(feature = "debug-print")]
            info!("There was no existing sleep() request");
        }
    }

    pub fn start_sleep(
        &mut self,
        sleep_heap: &mut BTreeMap<TimeoutExpiry, TimerRequest>, // min-heap with Reverse
    ) {
        // If there are items in the sleep heap, take the next item that will expire.
        if let Some((_msec, next_response)) = sleep_heap.pop_first() {
            #[cfg(feature = "debug-print")]
            info!(
                "scheduling a response at {} to {} (heap: {:?})",
                next_response.msec, next_response.sender, sleep_heap
            );

            self.schedule_response(next_response);
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
    pub fn recalculate_sleep(
        &mut self,
        sleep_heap: &mut BTreeMap<TimeoutExpiry, TimerRequest>, // min-heap with Reverse
        new: Option<TimerRequest>,
    ) {
        let elapsed = self.elapsed_ms();
        self.stop_sleep(sleep_heap);
        log::trace!("Elapsed: {}", elapsed);
        self.clear_last_response();

        log::trace!("Increasing running counter {} by {}: {}", self.running_counter, elapsed, self.running_counter + elapsed as u64);
        self.running_counter += elapsed as u64;

        // If we have a new sleep request, add it to the heap.
        if let Some(mut request) = new {
            #[cfg(feature = "debug-print")]
            info!("New sleep request was: {:?}", request);

            // Ensure that each timeout only exists once inside the tree
            request.msec += self.running_counter as i64;
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

        log::trace!("Sleep heap: {:?}", sleep_heap);

        self.start_sleep(sleep_heap);
    }
}
