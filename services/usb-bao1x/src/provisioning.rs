// SPDX-License-Identifier: GPL-3.0-only
//
//! First-boot USB CDC PIN staging (`ccid-openpgp`). Uses raw pointers for the IRQ handler because
//! [`ProvisioningClass`] borrows the [`UsbBusAllocator`] while the ISR must poll the same device.

use std::sync::atomic::{compiler_fence, AtomicPtr, Ordering};

use bao1x_hal::rram::Reram;
use bao1x_hal::usb::driver::{
    handle_event_inner, CorigineWrapper, EventTrbS, CRG_UDC_ERDPLO_EHB, ERDPHI, ERDPLO, IMAN, IMAN_IE,
    IMAN_IP, USBCMD, USBSTS,
};
use bao1x_hal::usb::utra::*;
use galdr_core::HalError;
use usb_device::class::UsbClass;
use usb_device::class_prelude::*;
use usb_device::device::UsbDevice;
use usb_device::prelude::*;
use usb_personality::ccid::{
    USB_PID_GALDRALAG_TOKEN, USB_STRING_MANUFACTURER, USB_VID_GALDRALAG,
};
use usb_personality::provisioning::{ProvisioningClass, ProvisioningCommit};
use utralib::{AtomicCsr, utra};

use baochip_openpgp::write_provisioning_pins;

use crate::hw::{CORIGINE_IRQ_MASK, SW_IRQ_MASK};

pub(crate) struct PinProvisionCommit {
    pub rram: std::rc::Rc<std::cell::RefCell<Reram>>,
}

impl ProvisioningCommit for PinProvisionCommit {
    fn commit_pins(&mut self, user_pin: &[u8], admin_pin: &[u8]) -> Result<(), HalError> {
        write_provisioning_pins(&self.rram, user_pin, admin_pin)
    }
}

pub(crate) struct ProvisioningIrqCtx {
    pub csr: AtomicCsr<u32>,
    pub irq_csr: AtomicCsr<u32>,
    pub wrapper: CorigineWrapper,
    pub device: *mut UsbDevice<'static, CorigineWrapper>,
    pub class: *mut ProvisioningClass<'static, CorigineWrapper, PinProvisionCommit>,
}

fn noop_irq(_irq_no: usize, _arg: *mut usize) {}

/// Enumerate the CDC provisioning personality until [`ProvisioningClass::commit_succeeded`] (host `COMMIT`).
///
/// Replaces the IRQ handler temporarily; [`crate::hw::Bao1xUsb::init`] claims it again for the composite stack.
///
/// # Safety
///
/// `ctx.device` and `ctx.class` must point to live `UsbDevice` / `ProvisioningClass` storage for the entire
/// duration this handler is registered. Interrupts are disabled before those objects are dropped.
pub(crate) fn run_first_boot_pin_provisioning(
    usb_csr: &AtomicCsr<u32>,
    irq_csr: &AtomicCsr<u32>,
    cw: &CorigineWrapper,
    serial_number: &str,
    rram: &std::rc::Rc<std::cell::RefCell<Reram>>,
) -> Result<(), HalError> {
    let usb_alloc = UsbBusAllocator::new(cw.clone());
    let mut commit = Box::new(PinProvisionCommit { rram: rram.clone() });
    let mut class = ProvisioningClass::new(&usb_alloc, commit.as_mut());
    let mut device = UsbDeviceBuilder::new(&usb_alloc, UsbVidPid(USB_VID_GALDRALAG, USB_PID_GALDRALAG_TOKEN))
        .manufacturer(USB_STRING_MANUFACTURER)
        .product("Galdralag Provisioning")
        .serial_number(serial_number)
        .max_packet_size_0(64)
        .composite_with_iads()
        .build();

    cw.core().init();
    cw.core().start();
    cw.core().update_current_speed();

    irq_csr.wo(utra::irqarray1::EV_SOFT, 0);
    irq_csr.wo(utra::irqarray1::EV_EDGE_TRIGGERED, 0);
    irq_csr.wo(utra::irqarray1::EV_POLARITY, 0);
    irq_csr.wo(utra::irqarray1::EV_PENDING, 0xFFFF_FFFF);
    irq_csr.wo(utra::irqarray1::EV_ENABLE, CORIGINE_IRQ_MASK | SW_IRQ_MASK);

    let mut ctx = ProvisioningIrqCtx {
        csr: usb_csr.clone(),
        irq_csr: irq_csr.clone(),
        wrapper: cw.clone(),
        device: (&mut device as *mut UsbDevice<'_, CorigineWrapper>).cast::<UsbDevice<'static, CorigineWrapper>>(),
        class: (&mut class as *mut ProvisioningClass<'_, CorigineWrapper, PinProvisionCommit>)
            .cast::<ProvisioningClass<'static, CorigineWrapper, PinProvisionCommit>>(),
    };

    let ctx_ptr = (&mut ctx as *mut ProvisioningIrqCtx) as *mut usize;
    xous::claim_interrupt(utra::irqarray1::IRQARRAY1_IRQ, provisioning_irq_handler, ctx_ptr)
        .map_err(|_| HalError::Bus)?;

    while unsafe { !(*ctx.class).commit_succeeded() } {
        xous::yield_slice();
    }

    irq_csr.wo(utra::irqarray1::EV_ENABLE, 0);
    device.force_reset().ok();
    let _ = xous::claim_interrupt(utra::irqarray1::IRQARRAY1_IRQ, noop_irq, core::ptr::null_mut());

    Ok(())
}

pub(crate) fn provisioning_irq_handler(_irq_no: usize, arg: *mut usize) {
    let ctx = unsafe { &mut *(arg as *mut ProvisioningIrqCtx) };

    let pending = ctx.irq_csr.r(utra::irqarray1::EV_PENDING);
    ctx.irq_csr.wo(utra::irqarray1::EV_PENDING, 0xffff_ffff);
    ctx.irq_csr.wo(utra::irqarray1::EV_ENABLE, CORIGINE_IRQ_MASK | SW_IRQ_MASK);

    if (pending & CORIGINE_IRQ_MASK) != 0 {
        let status = ctx.csr.r(USBSTS);
        if (status & ctx.csr.ms(USBSTS_SYSTEM_ERR, 1)) != 0 {
            crate::println!("System error");
            ctx.csr.wfo(USBSTS_SYSTEM_ERR, 1);
            crate::println!("USBCMD: {:x}", ctx.csr.r(USBCMD));
        } else if (status & ctx.csr.ms(USBSTS_EINT, 1)) != 0 {
            ctx.csr.wfo(USBSTS_EINT, 1);
            ctx.csr.rmwf(IMAN_IP, 1);

            loop {
                {
                    let mut corigine_usb = match ctx.wrapper.hw.try_lock() {
                        Ok(lock) => lock,
                        _ => {
                            crate::println!("double lock - provisioning IRQ");
                            return;
                        }
                    };
                    let mut event = {
                        if corigine_usb.udc_event.evt_dq_pt.load(Ordering::SeqCst).is_null() {
                            crate::println!("null pointer in process_event_ring");
                            break;
                        }
                        let event_ptr = corigine_usb.udc_event.evt_dq_pt.load(Ordering::SeqCst) as usize;
                        match unsafe { (event_ptr as *mut EventTrbS).as_mut() } {
                            Some(ptr) => ptr,
                            None => {
                                break;
                            }
                        }
                    };
                    if event.dw3.cycle_bit() != corigine_usb.udc_event.ccs {
                        break;
                    }

                    if handle_event_inner(&mut corigine_usb, &mut event) {
                        crate::println!("~~~~~got reset~~~~");
                        for ready in ctx.wrapper.ep_out_ready.iter() {
                            ready.store(false, Ordering::SeqCst);
                        }
                        ctx.wrapper.address_is_set.store(false, Ordering::SeqCst);
                        unsafe {
                            (*ctx.class).reset();
                        }
                    }
                }

                unsafe {
                    let device = &mut *ctx.device;
                    let class = &mut *ctx.class;
                    let _ = device.poll(&mut [class as &mut dyn UsbClass<_>]);
                }

                {
                    let mut hw_lock = ctx.wrapper.core();
                    if hw_lock.udc_event.evt_dq_pt.load(Ordering::SeqCst)
                        == hw_lock.udc_event.evt_seg0_last_trb.load(Ordering::SeqCst)
                    {
                        hw_lock.udc_event.ccs = !hw_lock.udc_event.ccs;
                        hw_lock.udc_event.evt_dq_pt = AtomicPtr::new(
                            hw_lock.udc_event.event_ring.vaddr.load(Ordering::SeqCst) as *mut EventTrbS,
                        );
                    } else {
                        hw_lock.udc_event.evt_dq_pt = AtomicPtr::new(unsafe {
                            hw_lock.udc_event.evt_dq_pt.load(Ordering::SeqCst).add(1)
                        });
                    }
                }
            }
            ctx.csr.wo(ERDPHI, 0);
            ctx.csr.wo(
                ERDPLO,
                (ctx.wrapper.core().udc_event.evt_dq_pt.load(Ordering::SeqCst) as u32 & 0xFFFF_FFF0)
                    | CRG_UDC_ERDPLO_EHB,
            );
            compiler_fence(Ordering::SeqCst);
        }
        if ctx.csr.rf(IMAN_IE) != 0 {
            ctx.csr.wo(IMAN, ctx.csr.ms(IMAN_IE, 1) | ctx.csr.ms(IMAN_IP, 1));
        }
    }
}
