#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::Opcode;

mod os_timer;

use core::convert::TryFrom;

use heapless::binary_heap::{BinaryHeap, Min};
use heapless::consts::*;

use log::{error, info};

#[derive(Eq, Debug)]
pub struct SleepResponse {
    msec: usize,
    sender: xous::MessageSender,
}

impl core::cmp::Ord for SleepResponse {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.msec.cmp(&other.msec)
    }
}

impl core::cmp::PartialOrd for SleepResponse {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl core::cmp::PartialEq for SleepResponse {
    fn eq(&self, other: &Self) -> bool {
        self.msec == other.msec && self.sender == other.sender
    }
}

#[cfg(target_os = "none")]
mod implementation {
    const TICKS_PER_MS: u64 = 1;
    use super::SleepResponse;
    use utralib::generated::*;

    pub struct XousTickTimer {
        csr: utralib::CSR<u32>,
        wdt: utralib::CSR<u32>,
        current_response: Option<SleepResponse>,
        response_start: u64,
        connection: xous::CID,
    }

    fn handle_irq(_irq_no: usize, arg: *mut usize) {
        let xtt = unsafe { &mut *(arg as *mut XousTickTimer) };
        // println!("In IRQ, connection: {}", xtt.connection);

        // Safe because we're in an interrupt, and this interrupt is only
        // enabled when this value is not None.
        let response = xtt.current_response.take().unwrap();
        xous::return_scalar(response.sender, 0).expect("couldn't send response");

        xtt.csr.wo(utra::ticktimer::EV_ENABLE, 0); // Disable the interrupt

        // This is dangerous and may panic if the queue is full.
        xous::try_send_message(xtt.connection, crate::api::Opcode::RecalculateSleep.into())
            .map(|_| ())
            .unwrap();
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

            let mut xtt = XousTickTimer {
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
                wdt: CSR::new(wdt.as_mut_ptr() as *mut u32),
                current_response: None,
                response_start: 0,
                connection,
            };

            xtt.wdt.wfo(utra::wdt::WATCHDOG_ENABLE, 1);

            xous::claim_interrupt(
                utra::ticktimer::TICKTIMER_IRQ,
                handle_irq,
                (&mut xtt) as *mut XousTickTimer as *mut usize,
            )
            .expect("couldn't claim irq");

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

        pub fn stop_interrupt(&mut self) -> Option<SleepResponse> {
            self.csr.wfo(utra::ticktimer::EV_ENABLE_ALARM, 0); // Disable the timer
            let current_value = self.elapsed_ms();
            if let Some(sr) = self.current_response.take() {
                Some(SleepResponse {
                    msec: (current_value - self.response_start) as _,
                    sender: sr.sender,
                })
            } else {
                None
            }
        }

        pub fn schedule_response(&mut self, milliseconds: usize, sender: xous::MessageSender) {
            self.current_response = Some(SleepResponse {
                sender,
                msec: milliseconds,
            });
            self.response_start = self.elapsed_ms();
            let irq_target = self.response_start + (milliseconds as u64);
            log::info!(
                "setting a response at {} ms (current time: {} ms)",
                irq_target,
                self.response_start
            );
            self.csr.wfo(utra::ticktimer::EV_PENDING_ALARM, 1); // Clear previous interrupt (if any)
            self.csr
                .wo(utra::ticktimer::MSLEEP_TARGET1, (irq_target >> 32) as _);
            self.csr
                .wo(utra::ticktimer::MSLEEP_TARGET0, irq_target as _);
            self.csr.wfo(utra::ticktimer::EV_ENABLE_ALARM, 1); // Enable the interrupt
        }

        pub fn reset_wdt(&mut self) {
            // disarm the WDT

            // why do we have this weird interlock dance?
            //  - the WDT is triggered on a "ring oscillator" that's entirely internal to the SoC
            //    (so you can't defeat the WDT by just pausing the external clock sourc)
            //  - the ring oscillator has a tolerance band of 65MHz +/- 50%
            //  - the CPU runs at 100MHz with a tight tolerance
            //  - thus we have to confirm the write of the watchdog data before moving to the next state
            if self.wdt.rf(utra::wdt::STATE_ENABLED) == 1 {
                if self.wdt.rf(utra::wdt::STATE_DISARMED) != 1 {
                    while self.wdt.rf(utra::wdt::STATE_ARMED1) == 1 {
                        self.wdt.wfo(utra::wdt::WATCHDOG_RESET_CODE, 0x600d);
                    }
                    self.wdt.wfo(utra::wdt::WATCHDOG_RESET_CODE, 0xc0de);
                    while self.wdt.rf(utra::wdt::STATE_ARMED2) == 1 {
                        self.wdt.wfo(utra::wdt::WATCHDOG_RESET_CODE, 0xc0de);
                    }
                }
            }
        }
    }
}

#[cfg(not(target_os = "none"))]
mod implementation {
    use super::SleepResponse;
    use std::convert::TryInto;

    #[derive(Debug)]
    enum SleepComms {
        InterruptSleep,
        StartSleep(xous::MessageSender, u64 /* ms */),
    }
    pub struct XousTickTimer {
        start: std::time::Instant,
        sleep_comms: std::sync::mpsc::Sender<SleepComms>,
        time_remaining_receiver: std::sync::mpsc::Receiver<Option<SleepResponse>>,
    }

    impl XousTickTimer {
        pub fn new(cid: xous::CID) -> XousTickTimer {
            let (sleep_sender, sleep_receiver) = std::sync::mpsc::channel();
            let (time_remaining_sender, time_remaining_receiver) = std::sync::mpsc::channel();
            xous::create_thread(move || {
                let mut timeout = None;
                let mut sender = Default::default();
                loop {
                    let start_time = std::time::Instant::now();
                    let result = match timeout {
                        None => sleep_receiver
                            .recv()
                            .map_err(|_| std::sync::mpsc::RecvTimeoutError::Disconnected),
                        Some(s) => sleep_receiver.recv_timeout(s),
                    };
                    match result {
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            xous::return_scalar(sender, 0).expect("couldn't send response");

                            // This is dangerous and may panic if the queue is full.
                            xous::try_send_message(
                                cid,
                                crate::api::Opcode::RecalculateSleep.into(),
                            )
                            .unwrap();
                            timeout = None;
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                            return;
                        }
                        Ok(SleepComms::InterruptSleep) => time_remaining_sender
                            .send(if timeout.is_some() {
                                Some(SleepResponse {
                                    sender,
                                    msec: start_time.elapsed().as_millis() as _,
                                })
                            } else {
                                None
                            })
                            .unwrap(),
                        Ok(SleepComms::StartSleep(new_sender, duration)) => {
                            timeout = Some(std::time::Duration::from_millis(duration));
                            sender = new_sender;
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

        pub fn stop_interrupt(&mut self) -> Option<SleepResponse> {
            self.sleep_comms.send(SleepComms::InterruptSleep).unwrap();
            self.time_remaining_receiver.recv().unwrap()
        }

        pub fn schedule_response(&mut self, milliseconds: usize, sender: xous::MessageSender) {
            self.sleep_comms
                .send(SleepComms::StartSleep(sender, milliseconds as _))
                .unwrap();
        }

        pub fn reset_wdt(&self) {
            // dummy function, does nothing
        }
    }
}

use implementation::*;

fn recalculate_sleep(
    ticktimer: &mut XousTickTimer,
    sleep_heap: &mut BinaryHeap<SleepResponse, U32, Min>,
    new: Option<SleepResponse>,
) {
    if let Some(current) = ticktimer.stop_interrupt() {
        sleep_heap.push(current).expect("couldn't push to heap")
    }

    if let Some(response) = new {
        sleep_heap
            .push(response)
            .expect("couldn't push new sleep to heap");
    }
    if let Some(next_response) = sleep_heap.pop() {
        info!("scheduling a response at {}", next_response.msec);
        ticktimer.schedule_response(next_response.msec, next_response.sender);
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    // Start the OS timer which is responsible for setting up preemption.
    os_timer::init();

    log_server::init_wait().unwrap();

    // "Sleep" commands get put in here and are ordered as necessary
    let mut sleep_heap: BinaryHeap<SleepResponse, U32, Min> = BinaryHeap::new();

    let ticktimer_server =
        xous::create_server_with_address(b"ticktimer-server").expect("Couldn't create Ticktimer server");

    // Connect to our own server so we can send the "Recalculate" message
    let ticktimer_client = xous::connect(xous::SID::from_bytes(b"ticktimer-server").unwrap())
        .expect("couldn't connect to self");

    // Create a new ticktimer object
    let mut ticktimer = XousTickTimer::new(ticktimer_client);

    loop {
        ticktimer.reset_wdt();

        //info!("TickTimer: waiting for message");
        let envelope = xous::receive_message(ticktimer_server).unwrap();
        //info!("TickTimer: Message: {:?}", envelope);
        if let Ok(opcode) = Opcode::try_from(&envelope.body) {
            //info!("TickTimer: Opcode: {:?}", opcode);
            match opcode {
                Opcode::Reset => {
                    info!("TickTimer: reset called");
                    ticktimer.reset();
                }
                Opcode::ElapsedMs => {
                    let time = ticktimer.elapsed_ms();
                    //info!("TickTimer: returning time of {:?}", time);
                    xous::return_scalar2(
                        envelope.sender,
                        (time & 0xFFFF_FFFFu64) as usize,
                        ((time >> 32) & 0xFFF_FFFFu64) as usize,
                    )
                    .expect("TickTimer: couldn't return time request");
                    //info!("TickTimer: done returning value");
                }
                Opcode::SleepMs(ms) => recalculate_sleep(
                    &mut ticktimer,
                    &mut sleep_heap,
                    Some(SleepResponse {
                        msec: ms,
                        sender: envelope.sender,
                    }),
                ),
                Opcode::RecalculateSleep => {
                    recalculate_sleep(&mut ticktimer, &mut sleep_heap, None);
                    //info!("TickTimer: Done recalculating");
                }
            }
        } else {
            error!("couldn't convert opcode");
        }
    }
}
