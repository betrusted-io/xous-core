use std::pin::Pin;

use arbitrary_int::u4;
use bao1x_api::IrqNotification;
use bitbybit::bitfield;
use utralib::*;

#[bitfield(u32)]
#[derive(PartialEq, Eq, Debug)]
// this is the AOFR register - ao_sysctrl::SFR_AOFR
pub struct AoIntStatus {
    #[bit(6, rw)]
    ao_pad_in0: bool,
    #[bit(5, rw)]
    ao_pad_in1: bool,
    #[bit(4, rw)]
    wakeup_valid: bool,
    #[bit(3, rw)]
    kpc_interrupt: bool,
    #[bit(2, rw)]
    timer_interrupt: bool,
    #[bit(1, rw)]
    rtc_interrupt: bool,
    #[bit(0, rw)]
    wdt_reset: bool,
}

// Numbers here are relative to the bit position within the IRQ2 register
#[repr(u32)]
pub enum IrqMapping {
    // AoWakeup triggers based on either of PI0 or PI1 going low, plus a subset of internal wakeup sources
    AoWakeup = 15,
    // This is the subset of internal sources
    AoInt = 14,
    Watchdog = 13,
    Timer1 = 12,
    Timer0 = 11,
    Reram = 22,
    Mailbox3 = 5,
    Mailbox2 = 4,
    Mailbox1 = 3,
    Mailbox0 = 2,
    Mdma = 1,
    Qfc = 0,
}
/// This structure is only available in `std` environment due to the very different
/// way in which interrupts are handled between the two environments.
pub struct KpcAoInt {
    pub kpc: CSR<u32>,
    pub ao: CSR<u32>,
    pub irq: CSR<u32>,
    pub args: Vec<IrqNotification>,
    enable: u32,
}

impl KpcAoInt {
    pub fn new(handler: Option<fn(usize, *mut usize)>) -> Pin<Box<Self>> {
        let mem = xous::map_memory(
            xous::MemoryAddress::new(utra::dkpc::HW_DKPC_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("Couldn't map dkpc range");
        let irq = xous::map_memory(
            xous::MemoryAddress::new(utra::irqarray2::HW_IRQARRAY2_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("Couldn't map irq2 range");
        let ao = xous::map_memory(
            xous::MemoryAddress::new(utra::ao_sysctrl::HW_AO_SYSCTRL_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("Couldn't map ao_sysctrl range");

        let mut kpc_aoint = Box::pin(KpcAoInt {
            kpc: CSR::new(mem.as_ptr() as *mut u32),
            irq: CSR::new(irq.as_ptr() as *mut u32),
            ao: CSR::new(ao.as_ptr() as *mut u32),
            args: Vec::new(),
            enable: 0,
        });

        if let Some(handler) = handler {
            xous::claim_interrupt(
                utra::irqarray2::IRQARRAY2_IRQ,
                handler,
                Pin::as_mut(&mut kpc_aoint).get_mut() as *mut KpcAoInt as *mut usize,
            )
            .expect("couldn't claim ao/kpc handler interrupt");
        }
        kpc_aoint
    }

    /// Adds a notifier to the IRQ stack. Does not also enable the IRQ.
    pub fn add_irq_notifier(&mut self, notification: IrqNotification) { self.args.push(notification) }

    pub fn modify_irq_ena(&mut self, bit: u4, enable: bool) {
        if enable {
            self.enable |= 1 << bit.value() as u32;
        } else {
            self.enable &= !(1 << bit.value() as u32);
        }
        self.irq.wo(utra::irqarray2::EV_ENABLE, self.enable);
    }
}
