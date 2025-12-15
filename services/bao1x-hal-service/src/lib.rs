pub mod api;
pub mod trng;

use core::sync::atomic::{AtomicU32, Ordering};

use bao1x_api::*;
use num_traits::*;
static REFCOUNT: AtomicU32 = AtomicU32::new(0);

pub struct UdmaGlobal {
    conn: xous::CID,
}

impl UdmaGlobal {
    pub fn new() -> Self {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let xns = xous_names::XousNames::new().unwrap();
        let conn =
            xns.request_connection(SERVER_NAME_BAO1X_HAL).expect("Couldn't connect to bao1x HAL server");
        UdmaGlobal { conn }
    }

    pub fn udma_clock_config(&self, peripheral: PeriphId, enable: bool) {
        xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                HalOpcode::ConfigureUdmaClock.to_usize().unwrap(),
                peripheral as u32 as usize,
                if enable { 1 } else { 0 },
                0,
                0,
            ),
        )
        .expect("Couldn't setup UDMA clock");
    }

    /// Safety: this event does no checking if an event has been previously mapped. It is up
    /// to the caller to ensure that no events are being stomped on.
    pub unsafe fn udma_event_map(
        &self,
        peripheral: PeriphId,
        event_type: PeriphEventType,
        to_channel: EventChannel,
    ) {
        let et_u32: u32 = event_type.into();
        xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                HalOpcode::ConfigureUdmaEvent.to_usize().unwrap(),
                peripheral as u32 as usize,
                et_u32 as usize,
                to_channel as u32 as usize,
                0,
            ),
        )
        .expect("Couldn't setup UDMA event mapping");
    }

    pub fn reset(&self, peripheral: PeriphId) {
        xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                HalOpcode::PeriphReset.to_usize().unwrap(),
                peripheral as u32 as usize,
                0,
                0,
                0,
            ),
        )
        .expect("Couldn't setup UDMA clock");
    }
}

pub struct Hal {
    conn: xous::CID,
}
impl Hal {
    pub fn new() -> Self {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let xns = xous_names::XousNames::new().unwrap();
        let conn =
            xns.request_connection(SERVER_NAME_BAO1X_HAL).expect("Couldn't connect to bao1x HAL server");
        Hal { conn }
    }

    pub fn set_preemption(&self, on: bool) {
        xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                HalOpcode::SetPreemptionState.to_usize().unwrap(),
                if on { 1 } else { 0 },
                0,
                0,
                0,
            ),
        )
        .expect("Couldn't setup preemption state");
    }
}

impl UdmaGlobalConfig for UdmaGlobal {
    fn clock(&self, peripheral: PeriphId, enable: bool) { self.udma_clock_config(peripheral, enable); }

    unsafe fn udma_event_map(
        &self,
        peripheral: PeriphId,
        event_type: PeriphEventType,
        to_channel: EventChannel,
    ) {
        self.udma_event_map(peripheral, event_type, to_channel);
    }

    fn reset(&self, peripheral: PeriphId) { self.reset(peripheral); }

    fn irq_status_bits(&self, bank: IrqBank) -> u32 {
        match xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                HalOpcode::UdmaIrqStatusBits.to_usize().unwrap(),
                bank as u32 as usize,
                0,
                0,
                0,
            ),
        ) {
            Ok(xous::Result::Scalar5(_, value, _, _, _)) => value as u32,
            _ => panic!("Unhandled response on irq_status_bits"),
        }
    }
}
