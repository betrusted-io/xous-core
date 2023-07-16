use crate::RequestKind;
use crate::TimerRequest;
use crate::TimeoutExpiry;

use num_traits::ToPrimitive;
use std::convert::TryInto;
use std::collections::BTreeMap;

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

    pub fn last_response(&self) -> &Option<TimerRequest> {
        &None
    }

    pub fn clear_last_response(&mut self) {}
    pub fn reset(&mut self) {
        self.start = std::time::Instant::now();
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis().try_into().unwrap()
    }

    pub fn stop_interrupt(&mut self) -> Option<TimerRequest> {
        self.sleep_comms.send(SleepComms::InterruptSleep).unwrap();
        self.time_remaining_receiver.recv().ok().flatten()
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
    pub fn init(&mut self) {}
    pub fn suspend(&self) {}
    pub fn resume(&self) {}

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
            log::info!("Existing request was {:?}", current);
            sleep_heap.insert(current.msec, current);
        } else {
            #[cfg(feature = "debug-print")]
            log::info!("There was no existing sleep() request");
        }
    }

    pub fn start_sleep(
        &mut self,
        sleep_heap: &mut BTreeMap<TimeoutExpiry, TimerRequest>, // min-heap with Reverse
    ) {
        // If there are items in the sleep heap, take the next item that will expire.
        if let Some((_msec, next_response)) = sleep_heap.pop_first() {
            #[cfg(feature = "debug-print")]
            log::info!(
                "scheduling a response at {} to {} (heap: {:?})",
                next_response.msec, next_response.sender, sleep_heap
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
        self.stop_sleep(sleep_heap);
        log::trace!("Elapsed: {}", self.elapsed_ms());
        self.clear_last_response();

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

        self.start_sleep(sleep_heap);
    }
}
