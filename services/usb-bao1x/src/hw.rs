use std::borrow::BorrowMut;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, compiler_fence};
use std::sync::atomic::{AtomicPtr, Ordering};

use bao1x_hal::usb::driver::*;
use bao1x_hal::usb::utra::*;
use num_traits::*;
use usb_device::class_prelude::*;
use usb_device::device::UsbDevice;
use usb_device::prelude::*;
use usbd_serial::SerialPort;
use utralib::{AtomicCsr, utra};
use xous::Message;
use xous_usb_hid::device::DeviceClass;
use xous_usb_hid::device::fido::RawFido;
use xous_usb_hid::device::fido::RawFidoConfig;
use xous_usb_hid::device::fido::RawFidoReport;
use xous_usb_hid::device::keyboard::{NKROBootKeyboard, NKROBootKeyboardConfig};
use xous_usb_hid::page::Keyboard;
use xous_usb_hid::prelude::UsbHidClass;
use xous_usb_hid::prelude::*;

use crate::Opcode;

/// Maximum packet size for serial - tied to the speed of the port (HS)
pub const SERIAL_MAX_PACKET_SIZE: usize = 512;

#[repr(align(32))]
pub struct Bao1xUsb<'a> {
    pub conn: xous::CID,
    pub csr: AtomicCsr<u32>,
    pub irq_csr: AtomicCsr<u32>,
    pub fido_tx_queue: RefCell<VecDeque<RawFidoReport>>,
    pub kbd_tx_queue: RefCell<VecDeque<Keyboard>>,
    pub irq_req: Option<UsbIrqReq>,
    pub wrapper: CorigineWrapper,
    pub device: UsbDevice<'a, CorigineWrapper>,
    pub class: UsbHidClass<
        'a,
        CorigineWrapper,
        frunk_core::hlist::HCons<
            RawFido<'a, CorigineWrapper>,
            frunk_core::hlist::HCons<NKROBootKeyboard<'a, CorigineWrapper>, frunk_core::hlist::HNil>,
        >,
    >,
    // storage for hid_packets to expatriate from the interrupt handler
    pub hid_packet: Option<[u8; 64]>,
    pub serial_port: SerialPort<'a, CorigineWrapper, [u8; 1024], [u8; 1024]>,
    // holds one HS packet - must be statically allocated in IRQ handler. Valid length is
    // passed as part of the interrupt recovery message.
    pub serial_rx: [u8; SERIAL_MAX_PACKET_SIZE],
    // an error reporter for the double lock condition, which we need to figure out how to handle still.
    // used for debugging, the idea is to query this in userspace to try and pick up the double-lock problem
    // from the interrupt handler.
    pub double_lock: AtomicBool,
}

impl<'a> Bao1xUsb<'a> {
    pub fn new(
        csr: AtomicCsr<u32>,
        irq_csr: AtomicCsr<u32>,
        cid: xous::CID,
        cw: CorigineWrapper,
        usb_alloc: &'a UsbBusAllocator<CorigineWrapper>,
        serial_number: &'a String,
    ) -> Self {
        let class = UsbHidClassBuilder::new()
            .add_device(NKROBootKeyboardConfig::default())
            .add_device(RawFidoConfig::default())
            .build(usb_alloc);

        let rx_buf = [0u8; SERIAL_MAX_PACKET_SIZE * 2];
        let tx_buf = [0u8; SERIAL_MAX_PACKET_SIZE * 2];
        let serial_port = SerialPort::new_with_store(&usb_alloc, rx_buf, tx_buf);
        // HACK ALERT: due to a shortcoming in the usb-device implementation, inside the interrupt handler we
        // have to catch and parse SETUP-OUT sequences. Basically the driver assumes that the OUT endpoint is
        // always configured to trigger, but in our stack every time we have an OUT on EP0, we have to
        // set it up with the correct length. See the "TrbType::SetupPkt" arm of handle_event_inner()
        // for more details.
        //
        // In particular, Mass Storage has to handle a similar situation, so if adding a mass storage
        // interface, this hack has to be dealt with (and btw, there are not enough endpoints availbale
        // to concurrently add that in - you have to kick out one of the interfaces above to add mass
        // storage!)

        let device = UsbDeviceBuilder::new(&usb_alloc, UsbVidPid(0x1209, 0x3613))
            .manufacturer("Baochip")
            .product("Baosec")
            .serial_number(&serial_number)
            // this is *required* by the corigine stack
            .max_packet_size_0(64)
            .composite_with_iads()
            .build();

        Bao1xUsb {
            conn: cid,
            // safety: we created iframrange to have the exact same P&V mappings
            wrapper: cw,
            device,
            class,
            csr,
            irq_csr,
            fido_tx_queue: RefCell::new(VecDeque::new()),
            kbd_tx_queue: RefCell::new(VecDeque::new()),
            irq_req: None,
            hid_packet: None,
            serial_port,
            serial_rx: [0u8; SERIAL_MAX_PACKET_SIZE],
            double_lock: AtomicBool::new(false),
        }
    }

    #[allow(dead_code)]
    /// Used only when debugging the double lock problem
    pub fn double_lock_detected(&self) -> bool { self.double_lock.swap(false, Ordering::SeqCst) }

    pub fn init(&mut self) {
        // this has to be done in `main` because we're passing a pointer to the Box'd structure, which
        // the IRQ handler can freely and safely manipulate
        xous::claim_interrupt(
            utra::irqarray1::IRQARRAY1_IRQ,
            composite_handler,
            self as *mut Bao1xUsb as *mut usize,
        )
        .expect("couldn't claim irq");
        log::debug!("claimed IRQ with state at {:x}", self as *mut Bao1xUsb as usize);

        // enable both the corigine core IRQ and the software IRQ bit
        // software IRQ is used to initiate send/receive from software to the interrupt context
        self.irq_csr.wo(utra::irqarray1::EV_SOFT, 0);
        self.irq_csr.wo(utra::irqarray1::EV_EDGE_TRIGGERED, 0);
        self.irq_csr.wo(utra::irqarray1::EV_POLARITY, 0);

        self.wrapper.core().init();
        self.wrapper.core().start();
        self.wrapper.core().update_current_speed();

        // irq must me enabled without dependency on the hw lock
        self.irq_csr.wo(utra::irqarray1::EV_PENDING, 0xFFFF_FFFF);
        self.irq_csr.wo(utra::irqarray1::EV_ENABLE, CORIGINE_IRQ_MASK | SW_IRQ_MASK);
    }

    pub fn sw_irq(&mut self, request_type: UsbIrqReq) {
        self.irq_req = Some(request_type);
        self.irq_csr.wfo(utra::irqarray1::EV_SOFT_TRIGGER, SW_IRQ_MASK);
    }

    /// Process an unplug event - only valid on baosec, because dabao doesn't have a battery and unplugging
    /// it would power it down.
    #[cfg(feature = "board-baosec")]
    pub fn unplug(&mut self) {
        // disable all interrupts so we can safely go through initialization routines
        self.irq_csr.wo(utra::irqarray1::EV_ENABLE, 0);

        self.wrapper.core().reset();
        self.wrapper.core().init();
        self.wrapper.core().start();
        self.wrapper.core().update_current_speed();

        // reset all shared data structures
        self.device.force_reset().ok();
        self.fido_tx_queue = RefCell::new(VecDeque::new());
        self.kbd_tx_queue = RefCell::new(VecDeque::new());
        self.irq_req = None;
        self.wrapper.event = None;
        self.wrapper.address_is_set.store(false, Ordering::SeqCst);
        self.wrapper.ep_out_ready = (0..bao1x_hal::usb::driver::CRG_EP_NUM + 1)
            .map(|_| std::sync::Arc::new(core::sync::atomic::AtomicBool::new(false)))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        for ready in self.wrapper.ep_out_ready.iter() {
            ready.store(false, Ordering::SeqCst);
        }

        // re-enable IRQs
        self.irq_csr.wo(utra::irqarray1::EV_PENDING, 0xFFFF_FFFF);
        self.irq_csr.wo(utra::irqarray1::EV_ENABLE, CORIGINE_IRQ_MASK | SW_IRQ_MASK);
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UsbIrqReq {
    FidoTx,
    KbdTx,
}

pub const CORIGINE_IRQ_MASK: u32 = 0x1;
pub const SW_IRQ_MASK: u32 = 0x2;

pub(crate) fn composite_handler(_irq_no: usize, arg: *mut usize) {
    let usb = unsafe { &mut *(arg as *mut Bao1xUsb) };

    // immediately clear the interrupt and re-enable it so we can catch an interrupt
    // that is generated while we are handling the interrupt.
    let pending = usb.irq_csr.r(utra::irqarray1::EV_PENDING);
    #[cfg(feature = "verbose-debug")]
    crate::println!("pending: {:x}, status: {:x}", pending, usb.irq_csr.r(utra::irqarray1::EV_STATUS),);
    // clear pending
    usb.irq_csr.wo(utra::irqarray1::EV_PENDING, 0xffff_ffff);
    // re-enable interrupts
    usb.irq_csr.wo(utra::irqarray1::EV_ENABLE, CORIGINE_IRQ_MASK | SW_IRQ_MASK);

    if (pending & CORIGINE_IRQ_MASK) != 0 {
        let status = usb.csr.r(USBSTS);
        #[cfg(feature = "verbose-debug")]
        crate::println!("crg status: {:x}", status);
        if (status & usb.csr.ms(USBSTS_SYSTEM_ERR, 1)) != 0 {
            crate::println!("System error");
            usb.csr.wfo(USBSTS_SYSTEM_ERR, 1);
            crate::println!("USBCMD: {:x}", usb.csr.r(USBCMD));
        } else if (status & usb.csr.ms(USBSTS_EINT, 1)) != 0 {
            usb.csr.wfo(USBSTS_EINT, 1);
            // clear IP
            usb.csr.rmwf(IMAN_IP, 1);

            loop {
                {
                    // #[cfg(feature = "verbose-debug")]
                    // crate::println!("getting event");
                    // scoping on the hardware lock to manipulate pointer states
                    let mut corigine_usb = match usb.wrapper.hw.try_lock() {
                        Ok(lock) => lock,
                        _ => {
                            crate::println!("double lock - this case is actually broken, stack will crash");
                            usb.double_lock.store(true, Ordering::SeqCst);
                            return;
                        }
                    };
                    let mut event = {
                        if corigine_usb.udc_event.evt_dq_pt.load(Ordering::SeqCst).is_null() {
                            // break;
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

                    // leaves a side-effect result of the CrgEvent inside the corigine_usb object
                    #[cfg(feature = "verbose-debug")]
                    crate::println!("handle inner");
                    if bao1x_hal::usb::driver::handle_event_inner(&mut corigine_usb, &mut event) {
                        crate::println!("~~~~~got reset~~~~");
                        // reset the ready state
                        for ready in usb.wrapper.ep_out_ready.iter() {
                            ready.store(false, Ordering::SeqCst);
                        }
                        usb.wrapper.address_is_set.store(false, Ordering::SeqCst);
                    }
                }

                let device = usb.device.borrow_mut();
                let class = usb.class.borrow_mut();
                let serial = usb.serial_port.borrow_mut();
                if device.poll(&mut [class, serial as &mut dyn UsbClass<_>]) {
                    if let Ok(count) = serial.read(&mut usb.serial_rx) {
                        xous::try_send_message(
                            usb.conn,
                            Message::new_scalar(Opcode::IrqSerialRx.to_usize().unwrap(), count, 0, 0, 0),
                        )
                        .ok();
                    }
                    match class.device::<NKROBootKeyboard<_>, _>().read_report() {
                        Ok(l) => {
                            // for now all we do is just print this, we don't
                            // actually store the data or pass it on to userspace
                            crate::println!("keyboard LEDs: {:?}", l);
                        }
                        Err(e) => match e {
                            UsbError::WouldBlock => {}
                            _ => crate::println!("KEYB ERR: {:?}", e),
                        },
                    };
                    // It's illegal to allocate a Buffer in an interrupt context (because the operation is
                    // fallible), so we use a pre-allocated storage (usb.hid_packet) to
                    // pass the data to userspace, which is then notified with `IrqFidoRx`
                    // to read the stashed data
                    match class.device::<RawFido<'_, _>, _>().read_report() {
                        Ok(u2f_report) => {
                            // crate::println!("got report {:x?}", u2f_report);
                            usb.hid_packet = Some(u2f_report.packet);
                            xous::try_send_message(
                                usb.conn,
                                Message::new_scalar(Opcode::IrqFidoRx.to_usize().unwrap(), 0, 0, 0, 0),
                            )
                            .ok();
                        }
                        Err(e) => match e {
                            UsbError::WouldBlock => {}
                            _ => crate::println!("U2F ERR: {:?}", e),
                        },
                    }
                }
                {
                    // scoping on the hardware lock to manipulate pointer states
                    let mut hw_lock = usb.wrapper.core();
                    if hw_lock.udc_event.evt_dq_pt.load(Ordering::SeqCst)
                        == hw_lock.udc_event.evt_seg0_last_trb.load(Ordering::SeqCst)
                    {
                        hw_lock.udc_event.ccs = !hw_lock.udc_event.ccs;
                        // does this...go to null to end the transfer??
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
            // update dequeue pointer
            usb.csr.wo(ERDPHI, 0);
            usb.csr.wo(
                ERDPLO,
                (usb.wrapper.core().udc_event.evt_dq_pt.load(Ordering::SeqCst) as u32 & 0xFFFF_FFF0)
                    | CRG_UDC_ERDPLO_EHB,
            );
            compiler_fence(Ordering::SeqCst);
        }
        if usb.csr.rf(IMAN_IE) != 0 {
            usb.csr.wo(IMAN, usb.csr.ms(IMAN_IE, 1) | usb.csr.ms(IMAN_IP, 1));
        }
    } else if (pending & SW_IRQ_MASK) != 0 {
        let composite = usb.class.borrow_mut();
        match usb.irq_req.take() {
            Some(UsbIrqReq::FidoTx) => {
                let u2f = composite.device::<RawFido<'_, _>, _>();
                // you know, I'm not 100% sure we *can* write multiple reports without taking
                // another interrupt. But, in practice, we /should/ only ever get one event
                // and the VecDequeue structure lays the ground work to extend this to something
                // more flexible if we decide we need to handle multiple Tx queued events in a single
                // IRQ trigger.
                while let Some(u2f_msg) = usb.fido_tx_queue.borrow_mut().pop_front() {
                    u2f.write_report(&u2f_msg).ok();
                }
            }
            Some(UsbIrqReq::KbdTx) => {
                let keyboard = composite.device::<NKROBootKeyboard<'_, _>, _>();
                usb.kbd_tx_queue.borrow_mut().make_contiguous();
                let kbd_events = usb.kbd_tx_queue.borrow().as_slices().0.to_vec();
                keyboard.write_report(kbd_events).ok();
                usb.kbd_tx_queue.borrow_mut().clear();
                keyboard.tick().ok();
            }
            None => (),
        }
    }
}
