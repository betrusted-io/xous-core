pub mod api;
pub mod iox_lib;
pub mod keyboard;
pub mod trng;

use api::Opcode;
use cramium_hal::udma::{EventChannel, PeriphEventType, PeriphId};
pub use iox_lib::*;
use num_traits::*;

/// Do not change this constant, it is hard-coded into libraries in order to break
/// circular dependencies on the IFRAM block.
pub const SERVER_NAME_CRAM_HAL: &str = "_Cramium-SoC HAL_";

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);

pub struct UdmaGlobal {
    conn: xous::CID,
}

impl UdmaGlobal {
    pub fn new() -> Self {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let xns = xous_names::XousNames::new().unwrap();
        let conn =
            xns.request_connection(SERVER_NAME_CRAM_HAL).expect("Couldn't connect to Cramium HAL server");
        UdmaGlobal { conn }
    }

    pub fn udma_clock_config(&self, peripheral: PeriphId, enable: bool) {
        xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                Opcode::ConfigureUdmaClock.to_usize().unwrap(),
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
                Opcode::ConfigureUdmaEvent.to_usize().unwrap(),
                peripheral as u32 as usize,
                et_u32 as usize,
                to_channel as u32 as usize,
                0,
            ),
        )
        .expect("Couldn't setup UDMA event mapping");
    }
}
