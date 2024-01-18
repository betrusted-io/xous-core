use core::mem::size_of;

use utralib::generated::*;

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
#[derive(Copy, Clone)]
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
#[derive(Copy, Clone)]
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

/// The common UDMA channel structure shared across all UDMA peripherals
pub struct Channel {
    /// This is assumed to point to the base of the peripheral's UDMA register set.
    csr: CSR<u32>,
}

impl Channel {
    pub fn new(udma_base_addr: *mut u32) -> Self { Channel { csr: CSR::new(udma_base_addr) } }

    /// `bank` selects which UDMA bank is the target
    /// `buf` is a slice that points to the memory that is the target of the UDMA. Needs to be accessible
    ///    by the UDMA subsystem, e.g. in IFRAM0/IFRAM1 range, and is a *physical address* even in a
    ///    system running on virtual memory (!!!)
    /// `config` is a device-specific word that configures the DMA.
    pub fn enqueue(&mut self, bank: Bank, buf: &[u8], config: u32) {
        // Safety: only safe when used in the context of UDMA registers.
        unsafe {
            let bank_addr = self.csr.base().add(bank as usize);
            let buf_addr = buf.as_ptr() as u32;
            bank_addr.add(DmaReg::Saddr.into()).write_volatile(buf_addr);
            bank_addr.add(DmaReg::Size.into()).write_volatile(buf.len() as u32);
            bank_addr.add(DmaReg::Cfg.into()).write_volatile(config | CFG_EN)
        }
    }

    pub fn can_enqueue(&self, bank: Bank) -> bool {
        // Safety: only safe when used in the context of UDMA registers.
        unsafe {
            (self.csr.base().add(bank as usize).add(DmaReg::Cfg.into()).read_volatile() & CFG_SHADOW) == 0
        }
    }

    pub fn busy(&self, bank: Bank) -> bool {
        // Safety: only safe when used in the context of UDMA registers.
        unsafe { (self.csr.base().add(bank as usize).add(DmaReg::Cfg.into()).read_volatile() & CFG_EN) != 0 }
    }
}

// ----------------------------------- UART ------------------------------------
#[repr(usize)]
enum UartReg {
    Status = 0,
    Setup = 1,
}
impl Into<usize> for UartReg {
    fn into(self) -> usize { self as usize }
}

/// UDMA UART wrapper. Contains all the warts on top of the Channel abstraction.
pub struct Uart {
    /// This is assumed to point to the base of the peripheral's UDMA register set.
    csr: CSR<u32>,
    udma: Channel,
}

impl Uart {
    /// Configures for N81
    ///
    /// This function is `unsafe` because it can only be called after the
    /// global shared UDMA state has been set up to un-gate clocks and set up
    /// events.
    ///
    /// It is also `unsafe` on Drop because you have to remember to unmap
    /// the clock manually as well once the object is dropped...
    pub unsafe fn new(base_addr: usize, baud: u32, clk_freq: u32) -> Self {
        // now setup the channel
        let channel = Channel::new(base_addr as *mut u32);
        let csr = CSR::new(base_addr as *mut u32);

        let clk_counter: u32 = (clk_freq + baud / 2) / baud;
        // safe only in the context of a UART UDMA address
        unsafe {
            // setup baud, bits, parity, etc.
            csr.base()
                .add(Bank::Custom.into())
                .add(UartReg::Setup.into())
                .write_volatile(0x0306 | (clk_counter << 16))
        }
        Uart { csr, udma: channel }
    }

    pub fn disable(&mut self) {
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
        while self.udma.busy(Bank::Tx) {}
        while self.tx_busy() {}
    }

    pub fn wait_rx_done(&self) { while self.udma.busy(Bank::Rx) {} }

    pub fn write(&mut self, buf: &[u8]) {
        self.udma.enqueue(Bank::Tx, buf, CFG_EN | CFG_SIZE_8);
        self.wait_tx_done();
    }

    pub fn read(&mut self, buf: &mut [u8]) {
        self.udma.enqueue(Bank::Rx, buf, CFG_EN | CFG_SIZE_8);
        self.wait_rx_done();
    }
}

impl Drop for Uart {
    fn drop(&mut self) {
        self.wait_tx_done();
        self.disable();
        // NOTE: this does not unmap the clock on drop, because clocks are managed by global shared state.
    }
}
