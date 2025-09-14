/// The Opcode numbers here should not be changed. You can add new ones,
/// but do not re-use old numbers or repurpose them. This is because the
/// numbers are hard-coded in other libraries in order to break circular
/// dependencies on this file.
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
#[repr(usize)]
pub enum HalOpcode {
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
    /// Configure BIO pins
    ConfigureBio = 17,

    /// Configure UDMA clocks & events
    // blocking scalar
    ConfigureUdmaClock = 7,
    // blocking scalar
    ConfigureUdmaEvent = 8,
    // blocking scalar
    UdmaIrqStatusBits = 16,

    /// I2C operations
    I2c = 9,

    /// Peripheral reset
    PeriphReset = 10,

    /// Configure Iox IRQ
    ConfigureIoxIrq = 11,
    IrqLocalHandler = 12,

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
