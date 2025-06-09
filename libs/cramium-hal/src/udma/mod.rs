pub mod i2c;
pub mod spim;
pub mod uart;

use core::mem::size_of;

use cramium_api::*;
pub use i2c::*;
pub use spim::*;
pub use uart::*;
use utralib::*;

use crate::SharedCsr;

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
    Reset = 2,
}
impl Into<usize> for GlobalReg {
    fn into(self) -> usize { self as usize }
}

pub struct GlobalConfig {
    csr: SharedCsr<u32>,
    i2c_irq: SharedCsr<u32>,
    i2c_err_irq: SharedCsr<u32>,
}
impl GlobalConfig {
    pub fn new() -> Self {
        #[cfg(target_os = "xous")]
        let base_addr = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::generated::HW_UDMA_CTRL_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map UDMA global control")
        .as_mut_ptr() as *mut u32;
        #[cfg(not(target_os = "xous"))]
        let base_addr = utra::udma_ctrl::HW_UDMA_CTRL_BASE as *mut u32;

        #[cfg(target_os = "xous")]
        let i2c_irq_addr = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::generated::HW_IRQARRAY7_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map I2C IRQ range")
        .as_mut_ptr() as *mut u32;
        #[cfg(not(target_os = "xous"))]
        let i2c_irq_addr = utralib::generated::HW_IRQARRAY7_BASE as *mut u32;

        #[cfg(target_os = "xous")]
        let i2c_err_irq_addr = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::generated::HW_IRQARRAY12_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map I2C IRQ range")
        .as_mut_ptr() as *mut u32;
        #[cfg(not(target_os = "xous"))]
        let i2c_err_irq_addr = utralib::generated::HW_IRQARRAY12_BASE as *mut u32;

        GlobalConfig {
            csr: SharedCsr::new(base_addr),
            i2c_irq: SharedCsr::new(i2c_irq_addr),
            i2c_err_irq: SharedCsr::new(i2c_err_irq_addr),
        }
    }

    pub fn clock_on(&self, peripheral: PeriphId) {
        // Safety: only safe when used in the context of UDMA registers.
        unsafe {
            self.csr.base().add(GlobalReg::ClockGate.into()).write_volatile(
                self.csr.base().add(GlobalReg::ClockGate.into()).read_volatile() | peripheral as u32,
            );
        }
    }

    pub fn clock_off(&self, peripheral: PeriphId) {
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

    pub fn map_event(&self, peripheral: PeriphId, event_type: PeriphEventType, to_channel: EventChannel) {
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
    pub fn map_event_with_offset(&self, peripheral: PeriphId, event_offset: u32, to_channel: EventChannel) {
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

    pub fn reset(&self, peripheral: PeriphId) {
        unsafe {
            // assert reset
            self.csr.base().add(GlobalReg::Reset.into()).write_volatile(peripheral.into());
            // a few nops for the reset to propagate
            core::arch::asm!(
                "nop", "nop", "nop", "nop", "nop", "nop", "nop", "nop", "nop", "nop", "nop", "nop", "nop",
                "nop", "nop", "nop",
            );
            // de-assert reset
            self.csr.base().add(GlobalReg::Reset.into()).write_volatile(0);
        }
    }
}

impl UdmaGlobalConfig for GlobalConfig {
    fn clock(&self, peripheral: PeriphId, enable: bool) {
        if enable {
            self.clock_on(peripheral);
        } else {
            self.clock_off(peripheral);
        }
    }

    unsafe fn udma_event_map(
        &self,
        peripheral: PeriphId,
        event_type: PeriphEventType,
        to_channel: EventChannel,
    ) {
        self.map_event(peripheral, event_type, to_channel);
    }

    fn reset(&self, peripheral: PeriphId) { self.reset(peripheral); }

    fn irq_status_bits(&self, bank: IrqBank) -> u32 {
        match bank {
            IrqBank::I2c => {
                let pending = self.i2c_irq.r(utralib::utra::irqarray7::EV_PENDING);
                self.i2c_irq.wo(utralib::utra::irqarray7::EV_PENDING, pending);
                pending
            }
            IrqBank::I2cErr => {
                let pending = self.i2c_err_irq.r(utralib::utra::irqarray12::EV_PENDING);
                self.i2c_err_irq.wo(utralib::utra::irqarray12::EV_PENDING, pending);
                pending
            }
        }
    }
}
// --------------------------------- DMA channel ------------------------------------
pub(crate) const CFG_EN: u32 = 0b01_0000; // start a transfer
#[allow(dead_code)]
pub(crate) const CFG_CONT: u32 = 0b00_0001; // continuous mode
#[allow(dead_code)]
pub(crate) const CFG_SIZE_8: u32 = 0b00_0000; // 8-bit transfer
#[allow(dead_code)]
pub(crate) const CFG_SIZE_16: u32 = 0b00_0010; // 16-bit transfer
#[allow(dead_code)]
pub(crate) const CFG_SIZE_32: u32 = 0b00_0100; // 32-bit transfer
#[allow(dead_code)]
/// NOTE NOTE NOTE: The position of this bit is different in the RTL from the documentation
/// Bit 6 is what is in the RTL, so we are using this instead of bit 5, which is the docus
pub(crate) const CFG_CLEAR: u32 = 0b100_0000; // stop and clear all pending transfers
#[allow(dead_code)]
/// NOTE NOTE NOTE: the transfer pending bit *is* in the correct place
pub(crate) const CFG_PENDING: u32 = 0b10_0000; // indicates a transfer pending
pub(crate) const CFG_SHADOW: u32 = 0b10_0000; // indicates a shadow transfer

#[allow(dead_code)]
pub(crate) const CFG_BACKPRESSURE: u32 = 0b1000_0000; // use RX backpressure to stall interface (found on SPIM in NTO)

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
pub enum DmaReg {
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
        /*
        crate::println!(
            "udma_enqueue: @{:x}[{}]/{:x}",
            buf_addr,
            (buf.len() * size_of::<T>()) as u32,
            config | CFG_EN
        ); */
        bank_addr.add(DmaReg::Saddr.into()).write_volatile(buf_addr);
        bank_addr.add(DmaReg::Size.into()).write_volatile((buf.len() * size_of::<T>()) as u32);
        bank_addr.add(DmaReg::Cfg.into()).write_volatile(config | CFG_EN)
    }
    fn udma_reset(&self, bank: Bank) {
        unsafe {
            let bank_addr = self.csr().base().add(bank as usize);
            bank_addr.add(DmaReg::Cfg.into()).write_volatile(CFG_CLEAR);
        }
    }
    fn udma_can_enqueue(&self, bank: Bank) -> bool {
        // Safety: only safe when used in the context of UDMA registers.
        unsafe {
            (self.csr().base().add(bank as usize).add(DmaReg::Cfg.into()).read_volatile() & CFG_SHADOW) == 0
        }
    }
    fn udma_busy(&self, bank: Bank) -> bool {
        // create dummy traffic on IFRAM that causes stall conditions on the bus
        // the write is totally bogus and
        #[cfg(feature = "udma-stress-test")]
        for i in 0..12 {
            unsafe {
                let ifram_tickle = 0x5000_0000 as *mut u32;
                ifram_tickle.add(i).write_volatile(i as u32);
            }
        }
        // Safety: only safe when used in the context of UDMA registers.
        unsafe {
            let saddr = self.csr().base().add(bank as usize).add(DmaReg::Saddr.into()).read_volatile();
            // crate::println!("brx: {:x}", saddr);
            saddr != 0
        }
    }
}
