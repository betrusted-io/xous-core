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

// clock divider equation:
// output frequency = fclk / (div_int + div_frac / 256)

// WS2812B timings - base cycle time is 210ns
// 0: Hi 2 cycles, low 4 cycles
// 1: Hi 4 cycles, low 2 cycles
// WS2812C timings - base cycle time is 312.5ns -0/+50ns
// 0: Hi 1 cycle, low 3 cycle
// 1: Hi 2 cycle, low 2 cycle

use core::num::NonZeroU32;
use std::marker::PhantomData;

use bitbybit::bitfield;
use num_traits::ToPrimitive;

pub const BIO_SERVER_NAME: &'static str = "_BIO server_";

#[derive(Debug, Copy, Clone, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum BioOp {
    InitCore,
    DeInitCore,
    SetCoreFreq,
    GetCoreFreq,
    CoreState,
    GetCoreHandle,
    ReleaseCoreHandle,
    IrqConfig,
    DmaWindows,
    IoConfig,
    FifoEventTriggers,
    GetVersion,
}

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
// align this so it can be passed as a memory message
#[repr(align(4096))]
pub struct CoreInitRkyv {
    pub core: BioCore,
    pub offset: usize,
    pub config: CoreConfigRkyv,
    pub code: [u8; 4096],
}

#[repr(usize)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum BioCore {
    Core0 = 0,
    Core1 = 1,
    Core2 = 2,
    Core3 = 3,
}

#[repr(usize)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum CoreRunSetting {
    Unchanged = 0,
    Start = 1,
    Stop = 2,
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

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum BioError {
    /// specified state machine is not valid
    InvalidSm,
    /// program can't fit in memory, for one reason or another
    Oom,
    /// no more machines available
    NoFreeMachines,
    /// Loaded code did not match, first error at argument
    CodeCheck(usize),
}

impl From<xous::Error> for BioError {
    fn from(error: xous::Error) -> Self {
        match error {
            xous::Error::OutOfMemory => BioError::Oom,
            xous::Error::ServerNotFound => BioError::InvalidSm,
            xous::Error::ServerExists => BioError::NoFreeMachines,
            // Handle unmapped cases with a panic
            _ => panic!("Cannot convert Error::{:?} to BioError", error),
        }
    }
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct BioPin {
    pin_number: u8,
}
impl BioPin {
    pub fn pin_number(&self) -> u8 { self.pin_number }

    pub fn new(pin: u8) -> Self {
        if pin < 32 { Self { pin_number: pin } } else { panic!("Pin value out of range") }
    }
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct CoreConfig {
    pub div_int: u16,
    pub div_frac: u8,
    /// When some, configures use_extclk of the corresponding core to use the GPIO pin specified as the
    /// argument
    pub quantum_from_pin: Option<BioPin>,
}

impl From<CoreConfig> for CoreConfigRkyv {
    fn from(config: CoreConfig) -> Self {
        CoreConfigRkyv {
            div_int: config.div_int,
            div_frac: config.div_frac,
            quantum_from_pin: config.quantum_from_pin.map(|pin| pin.pin_number().into()),
        }
    }
}

impl From<CoreConfigRkyv> for CoreConfig {
    fn from(config: CoreConfigRkyv) -> Self {
        CoreConfig {
            div_int: config.div_int,
            div_frac: config.div_frac,
            quantum_from_pin: config.quantum_from_pin.map(|pin_num| BioPin::new(pin_num)),
        }
    }
}

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct CoreConfigRkyv {
    pub div_int: u16,
    pub div_frac: u8,
    /// When some, configures use_extclk of the corresponding core to use the GPIO pin specified as the
    /// argument
    pub quantum_from_pin: Option<u8>,
}

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
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

#[bitfield(u8)]
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct TriggerSlot {
    #[bits(1..=7)]
    _reserved: arbitrary_int::u7,
    #[bit(0, rw)]
    trigger_slot: arbitrary_int::u1,
}

#[bitfield(u8)]
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct FifoLevel {
    #[bits(3..=7)]
    _reserved: arbitrary_int::u5,
    #[bits(0..=2, rw)]
    level: arbitrary_int::u3,
}

/// The event register is divided into 24 code-settable event bits + 8 FIFO event bits
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
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

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct FifoEventConfigRkyv {
    // for which FIFO we're configuring its event triggers
    pub which: Fifo,
    // there are up to two trigger slots per FIFO, specify 0 or 1 here.
    pub trigger_slot: bool,
    // the level used for the triggers. Any number from 0-7.
    pub level: u8,
    // when set, the trigger condition happens compared to the level above
    pub trigger_less_than: bool,
    pub trigger_greater_than: bool,
    pub trigger_equal_to: bool,
}

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum Fifo {
    Fifo0,
    Fifo1,
    Fifo2,
    Fifo3,
}

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum Irq {
    Irq0,
    Irq1,
    Irq2,
    Irq3,
}

#[bitfield(u32)]
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
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

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
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
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct DmaWindow {
    pub base: u32,
    pub bounds: NonZeroU32,
}

/// Structure that defines all of the windows allowed by the system
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct DmaFilterWindows {
    pub windows: [Option<DmaWindow>; 4],
}

/*
#[bitfield(u32)]
pub struct PackedFifoLevels {
    #[bits(16..=31)]
    _padding: u16,
    #[bits(12..=15)]
    fifo3_level: arbitrary_int::u4,
    #[bits(8..=11)]
    fifo2_level: arbitrary_int::u4,
    #[bits(4..=7)]
    fifo1_level: arbitrary_int::u4,
    #[bits(0..=3)]
    fifo0_level: arbitrary_int::u4,
}

#[bitfield(u32)]
pub struct PackedEvent {
    #[bits(24..=31)]
    _padding: u8,
    #[bits(0..=23)]
    event: arbitrary_int::u24,
}
/// Memory-mapped structure for a FIFO endpoint. The returned structure
/// is literally mapped on top of the virtual address of the FIFO page starting at offset 0xC.
/// You could also access these fields using the UTRA abstractions, but with the the CSR base
/// set to the FIFO alias region
#[repr(C)]
pub struct FifoHandle {
    pub packed_fifo_levels: PackedFifoLevels,
    pub tx_fifo: [u32; 4],
    pub rx_fifo: [u32; 4],
    pub _unused: [u32; 2],
    pub set_event: PackedEvent,
    pub clear_event: PackedEvent,
    pub event_status: PackedEvent,
}
*/

pub struct CoreHandle<'a> {
    conn: xous::CID,
    handle: usize,
    _phantom: PhantomData<&'a ()>,
}

impl<'a> CoreHandle<'a> {
    pub fn new(conn: xous::CID, handle: usize) -> Self { Self { conn, handle, _phantom: PhantomData } }

    /// safety: this needs to be wrapped in a hardware-level CSR object that tracks the lifetime of
    /// the underlying pointer handle.
    pub unsafe fn handle(&self) -> (usize, PhantomData<&'a ()>) { (self.handle, self._phantom) }
}

impl Drop for CoreHandle<'_> {
    fn drop(&mut self) {
        xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                BioOp::ReleaseCoreHandle.to_usize().unwrap(),
                self.handle,
                0,
                0,
                0,
            ),
        )
        .unwrap();
    }
}

/// A platform-neutral API for the BIO. Intended to be usable in both baremetal and `std` environments
pub trait BioApi<'a> {
    /// Initializes a core with the given `code`, loading into `offset`, with a given
    /// core configuration.
    fn init_core(
        &self,
        core: BioCore,
        code: &[u8],
        offset: usize,
        config: CoreConfig,
    ) -> Result<(), BioError>;

    /// Releases the core. As a side effect, the core is stopped.
    fn de_init_core(&self, core: BioCore) -> Result<(), BioError>;

    /// Sets the frequency of the cores, used to calculate dividers etc.
    /// Returns the previous frequency used by the system. Setting this frequency
    /// allows the sleep/wake clock manager to do a best-effort rescaling of the
    /// clock dividers for each core as DVFS is applied for power savings.
    fn set_core_freq(&self, freq: u32) -> Result<u32, BioError>;

    /// Returns the frequency, in Hz, of the incoming clock to the BIO cores.
    fn get_freq(&self) -> u32;

    /// The index of `which` corresponds to each of the cores, 0-3.
    fn set_core_state(&self, which: [CoreRunSetting; 4]) -> Result<(), BioError>;

    /// Returns a `usize` which should be turned into a CSR as *mut u32 by the caller
    /// this can then be dereferenced using UTRA abstractions to access the following
    /// registers:
    ///   - SFR_FLEVEL
    ///   - SFR_TXF0-3
    ///   - SFR_RXF0-3
    ///   - SFR_EVENT_SET
    ///   - SFR_EVENT_CLR
    ///   - SFR_EVENT_STATUS
    ///
    /// The handle has a `Drop` implementation that releases it when it goes out of scope.
    ///
    /// Safety: this has to be wrapped in an object that derives a CSR that also tracks
    /// the lifetime of this object, to prevent `Drop` from being called at the wrong time.
    unsafe fn get_core_handle(&'a self) -> Result<CoreHandle<'a>, BioError>;

    /// This call sets up the BIO's IRQ routing. It doesn't actually claim the IRQ
    /// or install the handler - that's up to the caller to do with Xous API calls.
    fn setup_irq_config(&self, config: IrqConfig) -> Result<(), BioError>;

    /// Allows BIO cores to DMA to/from the windows specified in `windows`. DMA filtering is
    /// on by default with no windows allowed.
    fn setup_dma_windows(&self, windows: DmaFilterWindows) -> Result<(), BioError>;

    /// Sets up the BIO I/O configuration
    fn setup_io_config(&self, config: IoConfig) -> Result<(), BioError>;

    /// Sets up a FIFO event trigger
    fn setup_fifo_event_triggers(&self, config: FifoEventConfig) -> Result<(), BioError>;

    /// Returns a version code for the underlying hardware.
    fn get_version(&self) -> u32;
}
