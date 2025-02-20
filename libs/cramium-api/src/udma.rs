/// Constrain potential types for UDMA words to only what is representable and valid
/// for the UDMA subsystem.
pub trait UdmaWidths {}
impl UdmaWidths for i8 {}
impl UdmaWidths for u8 {}
impl UdmaWidths for i16 {}
impl UdmaWidths for u16 {}
impl UdmaWidths for i32 {}
impl UdmaWidths for u32 {}

#[derive(Debug, Clone, Copy)]
pub enum I2cChannel {
    Channel0,
    Channel1,
    Channel2,
    Channel3,
}

#[derive(Debug, Clone, Copy)]
pub enum SpimChannel {
    Channel0,
    Channel1,
    Channel2,
    Channel3,
}

#[repr(u32)]
#[derive(Copy, Clone, num_derive::FromPrimitive)]
pub enum PeriphId {
    Uart0 = 1 << 0,
    Uart1 = 1 << 1,
    Uart2 = 1 << 2,
    Uart3 = 1 << 3,
    Spim0 = 1 << 4,
    Spim1 = 1 << 5,
    Spim2 = 1 << 6,
    Spim3 = 1 << 7,
    I2c0 = 1 << 8,
    I2c1 = 1 << 9,
    I2c2 = 1 << 10,
    I2c3 = 1 << 11,
    Sdio = 1 << 12,
    I2s = 1 << 13,
    Cam = 1 << 14,
    Filter = 1 << 15,
    Scif = 1 << 16,
    Spis0 = 1 << 17,
    Spis1 = 1 << 18,
    Adc = 1 << 19,
}
impl Into<u32> for PeriphId {
    fn into(self) -> u32 { self as u32 }
}

impl From<SpimChannel> for PeriphId {
    fn from(value: SpimChannel) -> Self {
        match value {
            SpimChannel::Channel0 => PeriphId::Spim0,
            SpimChannel::Channel1 => PeriphId::Spim1,
            SpimChannel::Channel2 => PeriphId::Spim2,
            SpimChannel::Channel3 => PeriphId::Spim3,
        }
    }
}

impl From<I2cChannel> for PeriphId {
    fn from(value: I2cChannel) -> Self {
        match value {
            I2cChannel::Channel0 => PeriphId::I2c0,
            I2cChannel::Channel1 => PeriphId::I2c1,
            I2cChannel::Channel2 => PeriphId::I2c2,
            I2cChannel::Channel3 => PeriphId::I2c3,
        }
    }
}

#[repr(u32)]
#[derive(Copy, Clone)]
pub enum PeriphEventId {
    Uart0 = 0,
    Uart1 = 4,
    Uart2 = 8,
    Uart3 = 12,
    Spim0 = 16,
    Spim1 = 20,
    Spim2 = 24,
    Spim3 = 28,
    I2c0 = 32,
    I2c1 = 36,
    I2c2 = 40,
    I2c3 = 44,
    Sdio = 48,
    I2s = 52,
    Cam = 56,
    Adc = 57, // note exception to ordering here
    Filter = 60,
    Scif = 64,
    Spis0 = 68,
    Spis1 = 72,
}
impl From<PeriphId> for PeriphEventId {
    fn from(id: PeriphId) -> Self {
        match id {
            PeriphId::Uart0 => PeriphEventId::Uart0,
            PeriphId::Uart1 => PeriphEventId::Uart1,
            PeriphId::Uart2 => PeriphEventId::Uart2,
            PeriphId::Uart3 => PeriphEventId::Uart3,
            PeriphId::Spim0 => PeriphEventId::Spim0,
            PeriphId::Spim1 => PeriphEventId::Spim1,
            PeriphId::Spim2 => PeriphEventId::Spim2,
            PeriphId::Spim3 => PeriphEventId::Spim3,
            PeriphId::I2c0 => PeriphEventId::I2c0,
            PeriphId::I2c1 => PeriphEventId::I2c1,
            PeriphId::I2c2 => PeriphEventId::I2c2,
            PeriphId::I2c3 => PeriphEventId::I2c3,
            PeriphId::Sdio => PeriphEventId::Sdio,
            PeriphId::I2s => PeriphEventId::I2s,
            PeriphId::Cam => PeriphEventId::Cam,
            PeriphId::Filter => PeriphEventId::Filter,
            PeriphId::Scif => PeriphEventId::Scif,
            PeriphId::Spis0 => PeriphEventId::Spis0,
            PeriphId::Spis1 => PeriphEventId::Spis1,
            PeriphId::Adc => PeriphEventId::Adc,
        }
    }
}
#[repr(u32)]
#[derive(Copy, Clone)]
pub enum EventUartOffset {
    Rx = 0,
    Tx = 1,
    RxChar = 2,
    Err = 3,
}
#[repr(u32)]
#[derive(Copy, Clone)]
pub enum EventSpimOffset {
    Rx = 0,
    Tx = 1,
    Cmd = 2,
    Eot = 3,
}
#[repr(u32)]
#[derive(Copy, Clone)]
pub enum EventI2cOffset {
    Rx = 0,
    Tx = 1,
    Cmd = 2,
    Eot = 3,
}
#[repr(u32)]
#[derive(Copy, Clone)]
pub enum EventSdioOffset {
    Rx = 0,
    Tx = 1,
    Eot = 2,
    Err = 3,
}
#[repr(u32)]
#[derive(Copy, Clone)]
pub enum EventI2sOffset {
    Rx = 0,
    Tx = 1,
}
#[repr(u32)]
#[derive(Copy, Clone)]
pub enum EventCamOffset {
    Rx = 0,
}
#[repr(u32)]
#[derive(Copy, Clone)]
pub enum EventAdcOffset {
    Rx = 0,
}
#[repr(u32)]
#[derive(Copy, Clone)]
pub enum EventFilterOffset {
    Eot = 0,
    Active = 1,
}
#[repr(u32)]
#[derive(Copy, Clone)]

pub enum EventScifOffset {
    Rx = 0,
    Tx = 1,
    RxChar = 2,
    Err = 3,
}
#[repr(u32)]
#[derive(Copy, Clone)]
pub enum EventSpisOffset {
    Rx = 0,
    Tx = 1,
    Eot = 2,
}
#[derive(Copy, Clone)]
pub enum PeriphEventType {
    Uart(EventUartOffset),
    Spim(EventSpimOffset),
    I2c(EventI2cOffset),
    Sdio(EventSdioOffset),
    I2s(EventI2sOffset),
    Cam(EventCamOffset),
    Adc(EventAdcOffset),
    Filter(EventFilterOffset),
    Scif(EventScifOffset),
    Spis(EventSpisOffset),
}
impl Into<u32> for PeriphEventType {
    fn into(self) -> u32 {
        match self {
            PeriphEventType::Uart(t) => t as u32,
            PeriphEventType::Spim(t) => t as u32,
            PeriphEventType::I2c(t) => t as u32,
            PeriphEventType::Sdio(t) => t as u32,
            PeriphEventType::I2s(t) => t as u32,
            PeriphEventType::Cam(t) => t as u32,
            PeriphEventType::Adc(t) => t as u32,
            PeriphEventType::Filter(t) => t as u32,
            PeriphEventType::Scif(t) => t as u32,
            PeriphEventType::Spis(t) => t as u32,
        }
    }
}

#[repr(u32)]
#[derive(Debug, Copy, Clone, num_derive::FromPrimitive)]
pub enum EventChannel {
    Channel0 = 0,
    Channel1 = 8,
    Channel2 = 16,
    Channel3 = 24,
}

/// Use a trait that will allow us to share code between both `std` and `no-std` implementations
pub trait UdmaGlobalConfig {
    fn clock(&self, peripheral: PeriphId, enable: bool);
    unsafe fn udma_event_map(
        &self,
        peripheral: PeriphId,
        event_type: PeriphEventType,
        to_channel: EventChannel,
    );
    fn reset(&self, peripheral: PeriphId);
}
