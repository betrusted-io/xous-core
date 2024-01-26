use core::mem::size_of;

use utralib::generated::*;

use crate::ifram::{IframRange, UdmaWidths};

/// UDMA has a structure that Rust hates. The concept of UDMA is to take a bunch of
/// different hardware functions, and access them with a template register pattern.
/// But with small asterisks here and there depending upon the hardware block in question.
///
/// It is essentially polymorphism at the hardware level, but with special cases meant
/// to be patched up with instance-specific peeks and pokes. It's probably possible
/// to create a type system that can safe-ify this kind of structure, but just because
/// something is possible does not mean it's a good idea to do it, nor would it be
/// maintainable and/or ergonomic to use.
///
/// Anyways. Lots of unsafe code here. UDMA: specious concept, made entirely of footguns.

// --------------------------- Global Shared State (!!🤌!!) --------------------------
#[repr(usize)]
enum GlobalReg {
    ClockGate = 0,
    EventIn = 1,
}
impl Into<usize> for GlobalReg {
    fn into(self) -> usize { self as usize }
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
#[derive(Copy, Clone, num_derive::FromPrimitive)]
pub enum EventChannel {
    Channel0 = 0,
    Channel1 = 8,
    Channel2 = 16,
    Channel3 = 24,
}
pub struct GlobalConfig {
    csr: CSR<u32>,
}
impl GlobalConfig {
    pub fn new(base_addr: *mut u32) -> Self { GlobalConfig { csr: CSR::new(base_addr) } }

    pub fn clock_on(&mut self, peripheral: PeriphId) {
        // Safety: only safe when used in the context of UDMA registers.
        unsafe {
            self.csr.base().add(GlobalReg::ClockGate.into()).write_volatile(
                self.csr.base().add(GlobalReg::ClockGate.into()).read_volatile() | peripheral as u32,
            );
        }
    }

    pub fn clock_off(&mut self, peripheral: PeriphId) {
        // Safety: only safe when used in the context of UDMA registers.
        unsafe {
            self.csr.base().add(GlobalReg::ClockGate.into()).write_volatile(
                self.csr.base().add(GlobalReg::ClockGate.into()).read_volatile() & !(peripheral as u32),
            );
        }
    }

    pub fn raw_clock_map(&self) -> u32 {
        // Safety: only safe when used in the context of UDMA registers.
        unsafe { self.csr.base().add(GlobalReg::ClockGate.into()).read_volatile() }
    }

    pub fn is_clock_set(&self, peripheral: PeriphId) -> bool {
        // Safety: only safe when used in the context of UDMA registers.
        unsafe {
            (self.csr.base().add(GlobalReg::ClockGate.into()).read_volatile() & (peripheral as u32)) != 0
        }
    }

    pub fn map_event(&mut self, peripheral: PeriphId, event_type: PeriphEventType, to_channel: EventChannel) {
        let event_type: u32 = event_type.into();
        let id: u32 = PeriphEventId::from(peripheral) as u32 + event_type;
        // Safety: only safe when used in the context of UDMA registers.
        unsafe {
            self.csr.base().add(GlobalReg::EventIn.into()).write_volatile(
                self.csr.base().add(GlobalReg::EventIn.into()).read_volatile()
                    & !(0xFF << (to_channel as u32))
                    | id << (to_channel as u32),
            )
        }
    }

    /// Same as map_event(), but for cases where the offset is known. This would typically be the case
    /// where a remote function transformed a PeriphEventType into a primitive `u32` and passed
    /// it through an IPC interface.
    pub fn map_event_with_offset(
        &mut self,
        peripheral: PeriphId,
        event_offset: u32,
        to_channel: EventChannel,
    ) {
        let id: u32 = PeriphEventId::from(peripheral) as u32 + event_offset;
        // Safety: only safe when used in the context of UDMA registers.
        unsafe {
            self.csr.base().add(GlobalReg::EventIn.into()).write_volatile(
                self.csr.base().add(GlobalReg::EventIn.into()).read_volatile()
                    & !(0xFF << (to_channel as u32))
                    | id << (to_channel as u32),
            )
        }
    }

    pub fn raw_event_map(&self) -> u32 {
        // Safety: only safe when used in the context of UDMA registers.
        unsafe { self.csr.base().add(GlobalReg::EventIn.into()).read_volatile() }
    }
}

// --------------------------------- DMA channel ------------------------------------
const CFG_EN: u32 = 0b01_0000; // start a transfer
#[allow(dead_code)]
const CFG_CONT: u32 = 0b00_0001; // continuous mode
#[allow(dead_code)]
const CFG_SIZE_8: u32 = 0b00_0000; // 8-bit transfer
#[allow(dead_code)]
const CFG_SIZE_16: u32 = 0b00_0010; // 16-bit transfer
#[allow(dead_code)]
const CFG_SIZE_32: u32 = 0b00_0100; // 32-bit transfer
#[allow(dead_code)]
const CFG_CLEAR: u32 = 0b10_0000; // stop and clear all pending transfers
const CFG_SHADOW: u32 = 0b10_0000; // indicates a shadow transfer

#[repr(usize)]
pub enum Bank {
    Rx = 0,
    Tx = 0x10 / size_of::<u32>(),
    // woo dat special case...
    Custom = 0x20 / size_of::<u32>(),
}
impl Into<usize> for Bank {
    fn into(self) -> usize { self as usize }
}

/// Crate-local struct that defines the offset of registers in UDMA banks, as words.
#[repr(usize)]
enum DmaReg {
    Saddr = 0,
    Size = 1,
    Cfg = 2,
    #[allow(dead_code)]
    IntCfg = 3,
}
impl Into<usize> for DmaReg {
    fn into(self) -> usize { self as usize }
}

pub trait Udma {
    /// Every implementation of Udma has to implement the csr_mut() accessor
    fn csr_mut(&mut self) -> &mut CSR<u32>;
    /// Every implementation of Udma has to implement the csr() accessor
    fn csr(&self) -> &CSR<u32>;

    /// `bank` selects which UDMA bank is the target
    /// `buf` is a slice that points to the memory that is the target of the UDMA. Needs to be accessible
    ///    by the UDMA subsystem, e.g. in IFRAM0/IFRAM1 range, and is a *physical address* even in a
    ///    system running on virtual memory (!!!)
    /// `config` is a device-specific word that configures the DMA.
    ///
    /// Safety: the `buf` has to be allocated, length-checked, and in the range of memory
    /// that is valid for UDMA targets
    unsafe fn udma_enqueue<T>(&self, bank: Bank, buf: &[T], config: u32) {
        let bank_addr = self.csr().base().add(bank as usize);
        let buf_addr = buf.as_ptr() as u32;
        bank_addr.add(DmaReg::Saddr.into()).write_volatile(buf_addr);
        bank_addr.add(DmaReg::Size.into()).write_volatile((buf.len() * size_of::<T>()) as u32);
        bank_addr.add(DmaReg::Cfg.into()).write_volatile(config | CFG_EN)
    }
    fn udma_can_enqueue(&self, bank: Bank) -> bool {
        // Safety: only safe when used in the context of UDMA registers.
        unsafe {
            (self.csr().base().add(bank as usize).add(DmaReg::Cfg.into()).read_volatile() & CFG_SHADOW) == 0
        }
    }
    fn udma_busy(&self, bank: Bank) -> bool {
        // Safety: only safe when used in the context of UDMA registers.
        unsafe {
            (self.csr().base().add(bank as usize).add(DmaReg::Cfg.into()).read_volatile() & CFG_EN) != 0
        }
    }
}

// ----------------------------------- UART ------------------------------------
#[repr(usize)]
enum UartReg {
    Status = 0,
    Setup = 1,
}

pub enum UartChannel {
    Uart0,
    Uart1,
    Uart2,
    Uart3,
}

impl Into<usize> for UartReg {
    fn into(self) -> usize { self as usize }
}

/// UDMA UART wrapper. Contains all the warts on top of the Channel abstraction.
pub struct Uart {
    /// This is assumed to point to the base of the peripheral's UDMA register set.
    csr: CSR<u32>,
    ifram: IframRange,
}

/// Blanket implementations to access the CSR within UART. Needed because you can't
/// have default fields in traits: https://github.com/rust-lang/rfcs/pull/1546
impl Udma for Uart {
    fn csr_mut(&mut self) -> &mut CSR<u32> { &mut self.csr }

    fn csr(&self) -> &CSR<u32> { &self.csr }
}
/// The sum of UART_TX_BUF_SIZE + UART_RX_BUF_SIZE should be 4096.
const UART_TX_BUF_SIZE: usize = 2048;
const UART_RX_BUF_SIZE: usize = 2048;
impl Uart {
    /// Configures for N81
    ///
    /// This function is `unsafe` because it can only be called after the
    /// global shared UDMA state has been set up to un-gate clocks and set up
    /// events.
    ///
    /// It is also `unsafe` on Drop because you have to remember to unmap
    /// the clock manually as well once the object is dropped...
    ///
    /// Allocates a 4096-deep buffer for tx/rx purposes: the first 2048 bytes
    /// are used for Tx, the second 2048 bytes for Rx.
    #[cfg(not(feature = "baremetal"))]
    pub unsafe fn new(channel: UartChannel, baud: u32, clk_freq: u32) -> Self {
        assert!(UART_RX_BUF_SIZE + UART_TX_BUF_SIZE == 4096, "Configuration error in UDMA UART");
        let bank_addr = match channel {
            UartChannel::Uart0 => utra::udma_uart_0::HW_UDMA_UART_0_BASE,
            UartChannel::Uart1 => utra::udma_uart_1::HW_UDMA_UART_1_BASE,
            UartChannel::Uart2 => utra::udma_uart_2::HW_UDMA_UART_2_BASE,
            UartChannel::Uart3 => utra::udma_uart_3::HW_UDMA_UART_3_BASE,
        };
        let uart = xous::syscall::map_memory(
            xous::MemoryAddress::new(bank_addr),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map serial port");

        // now setup the channel
        let csr = CSR::new(uart.as_mut_ptr() as *mut u32);

        let clk_counter: u32 = (clk_freq + baud / 2) / baud;
        // setup baud, bits, parity, etc.
        csr.base()
            .add(Bank::Custom.into())
            .add(UartReg::Setup.into())
            .write_volatile(0x0306 | (clk_counter << 16));

        Uart { csr, ifram: IframRange::request(UART_RX_BUF_SIZE + UART_TX_BUF_SIZE, None).unwrap() }
    }

    /// Gets a handle to the UART. Used for re-acquiring previously initialized
    /// UART hardware, such as from the loader booting into Xous
    ///
    /// Safety: only safe to call in the context of a previously initialized UART
    pub unsafe fn get_handle(csr_virt_addr: usize, udma_phys_addr: usize, udma_virt_addr: usize) -> Self {
        assert!(UART_RX_BUF_SIZE + UART_TX_BUF_SIZE == 4096, "Configuration error in UDMA UART");
        let csr = CSR::new(csr_virt_addr as *mut u32);
        Uart {
            csr,
            ifram: IframRange::from_raw_parts(
                udma_phys_addr,
                udma_virt_addr,
                UART_RX_BUF_SIZE + UART_TX_BUF_SIZE,
            ),
        }
    }

    pub fn disable(&mut self) {
        self.wait_tx_done();
        // safe only in the context of a UART UDMA address
        unsafe {
            self.csr.base().add(Bank::Custom.into()).add(UartReg::Setup.into()).write_volatile(0x0050_0006);
        }
    }

    pub fn tx_busy(&self) -> bool {
        // safe only in the context of a UART UDMA address
        unsafe {
            (self.csr.base().add(Bank::Custom.into()).add(UartReg::Status.into()).read_volatile() & 1) != 0
        }
    }

    pub fn rx_busy(&self) -> bool {
        // safe only in the context of a UART UDMA address
        unsafe {
            (self.csr.base().add(Bank::Custom.into()).add(UartReg::Status.into()).read_volatile() & 2) != 0
        }
    }

    pub fn wait_tx_done(&self) {
        while self.udma_busy(Bank::Tx) {
            #[cfg(not(feature = "baremetal"))]
            xous::yield_slice();
        }
        while self.tx_busy() {}
    }

    pub fn wait_rx_done(&self) {
        while self.udma_busy(Bank::Rx) {
            #[cfg(not(feature = "baremetal"))]
            xous::yield_slice();
        }
    }

    /// `buf` is assumed to be a virtual address (in Xous), or a machine address
    /// (in baremetal mode). This function is safe because it will operate as intended
    /// within a given environment, so long as the `baremetal` flag is applied correctly.
    ///
    /// In "baremetal" mode, it's *also* assumed that `buf` is range-checked to be valid
    /// for the UDMA engine.
    ///
    /// returns: total length of bytes written
    pub fn write(&mut self, buf: &[u8]) -> usize {
        let mut writelen = 0;
        for chunk in buf.chunks(UART_TX_BUF_SIZE) {
            #[cfg(not(feature = "baremetal"))]
            {
                self.ifram.as_slice_mut()[..chunk.len()].copy_from_slice(chunk);
                // safety: the slice is in the physical range for the UDMA, and length-checked
                unsafe {
                    self.udma_enqueue(
                        Bank::Tx,
                        &self.ifram.as_phys_slice::<u8>()[..chunk.len()],
                        CFG_EN | CFG_SIZE_8,
                    );
                }
                writelen += chunk.len();
            }
            #[cfg(feature = "baremetal")]
            unsafe {
                self.udma_enqueue(Bank::Tx, chunk, CFG_EN | CFG_SIZE_8);
                writelen += chunk.len();
            }

            self.wait_tx_done();
        }
        writelen
    }

    pub fn read(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(UART_RX_BUF_SIZE) {
            #[cfg(not(feature = "baremetal"))]
            unsafe {
                self.udma_enqueue(
                    Bank::Rx,
                    &self.ifram.as_phys_slice::<u8>()[UART_TX_BUF_SIZE..UART_TX_BUF_SIZE + chunk.len()],
                    CFG_EN | CFG_SIZE_8,
                );
            }
            #[cfg(feature = "baremetal")]
            unsafe {
                self.udma_enqueue(
                    Bank::Rx,
                    &chunk[UART_TX_BUF_SIZE..UART_TX_BUF_SIZE + chunk.len()],
                    CFG_EN | CFG_SIZE_8,
                );
            }
            self.wait_rx_done();
        }
    }
}

// ----------------------------------- SPIM ------------------------------------

/// The SPIM implementation for UDMA does reg-ception, in that they bury
/// a register set inside a register set. The registers are only accessible by,
/// surprise, DMA. The idea behind this is you can load a bunch of commands into
/// memory and just DMA them to the control interface. Sure, cool idea bro.
///
/// Anyways, the autodoc system is unable to extract the register
/// formats for the SPIM. Instead, we have to create a set of hand-crafted
/// structures to deal with this.

#[repr(u32)]
#[derive(Copy, Clone)]
pub enum SpimClkPol {
    LeadingEdgeRise = 0,
    LeadingEdgeFall = 1,
}
#[repr(u32)]
#[derive(Copy, Clone)]
pub enum SpimClkPha {
    CaptureOnLeading = 0,
    CaptureOnTrailing = 1,
}
#[repr(u32)]
#[derive(Copy, Clone)]
pub enum SpimCs {
    Cs0 = 0,
    Cs1 = 1,
    Cs2 = 2,
    Cs3 = 3,
}
#[repr(u32)]
#[derive(Copy, Clone)]
pub enum SpimMode {
    Standard = 0,
    Quad = 1,
}
#[repr(u32)]
#[derive(Copy, Clone)]
pub enum SpimByteAlign {
    Enable = 0,
    Disable = 1,
}
#[repr(u32)]
#[derive(Copy, Clone)]
pub enum SpimCheckType {
    Allbits = 0,
    OnlyOnes = 1,
    OnlyZeros = 2,
}
#[repr(u32)]
#[derive(Copy, Clone)]
pub enum SpimEventGen {
    Disabled = 0,
    Enabled = 1,
}
#[derive(Copy, Clone)]
pub enum SpimCmd {
    /// pol, pha, clkdiv
    Config(SpimClkPol, SpimClkPha, u8),
    StartXfer(SpimCs),
    /// mode, cmd_size (5 bits), command value, left-aligned
    SendCmd(SpimMode, u8, u16),
    /// mode, number of address bits (5 bits)
    SendAddr(SpimMode, u8),
    /// number of cycles (5 bits)
    Dummy(u8),
    /// Wait on an event. Note EventChannel coding needs interpretation prior to use.
    Wait(EventChannel),
    /// mode, use byte alignment, number of bits to send
    TxData(SpimMode, SpimByteAlign, u16),
    /// mode, use byte alignment, number of bits to receive
    RxData(SpimMode, SpimByteAlign, u16),
    /// repeat count
    RepeatNextCmd(u16),
    EndXfer(SpimEventGen),
    EndRepeat,
    /// mode, use byte alignment, check type, size of comparison (4 bits), comparison data
    RxCheck(SpimMode, SpimByteAlign, SpimCheckType, u8, u16),
    /// use byte alignment, size of data
    FullDuplex(SpimByteAlign, u16),
}
impl Into<u32> for SpimCmd {
    fn into(self) -> u32 {
        match self {
            SpimCmd::Config(pol, pha, div) => 0 << 28 | (pol as u32) << 9 | (pha as u32) << 8 | div as u32,
            SpimCmd::StartXfer(cs) => 1 << 28 | cs as u32,
            SpimCmd::SendCmd(mode, size, cmd) => {
                2 << 28 | (mode as u32) << 27 | (size as u32 & 0x1F) << 16 | cmd as u32
            }
            SpimCmd::SendAddr(mode, size) => 3 << 28 | (mode as u32) << 27 | (size as u32 & 0x1F) << 16,
            SpimCmd::Dummy(cycles) => 4 << 28 | (cycles as u32 & 0x1F) << 16,
            SpimCmd::Wait(channel) => match channel {
                EventChannel::Channel0 => 5 << 28 | 0,
                EventChannel::Channel1 => 5 << 28 | 1,
                EventChannel::Channel2 => 5 << 28 | 2,
                EventChannel::Channel3 => 5 << 28 | 3,
            },
            SpimCmd::TxData(mode, align, len) => {
                6 << 28 | (mode as u32) << 27 | (align as u32) << 26 | (len as u32)
            }
            SpimCmd::RxData(mode, align, len) => {
                7 << 28 | (mode as u32) << 27 | (align as u32) << 26 | (len as u32)
            }
            SpimCmd::RepeatNextCmd(count) => 8 << 28 | count as u32,
            SpimCmd::EndXfer(event) => 9 << 28 | event as u32,
            SpimCmd::EndRepeat => 10 << 28,
            SpimCmd::RxCheck(mode, align, check_type, size, data) => {
                11 << 28
                    | (mode as u32) << 27
                    | (align as u32) << 26
                    | (check_type as u32) << 24
                    | (size as u32 & 0xF) << 16
                    | data as u32
            }
            SpimCmd::FullDuplex(align, len) => 12 << 28 | (align as u32) << 26 | len as u32,
        }
    }
}
pub enum SpimChannel {
    Channel0,
    Channel1,
    Channel2,
    Channel3,
}
pub struct Spim {
    csr: CSR<u32>,
    cs: SpimCs,
    event_channel: Option<EventChannel>,
    mode: SpimMode,
    align: SpimByteAlign,
    ifram: IframRange,
    // starts at the base of ifram range
    tx_buf_len_bytes: usize,
    // immediately after the tx buf len
    rx_buf_len_bytes: usize,
}

// length of the command buffer
const SPIM_CMD_BUF_LEN_BYTES: usize = 16;

impl Udma for Spim {
    fn csr_mut(&mut self) -> &mut CSR<u32> { &mut self.csr }

    fn csr(&self) -> &CSR<u32> { &self.csr }
}

impl Spim {
    /// This function is `unsafe` because it can only be called after the
    /// global shared UDMA state has been set up to un-gate clocks and set up
    /// events.
    ///
    /// It is also `unsafe` on Drop because you have to remember to unmap
    /// the clock manually as well once the object is dropped...
    ///
    /// Return: the function can return None if it can't allocate enough memory
    /// for the requested tx/rx length.
    pub unsafe fn new(
        channel: SpimChannel,
        spi_clk_freq: u32,
        sys_clk_freq: u32,
        pol: SpimClkPol,
        pha: SpimClkPha,
        chip_select: SpimCs,
        event_channel: Option<EventChannel>,
        max_tx_len_bytes: usize,
        max_rx_len_bytes: usize,
    ) -> Option<Self> {
        // now setup the channel
        let base_addr = match channel {
            SpimChannel::Channel0 => utra::udma_spim_0::HW_UDMA_SPIM_0_BASE,
            SpimChannel::Channel1 => utra::udma_spim_1::HW_UDMA_SPIM_1_BASE,
            SpimChannel::Channel2 => utra::udma_spim_2::HW_UDMA_SPIM_2_BASE,
            SpimChannel::Channel3 => utra::udma_spim_3::HW_UDMA_SPIM_3_BASE,
        };
        let csr = CSR::new(base_addr as *mut u32);

        let clk_div = sys_clk_freq / spi_clk_freq;
        // make this a hard panic -- you'll find out at runtime that you f'd up
        // but at least you find out.
        assert!(clk_div < 256, "SPI clock divider is out of range");

        let mut reqlen = max_tx_len_bytes + max_rx_len_bytes + SPIM_CMD_BUF_LEN_BYTES;
        if reqlen % 4096 != 0 {
            // round up to the nearest page size
            reqlen = (reqlen + 4096) & !4095;
        }
        if let Some(ifram) = IframRange::request(reqlen, None) {
            let mut spim = Spim {
                csr,
                cs: chip_select,
                event_channel,
                align: SpimByteAlign::Disable,
                mode: SpimMode::Standard,
                ifram,
                tx_buf_len_bytes: max_tx_len_bytes,
                rx_buf_len_bytes: max_rx_len_bytes,
            };
            // setup the interface using a UDMA command
            spim.send_cmd_list(&[SpimCmd::Config(pol, pha, clk_div as u8)]);

            Some(spim)
        } else {
            None
        }
    }

    /// The command buf is *always* a `u32`; so tie the type down here.
    fn cmd_buf_mut(&mut self) -> &mut [u32] {
        &mut self.ifram.as_slice_mut()[(self.tx_buf_len_bytes + self.rx_buf_len_bytes) / size_of::<u32>()
            ..(self.tx_buf_len_bytes + self.rx_buf_len_bytes + SPIM_CMD_BUF_LEN_BYTES) / size_of::<u32>()]
    }

    unsafe fn cmd_buf_phys(&self) -> &[u32] {
        &self.ifram.as_phys_slice()[(self.tx_buf_len_bytes + self.rx_buf_len_bytes) / size_of::<u32>()
            ..(self.tx_buf_len_bytes + self.rx_buf_len_bytes + SPIM_CMD_BUF_LEN_BYTES) / size_of::<u32>()]
    }

    pub fn rx_buf<T: UdmaWidths>(&mut self) -> &[T] {
        &self.ifram.as_slice()[(self.tx_buf_len_bytes) / size_of::<T>()
            ..(self.tx_buf_len_bytes + self.rx_buf_len_bytes) / size_of::<T>()]
    }

    unsafe fn rx_buf_phys<T: UdmaWidths>(&self) -> &[T] {
        &self.ifram.as_phys_slice()[(self.tx_buf_len_bytes) / size_of::<T>()
            ..(self.tx_buf_len_bytes + self.rx_buf_len_bytes) / size_of::<T>()]
    }

    pub fn tx_buf_mut<T: UdmaWidths>(&mut self) -> &mut [T] {
        &mut self.ifram.as_slice_mut()[..self.tx_buf_len_bytes / size_of::<T>()]
    }

    unsafe fn tx_buf_phys<T: UdmaWidths>(&mut self) -> &[T] {
        &self.ifram.as_phys_slice()[..self.tx_buf_len_bytes / size_of::<T>()]
    }

    fn send_cmd_list(&mut self, cmds: &[SpimCmd]) {
        for cmd_chunk in cmds.chunks(SPIM_CMD_BUF_LEN_BYTES / size_of::<u32>()) {
            for (src, dst) in cmd_chunk.iter().zip(self.cmd_buf_mut().iter_mut()) {
                *dst = (*src).into();
            }
            // safety: this is safe because the cmd_buf_phys() slice is passed to a function that only
            // uses it as a base/bounds reference and it will not actually access the data.
            unsafe {
                self.udma_enqueue(
                    Bank::Custom,
                    &self.cmd_buf_phys()[..cmd_chunk.len()],
                    CFG_EN | CFG_SIZE_32,
                );
            }
        }
    }
}
