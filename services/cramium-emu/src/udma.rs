use cramium_api::*;

pub struct UdmaGlobal {}

impl UdmaGlobal {
    pub fn new() -> Self { Self {} }

    pub fn udma_clock_config(&self, _peripheral: PeriphId, _enable: bool) {}

    /// Safety: this event does no checking if an event has been previously mapped. It is up
    /// to the caller to ensure that no events are being stomped on.
    pub unsafe fn udma_event_map(
        &self,
        _peripheral: PeriphId,
        _event_type: PeriphEventType,
        _to_channel: EventChannel,
    ) {
    }

    pub fn reset(&self, _peripheral: PeriphId) {}
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
}
