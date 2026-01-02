//! BIO API
//!
//! For the first draft of this API, we're going to take an approach where all machines
//! and pins are explicitly managed. That is: we don't try and be clever and try to imply any
//! allocations. The developer has to correctly identify which engine to place their code on,
//! and which I/Os, if any, the thing uses.
//!
//! Here is the reference for what register correspond to what functions in BIO:
//!
//! FIFO - 8-deep fifo head/tail access. Cores halt on overflow/underflow.
//! - x16 r/w  fifo[0]
//! - x17 r/w  fifo[1]
//! - x18 r/w  fifo[2]
//! - x19 r/w  fifo[3]
//!
//! Quantum - core will halt until host-configured clock divider pules occurs,
//! or an external event comes in on a host-specified GPIO pin.
//! - x20 -/w  halt to quantum
//!
//! GPIO - note clear-on-0 semantics for bit-clear for data pins!
//!   This is done so we can do a shift-and-move without an invert to
//!   bitbang a data pin. Direction retains a more "conventional" meaning
//!   where a write of `1` to either clear or set will cause the action,
//!   as pin direction toggling is less likely to be in a tight inner loop.
//! - x21 r/w  write: (x26 & x21) -> gpio pins; read: gpio pins -> x21
//! - x22 -/w  (x26 & x22) -> `1` will set corresponding pin on gpio
//! - x23 -/w  (x26 & x23) -> `0` will clear corresponding pin on gpio
//! - x24 -/w  (x26 & x24) -> `1` will make corresponding gpio pin an output
//! - x25 -/w  (x26 & x25) -> `1` will make corresponding gpio pin an input
//! - x26 r/w  mask GPIO action outputs
//!
//! Events - operate on a shared event register. Bits [31:24] are hard-wired to FIFO
//! level flags, configured by the host; writes to bits [31:24] are ignored.
//! - x27 -/w  mask event sensitivity bits
//! - x28 -/w  `1` will set the corresponding event bit. Only [23:0] are wired up.
//! - x29 -/w  `1` will clear the corresponding event bit Only [23:0] are wired up.
//! - x30 r/-  halt until ((x27 & events) != 0), and return unmasked `events` value
//!
//! Core ID & debug:
//! - x31 r/-  [31:30] -> core ID; [29:0] -> cpu clocks since reset

// Other notes:
//
// clock divider equation:
// output frequency = fclk / (div_int + div_frac / 256)
//

use core::num::NonZeroU32;

use bitbybit::bitfield;
use num_traits::ToPrimitive;
use xous::MemoryRange;

pub const BIO_SERVER_NAME: &'static str = "_BIO server_";

/// A platform-neutral API for the BIO. Intended to be usable in both baremetal and `std` environments
pub trait BioApi<'a> {
    /// Initializes a core with the given `code`, loading into `offset`, with a given
    /// core configuration. This does not start the core running: that needs to be
    /// done with a separate `set_core_state` call.
    ///
    /// Returns a `u32` which is the frequency in Hz of the actual quantum interval that
    /// the core is running at. It's `None` if an external pin is configured as the quantum source.
    fn init_core(
        &mut self,
        core: BioCore,
        code: &[u8],
        offset: usize,
        config: CoreConfig,
    ) -> Result<Option<u32>, BioError>;

    /// Releases the core. As a side effect, the core is stopped.
    fn de_init_core(&mut self, core: BioCore) -> Result<(), BioError>;

    /// Updates the frequency of the cores, used to calculate dividers etc.
    /// Returns the previous frequency used by the system. Setting this
    /// allows the sleep/wake clock manager to do a best-effort rescaling of the
    /// clock dividers for each core as DVFS is applied for power savings.
    ///
    /// This API call does not actually *set* the frequency of the BIO complex -
    /// that is handled by the clock manager. This merely informs the BIO driver
    /// that the clocks may have changed.
    fn update_bio_freq(&mut self, freq: u32) -> u32;

    /// Returns the frequency, in Hz, of the incoming clock to the BIO cores.
    fn get_bio_freq(&self) -> u32;

    /// Returns the currently running frequency of a given BIO core. This API
    /// exists because `update_bio_freq()` call will result in the dividers being
    /// adjusted in an attempt to maintain the target quantum; however, there will
    /// often be some error between what was requested and the actual achievable
    /// frequency of the quantum interval.
    fn get_core_freq(&self, core: BioCore) -> Option<u32>;

    /// The index of `which` corresponds to each of the cores, 0-3.
    fn set_core_state(&mut self, which: [CoreRunSetting; 4]) -> Result<(), BioError>;

    /// Returns a `usize` which should be turned into a CSR as *mut u32 by the caller
    /// this can then be dereferenced using UTRA abstractions to access the following
    /// registers:
    ///   - SFR_FLEVEL
    ///   - SFR_TXF#
    ///   - SFR_RXF#
    ///   - SFR_EVENT_SET
    ///   - SFR_EVENT_CLR
    ///   - SFR_EVENT_STATUS
    ///
    /// The handle can only access one of the FIFOs, but it has access to all the other
    /// registers regardless of the FIFO.
    ///
    /// The handle has a `Drop` implementation that releases it when it goes out of scope.
    ///
    /// Safety: this has to be wrapped in an object that derives a CSR that also tracks
    /// the lifetime of this object, to prevent `Drop` from being called at the wrong time.
    ///
    /// Returns `None` if no more handles are available
    unsafe fn get_core_handle(&self, fifo: Fifo) -> Result<Option<CoreHandle>, BioError>;

    /// This call sets up the BIO's IRQ routing. It doesn't actually claim the IRQ
    /// or install the handler - that's up to the caller to do with Xous API calls.
    fn setup_irq_config(&mut self, config: IrqConfig) -> Result<(), BioError>;

    /// Allows BIO cores to DMA to/from the windows specified in `windows`. DMA filtering is
    /// on by default with no windows allowed.
    fn setup_dma_windows(&mut self, windows: DmaFilterWindows) -> Result<(), BioError>;

    /// Sets up the BIO I/O configuration
    fn setup_io_config(&mut self, config: IoConfig) -> Result<(), BioError>;

    /// Sets up a FIFO event trigger
    fn setup_fifo_event_triggers(&mut self, config: FifoEventConfig) -> Result<(), BioError>;

    /// Returns a version code for the underlying hardware.
    fn get_version(&self) -> u32;
}

#[macro_export]
/// This macro takes three identifiers and assembly code:
///   - name of the function to call to retrieve the assembled code
///   - a unique identifier that serves as label name for the start of the code
///   - a unique identifier that serves as label name for the end of the code
///   - a comma separated list of strings that form the assembly itself
///
///   *** The comma separated list must *not* end in a comma. ***
///
///   The macro is unable to derive names of functions or identifiers for labels
///   due to the partially hygienic macro rules of Rust, so you have to come
///   up with a list of unique names by yourself.
macro_rules! bio_code {
    ($fn_name:ident, $name_start:ident, $name_end:ident, $($item:expr),*) => {
        pub fn $fn_name() -> &'static [u8] {
            extern "C" {
                static $name_start: *const u8;
                static $name_end: *const u8;
            }
            // skip the first 4 bytes, as they contain the loading offset
            unsafe { core::slice::from_raw_parts($name_start.add(4), ($name_end as usize) - ($name_start as usize) - 4)}
        }

        core::arch::global_asm!(
            ".align 4",
            concat!(".globl ", stringify!($name_start)),
            concat!(stringify!($name_start), ":"),
            ".word .",
            $($item),*
            , ".align 4",
            concat!(".globl ", stringify!($name_end)),
            concat!(stringify!($name_end), ":"),
            ".word .",
        );
    };
}

#[derive(Debug, Copy, Clone, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum BioOp {
    InitCore,
    DeInitCore,
    UpdateBioFreq,
    GetCoreFreq,
    GetBioFreq,
    CoreState,
    GetCoreHandle,
    ReleaseCoreHandle,
    IrqConfig,
    DmaWindows,
    IoConfig,
    FifoEventTriggers,
    GetVersion,

    // Resource management opcodes
    ClaimResources,
    ReleaseResources,
    ResourceAvailability,
    CheckResources,
    CheckResourcesBatch,
    ClaimDynamicPin,
    ReleaseDynamicPin,

    InvalidCall,
}

#[derive(Debug, Copy, Clone)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
// align this so it can be passed as a memory message
#[repr(align(4096))]
pub struct CoreInitRkyv {
    pub core: BioCore,
    pub offset: usize,
    pub actual_freq: Option<u32>,
    pub config: CoreConfig,
    pub code: [u8; 4096],
    pub result: BioError,
}

#[repr(usize)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum BioCore {
    Core0 = 0,
    Core1 = 1,
    Core2 = 2,
    Core3 = 3,
}

impl From<usize> for BioCore {
    fn from(value: usize) -> Self {
        match value {
            0 => BioCore::Core0,
            1 => BioCore::Core1,
            2 => BioCore::Core2,
            3 => BioCore::Core3,
            _ => panic!("Invalid BioCore value: {}", value),
        }
    }
}

#[repr(usize)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum CoreRunSetting {
    Unchanged = 0,
    Start = 1,
    Stop = 2,
}

impl From<usize> for CoreRunSetting {
    fn from(value: usize) -> Self {
        match value {
            0 => CoreRunSetting::Unchanged,
            1 => CoreRunSetting::Start,
            2 => CoreRunSetting::Stop,
            _ => panic!("Invalid CoreRunSetting: {}", value),
        }
    }
}

#[derive(Debug, Copy, Clone)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum BioError {
    /// Uninitialized
    Uninit,
    /// No error
    None,
    /// specified core is not valid
    InvalidCore,
    /// program can't fit in memory, for one reason or another
    Oom,
    /// no more machines available
    NoFreeMachines,
    /// resource is already in use
    ResourceInUse,
    /// Loaded code did not match, first error at argument
    CodeCheck(usize),
    /// Catch-all for programming bugs that shouldn't happen
    InternalError,
}

impl From<xous::Error> for BioError {
    fn from(error: xous::Error) -> Self {
        match error {
            xous::Error::OutOfMemory => BioError::Oom,
            xous::Error::ServerNotFound => BioError::InvalidCore,
            xous::Error::ServerExists => BioError::NoFreeMachines,
            // Handle unmapped cases with a panic
            _ => panic!("Cannot convert Error::{:?} to BioError", error),
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct BioPin {
    pin_number: u8,
}
impl BioPin {
    pub fn pin_number(&self) -> u8 { self.pin_number }

    pub fn new(pin: u8) -> Self {
        if pin < 32 { Self { pin_number: pin } } else { panic!("Pin value out of range") }
    }
}

#[derive(Debug, Copy, Clone)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum ClockMode {
    /// Fixed divider - (int, frac)
    FixedDivider(u16, u8),
    /// Target frequency - fractional allowed. Attempts to adjust to target based on
    /// changing CPU clock. Fractional component means the "average" frequency is achieved
    /// by occasionally skipping clocks. This means there is jitter in the edge timing.
    TargetFreqFrac(u32),
    /// Target frequency - integer dividers only allowed. The absolute error of the
    /// frequency may be larger, but the jitter is smaller. Attempts to adjust to the
    /// target based on changing CPU clock.
    TargetFreqInt(u32),
    /// Use external pin as quantum source
    ExternalPin(BioPin),
}

#[derive(Debug, Copy, Clone)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct CoreConfig {
    pub clock_mode: ClockMode,
}

#[derive(Debug, Copy, Clone)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct IoConfig {
    /// bits set to `1` here are mapped to the BIO instead of the GPIO
    pub mapped: u32,
    /// bits set to `1` in the u32 corresponding to BIO inputs passed in as raw, unsynchronized
    /// values. This can lead to instability, but reduces latency.
    pub sync_bypass: u32,
    /// bits set to `1` in the u32 corresponding to BIO outputs that have the OE inverted
    pub oe_inv: u32,
    /// bits set to `1` in the u32 corresponding to BIO outputs that have the output value inverted
    /// compared to the value written into the register
    pub o_inv: u32,
    /// bits set to `1` in the u32 corresponding to BIO inputs that have the input value inverted
    /// before being passed into the register accessed by the core
    pub i_inv: u32,
    /// When specified all GPIO inputs are aligned to the divided clock of the specified core
    pub snap_inputs: Option<BioCore>,
    /// When specified all GPIO outputs are aligned to the divided clock of the specified core
    pub snap_outputs: Option<BioCore>,
}
impl Default for IoConfig {
    fn default() -> Self {
        Self {
            mapped: 0,
            sync_bypass: 0,
            oe_inv: 0,
            o_inv: 0,
            i_inv: 0,
            snap_inputs: None,
            snap_outputs: None,
        }
    }
}

#[bitfield(u8)]
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct TriggerSlot {
    #[bits(1..=7)]
    _reserved: arbitrary_int::u7,
    #[bit(0, rw)]
    trigger_slot: arbitrary_int::u1,
}

#[bitfield(u8)]
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct FifoLevel {
    #[bits(3..=7)]
    _reserved: arbitrary_int::u5,
    #[bits(0..=2, rw)]
    level: arbitrary_int::u3,
}

/// The event register is divided into 24 code-settable event bits + 8 FIFO event bits
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct FifoEventConfig {
    // for which FIFO we're configuring its event triggers
    pub which: Fifo,
    // there are up to two trigger slots per FIFO, specify 0 or 1 here.
    pub trigger_slot: TriggerSlot,
    // the level used for the triggers. Any number from 0-7.
    pub level: FifoLevel,
    // when set, the trigger condition happens compared to the level above
    pub trigger_less_than: bool,
    pub trigger_greater_than: bool,
    pub trigger_equal_to: bool,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[repr(usize)]
pub enum Fifo {
    Fifo0 = 0,
    Fifo1 = 1,
    Fifo2 = 2,
    Fifo3 = 3,
}

impl Fifo {
    pub fn to_usize_checked(self) -> usize {
        let discriminant = self as usize;
        assert!(discriminant <= 3, "Invalid discriminant");
        discriminant
    }
}

#[derive(Debug, Copy, Clone)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[repr(usize)]
pub enum Irq {
    Irq0 = 0,
    Irq1 = 1,
    Irq2 = 2,
    Irq3 = 3,
}

impl Irq {
    pub fn to_usize_checked(self) -> usize {
        let discriminant = self as usize;
        assert!(discriminant <= 3, "Invalid discriminant");
        discriminant
    }
}

#[bitfield(u32)]
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct IrqMask {
    #[bit(31, rw)]
    fifo3_trigger1: bool,
    #[bit(30, rw)]
    fifo3_trigger0: bool,
    #[bit(29, rw)]
    fifo2_trigger1: bool,
    #[bit(28, rw)]
    fifo2_trigger0: bool,
    #[bit(27, rw)]
    fifo1_trigger1: bool,
    #[bit(26, rw)]
    fifo1_trigger0: bool,
    #[bit(25, rw)]
    fifo0_trigger1: bool,
    #[bit(24, rw)]
    fifo0_trigger0: bool,
    #[bits(0..=23, rw)]
    software: arbitrary_int::u24,
}

#[derive(Debug, Copy, Clone)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct IrqConfig {
    /// Specifies which of the four BIO IRQ lines are being configured
    pub which: Irq,
    pub edge_triggered: bool,
    /// Bits set to 1 here will cause an interrupt to be passed up when they are set in the
    /// aggregated BIO event state.
    pub mask: IrqMask,
}

/// Defines an accessible window by DMA from BIO cores. It's a slice starting from `base` going for `bounds`
/// bytes.
#[derive(Debug, Copy, Clone)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct DmaWindow {
    pub base: u32,
    pub bounds: NonZeroU32,
}

/// Structure that defines all of the windows allowed by the system
#[derive(Debug, Copy, Clone)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct DmaFilterWindows {
    pub windows: [Option<DmaWindow>; 4],
}

pub struct CoreHandle {
    conn: xous::CID,
    handle: usize,
    fifo: arbitrary_int::u2,
}

impl CoreHandle {
    pub fn new(conn: xous::CID, handle: usize, fifo: arbitrary_int::u2) -> Self {
        Self { conn, handle, fifo }
    }

    /// safety: this needs to be wrapped in a hardware-level CSR object that tracks the lifetime of
    /// the underlying pointer handle.
    pub unsafe fn handle(&self) -> usize { self.handle }
}

impl Drop for CoreHandle {
    fn drop(&mut self) {
        // safety: handle was allocated by the OS and is thus safe to re-create as a range
        // the length of the range (one page) is set by the hardware implementation and never changes
        xous::unmap_memory(unsafe { MemoryRange::new(self.handle, 4096).unwrap() }).unwrap();
        xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                BioOp::ReleaseCoreHandle.to_usize().unwrap(),
                self.fifo.value() as usize,
                0,
                0,
                0,
            ),
        )
        .unwrap();
    }
}
