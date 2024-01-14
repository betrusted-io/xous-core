use core::convert::TryFrom;

use atsama5d27::{
    aic::{Aic, InterruptEntry, IrqPendingInfo, SourceKind},
    pmc::PeripheralId,
};
use utralib::{HW_AIC_BASE, HW_SAIC_BASE};
use xous_kernel::{arch::irq::IrqNumber, MemoryFlags, MemoryType, PID};

use crate::arch::irq::_irq_handler_rust;
use crate::mem::MemoryManager;

pub const AIC_KERNEL_ADDR: usize = 0xffca_0000;
pub static mut AIC_KERNEL: Option<AicKernel> = None;

pub const SAIC_KERNEL_ADDR: usize = 0xffcb_0000; // Secure version of AIC
pub static mut SAIC_KERNEL: Option<AicKernel> = None;

unsafe extern "C" fn spurious_interrupt_handler() {
    core::arch::asm!("bkpt");
}

pub struct AicKernel {
    base_addr: usize,
    pub inner: Option<Aic>,
}

impl AicKernel {
    pub fn new(addr: usize) -> AicKernel { AicKernel { base_addr: addr, inner: None } }

    pub fn init(&mut self) {
        let mut aic = Aic::with_alt_base_addr(self.base_addr as u32);
        aic.init();
        aic.set_spurious_handler_fn_ptr(spurious_interrupt_handler as unsafe extern "C" fn() as usize);
        self.inner = Some(aic);
    }

    pub fn enable_interrupt(&mut self, kind: SourceKind, id: PeripheralId) {
        if let Some(aic) = &mut self.inner {
            let handler = InterruptEntry {
                peripheral_id: id,
                vector_fn_ptr: _irq_handler_rust as unsafe extern "C" fn() as usize,
                kind,
                priority: 0,
            };
            aic.set_interrupt_handler(handler);
        } else {
            panic!("AIC is not initialized")
        }
    }

    pub fn disable_interrupt(&mut self, id: PeripheralId) {
        if let Some(aic) = &mut self.inner {
            aic.set_interrupt_enabled(id, false);
        } else {
            panic!("AIC is not initialized")
        }
    }

    pub fn get_pending_irqs(&self) -> IrqPendingInfo {
        if let Some(aic) = &self.inner { aic.get_pending_irqs() } else { panic!("AIC is not initialized") }
    }

    pub fn interrupt_completed(&mut self) {
        if let Some(aic) = &mut self.inner {
            aic.interrupt_completed()
        } else {
            panic!("AIC is not initialized")
        }
    }
}

pub fn init() {
    MemoryManager::with_mut(|memory_manager| {
        memory_manager
            .map_range(
                HW_AIC_BASE as *mut u8,
                (AIC_KERNEL_ADDR & !4095) as *mut u8,
                0x4000, // 16K
                PID::new(1).unwrap(),
                MemoryFlags::R | MemoryFlags::W | MemoryFlags::DEV,
                MemoryType::Default,
            )
            .expect("unable to map PMC_KERNEL")
    });
    MemoryManager::with_mut(|memory_manager| {
        memory_manager
            .map_range(
                HW_SAIC_BASE as *mut u8,
                (SAIC_KERNEL_ADDR & !4095) as *mut u8,
                0x4000, // 16K
                PID::new(1).unwrap(),
                MemoryFlags::R | MemoryFlags::W | MemoryFlags::DEV,
                MemoryType::Default,
            )
            .expect("unable to map PMC_KERNEL")
    });

    let mut aic_kernel = AicKernel::new(AIC_KERNEL_ADDR);
    aic_kernel.init();

    unsafe {
        AIC_KERNEL = Some(aic_kernel);
    }

    let mut saic_kernel = AicKernel::new(SAIC_KERNEL_ADDR);
    saic_kernel.init();

    unsafe {
        SAIC_KERNEL = Some(saic_kernel);
    }
}

fn peripheral_id_to_irq_no(id: PeripheralId) -> Option<IrqNumber> {
    klog!("Pending IRQ: {:?}", id);

    match id {
        PeripheralId::Pit => Some(IrqNumber::PeriodicIntervalTimer),

        PeripheralId::Uart0 => Some(IrqNumber::Uart0),
        PeripheralId::Uart1 => Some(IrqNumber::Uart1),
        PeripheralId::Uart2 => Some(IrqNumber::Uart2),
        PeripheralId::Uart3 => Some(IrqNumber::Uart3),
        PeripheralId::Uart4 => Some(IrqNumber::Uart4),

        PeripheralId::Pioa => Some(IrqNumber::Pioa),
        PeripheralId::Piob => Some(IrqNumber::Piob),
        PeripheralId::Pioc => Some(IrqNumber::Pioc),
        PeripheralId::Piod => Some(IrqNumber::Piod),

        PeripheralId::Isi => Some(IrqNumber::Isi),
        PeripheralId::Lcdc => Some(IrqNumber::Lcdc),

        PeripheralId::Uhphs => Some(IrqNumber::Uhphs),
        PeripheralId::Udphs => Some(IrqNumber::Udphs),

        PeripheralId::Tc0 => Some(IrqNumber::Tc0),
        PeripheralId::Tc1 => Some(IrqNumber::Tc1),

        _ => return None,
    }
}

pub fn set_irq_enabled(irq_no: IrqNumber, enabled: bool) {
    let (sama5d2_irq_no, source_kind) = match irq_no {
        IrqNumber::PeriodicIntervalTimer => (PeripheralId::Pit, SourceKind::LevelSensitive),

        IrqNumber::Uart0 => (PeripheralId::Uart0, SourceKind::LevelSensitive),
        IrqNumber::Uart1 => (PeripheralId::Uart1, SourceKind::LevelSensitive),
        IrqNumber::Uart2 => (PeripheralId::Uart2, SourceKind::LevelSensitive),
        IrqNumber::Uart3 => (PeripheralId::Uart3, SourceKind::LevelSensitive),
        IrqNumber::Uart4 => (PeripheralId::Uart3, SourceKind::LevelSensitive),

        IrqNumber::Pioa => (PeripheralId::Pioa, SourceKind::LevelSensitive),
        IrqNumber::Piob => (PeripheralId::Piob, SourceKind::LevelSensitive),
        IrqNumber::Pioc => (PeripheralId::Pioc, SourceKind::LevelSensitive),
        IrqNumber::Piod => (PeripheralId::Piod, SourceKind::LevelSensitive),

        IrqNumber::Isi => (PeripheralId::Isi, SourceKind::LevelSensitive),
        IrqNumber::Lcdc => (PeripheralId::Lcdc, SourceKind::LevelSensitive),

        IrqNumber::Uhphs => (PeripheralId::Uhphs, SourceKind::LevelSensitive),
        IrqNumber::Udphs => (PeripheralId::Udphs, SourceKind::LevelSensitive),

        IrqNumber::Tc0 => (PeripheralId::Tc0, SourceKind::LevelSensitive),
        IrqNumber::Tc1 => (PeripheralId::Tc1, SourceKind::LevelSensitive),
    };

    unsafe {
        let aic = AIC_KERNEL.as_mut().expect("AIC is not initialized");

        if enabled {
            aic.enable_interrupt(source_kind, sama5d2_irq_no);
        } else {
            aic.disable_interrupt(sama5d2_irq_no);
        }
    }
}

pub fn get_pending_irqs() -> usize {
    let pending_irqs = unsafe {
        let aic = AIC_KERNEL.as_mut().expect("AIC is not initialized");
        aic.get_pending_irqs()
    };

    let mut pending_mask = 0;
    for i in 0..32_u32 {
        if pending_irqs.irqs_0_31 & (1 << i) != 0 {
            if let Ok(id) = PeripheralId::try_from(i as u8) {
                if let Some(irq_no) = peripheral_id_to_irq_no(id) {
                    pending_mask |= 1 << irq_no as usize;
                }
            }
        }
    }
    for i in 32..63_u32 {
        if pending_irqs.irqs_32_63 & (1 << (i - 32)) != 0 {
            if let Ok(id) = PeripheralId::try_from(i as u8) {
                if let Some(irq_no) = peripheral_id_to_irq_no(id) {
                    pending_mask |= 1 << irq_no as usize;
                }
            }
        }
    }
    for i in 64..95_u32 {
        if pending_irqs.irqs_64_95 & (1 << (i - 64)) != 0 {
            if let Ok(id) = PeripheralId::try_from(i as u8) {
                if let Some(irq_no) = peripheral_id_to_irq_no(id) {
                    pending_mask |= 1 << irq_no as usize;
                }
            }
        }
    }
    for i in 96..127_u32 {
        if pending_irqs.irqs_96_127 & (1 << (i - 96)) != 0 {
            if let Ok(id) = PeripheralId::try_from(i as u8) {
                if let Some(irq_no) = peripheral_id_to_irq_no(id) {
                    pending_mask |= 1 << irq_no as usize;
                }
            }
        }
    }

    pending_mask
}

pub fn acknowledge_irq() {
    unsafe {
        let aic = AIC_KERNEL.as_mut().expect("AIC is not initialized");
        aic.interrupt_completed();
    };
}
