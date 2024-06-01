pub mod keyboard;
use cramium_hal::iox;
pub use keyboard::*;

/// The Opcode numbers here should not be changed. You can add new ones,
/// but do not re-use old numbers or repurpose them. This is because the
/// numbers are hard-coded in other libraries in order to break circular
/// dependencies on this file.
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
#[repr(usize)]
pub enum Opcode {
    /// Allocate an IFRAM block
    MapIfram = 0,
    /// Deallocate an IFRAM block
    UnmapIfram = 1,

    /// Manage PIO behaviors. PIO requires real-time behaviors, so
    /// the idea is that this service encodes a set of "behaviors" that
    /// are API extensions which are modular. The behaviors are set up as
    /// feature flags. Fast bulk data transfer to/from the behaviors is
    /// done using IFRAM blocks, which are managed by the above API calls;
    /// otherwise "singleton" peeks and pokes to the PIO should be handled
    /// with specific ScalarMessage calls to minimize OS overhead in
    /// context-switching to the block.
    #[cfg(feature = "pio")]
    ConfigurePioBehavior = 2,

    /// Gutter for Invalid Calls
    InvalidCall = 3,

    /// Configure Iox (memory mutable lend)
    ConfigureIox = 4,
    /// Set the whole bank with a value/bitmask pair (blocking scalar)
    SetGpioBank = 5,
    /// Return the value of a GPIO bank (blocking scalar)
    GetGpioBank = 6,

    /// Configure UDMA clocks & events
    // blocking scalar
    ConfigureUdmaClock = 7,
    // blocking scalar
    ConfigureUdmaEvent = 8,

    /// Exit server
    Quit = 255,

    /// Behavior opcode base
    #[cfg(feature = "pio")]
    BehaviorBase0 = 0x1000,
    #[cfg(feature = "pio")]
    BehaviorBase1 = 0x2000,
    #[cfg(feature = "pio")]
    BehaviorBase2 = 0x3000,
    #[cfg(feature = "pio")]
    BehaviorBase3 = 0x4000,
}

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[cfg(feature = "pio")]
pub enum PioBehavior {
    /// PIO pins for sclk, si, scs + IFRAM physical address
    #[cfg(feature = "pio-memlcd")]
    MemoryLcd(u8, u8, u8, usize),
}

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[cfg(feature = "pio")]
pub enum BehaviorBase {
    Base0,
    Base1,
    Base2,
    Base3,
}

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[cfg(feature = "pio")]
pub enum PioResult {
    Ok(BehaviorBase),
    NoSmAvailable,
    PinConflict,
}

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[cfg(feature = "pio-memlcd")]
#[repr(usize)]
pub enum MemoryLcdOpcode {
    /// Blit a frame, don't block. If a callback is set, messages the callback when done.
    BlitFrameNonBlocking = 0,
    /// Blit a frame, block until blit is done
    BiltFrameBlocking = 1,
    /// Poll if blitting is in progress
    IsBusy = 2,
    /// Sets a callback address as SID + opcode for pingbacks when the blit is done.
    SetCallbackServer = 3,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct IoxConfigMessage {
    pub port: iox::IoxPort,
    pub pin: u8,
    pub direction: Option<iox::IoxDir>,
    pub function: Option<iox::IoxFunction>,
    pub schmitt_trigger: Option<iox::IoxEnable>,
    pub pullup: Option<iox::IoxEnable>,
    pub slow_slew: Option<iox::IoxEnable>,
    pub strength: Option<iox::IoxDriveStrength>,
}
