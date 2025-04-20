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
