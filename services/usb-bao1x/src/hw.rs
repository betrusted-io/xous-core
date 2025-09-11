use std::borrow::BorrowMut;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::compiler_fence;
use std::sync::atomic::{AtomicPtr, Ordering};

use cramium_hal::usb::driver::*;
use cramium_hal::usb::utra::*;
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

/*
    1. interrupt enters
    2. poll() is called in a loop
        - this loop is the process_event_ring() "loop"
        - the result of poll() is immediately passed on to the Class() handler *before* the Event is dequeued
            - the read()/write() functions calls that would interact with the Transfer TRBs should all be called by now
        - (so, the events are handled one-by-one even though "poll" natively wants to aggregate all simultaneous events)
    3. only after the loop is done, do we execute the post-amble Event DeQueue pointer update
    4. EP allocation simply manages some bookkeeping that allows the EpAddress to be passed on to read()/write()
    call such that the handlers line up with the packet type. This virtually implements the function-pointer immediate
    decode & dispatch of handlers within the framework of the USB crate
    5. interrupt exits

    I think this succeeds where the previous version fails because:
        - the EventTRB is only dequeued after all the transfer pending data is handled - previously we would
        dequeue the EventTRB, then potentially take arbitrarily long (due to interruptability) to handle the
        TransferTRB, which could allow the TransferTRB to be overwritten because the existence of the EventTRB
        is more of what stalls the packet engine
        - The actual "set stall" handler thus might actually be nil, because the stall I believe is automatically
        originated within the USB hardware itself, as it knows that a packet is pending and will NAK the packets.
        The clearing of the stall is the only thing that might need an implementation, and that is equivalently
        the enqueue ZLP function but we need to revisit this on a case by case basis - it might make sense for
        the system to unstall right away based on the read of the TransferTRB instead of trying to split the
        unstall call to work with the usb-device stack.

    Another option would be to just figure out how to abuse the usb-device stack to simply generate
    a formatted device descriptor that we can feed into the state-machine based implementation provided
    by the vendor as a reference.

*/

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UsbIrqReq {
    FidoTx,
    KbdTx,
}

pub const CORIGINE_IRQ_MASK: u32 = 0x1;
pub const SW_IRQ_MASK: u32 = 0x2;

pub(crate) fn composite_handler(_irq_no: usize, arg: *mut usize) {
    let usb = unsafe { &mut *(arg as *mut CramiumUsb) };

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
                    let mut corigine_usb = usb.wrapper.core();
                    let mut event = {
                        if corigine_usb.udc_event.evt_dq_pt.load(Ordering::SeqCst).is_null() {
                            // break;
                            crate::println!("null pointer in process_event_ring");
                            break;
                        }
                        let event_ptr = corigine_usb.udc_event.evt_dq_pt.load(Ordering::SeqCst) as usize;
                        unsafe { (event_ptr as *mut EventTrbS).as_mut().expect("couldn't deref pointer") }
                    };
                    if event.dw3.cycle_bit() != corigine_usb.udc_event.ccs {
                        break;
                    }

                    // leaves a side-effect result of the CrgEvent inside the corigine_usb object
                    #[cfg(feature = "verbose-debug")]
                    crate::println!("handle inner");
                    if cramium_hal::usb::driver::handle_event_inner(&mut corigine_usb, &mut event) {
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
                    let mut buf = [0u8; 512];
                    if let Ok(count) = serial.read(&mut buf) {
                        crate::println!("serial read: {:x?}", &buf[..count]);
                        if buf[0] == 'a' as u32 as u8 {
                            // echo back
                            // let wr_res = serial.write(b"boo!");
                            let wr_res = serial.write(&buf[..count]);
                            crate::println!("serial write: {:?}", wr_res);
                        }
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
                        crate::println!(
                            " evt_last_trb {:x}",
                            hw_lock.udc_event.evt_seg0_last_trb.load(Ordering::SeqCst) as usize
                        );
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

#[repr(align(32))]
pub struct CramiumUsb<'a> {
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
}

impl<'a> CramiumUsb<'a> {
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

        // this buffer needs to be at least as big as the max packet size, ideally 2x as much. Set it to
        // 1k.
        let rx_buf = [0u8; 1024];
        let tx_buf = [0u8; 1024];
        let serial_port = SerialPort::new_with_store(&usb_alloc, rx_buf, tx_buf);

        let device = UsbDeviceBuilder::new(&usb_alloc, UsbVidPid(0x1209, 0x3613))
            .manufacturer("Kosagi")
            .product("Precursor")
            .serial_number(&serial_number)
            // this is *required* by the corigine stack
            .max_packet_size_0(64)
            .device_class(0)
            .build();

        CramiumUsb {
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
        }
    }

    pub fn init(&mut self) {
        // this has to be done in `main` because we're passing a pointer to the Box'd structure, which
        // the IRQ handler can freely and safely manipulate
        xous::claim_interrupt(
            utra::irqarray1::IRQARRAY1_IRQ,
            composite_handler,
            self as *mut CramiumUsb as *mut usize,
        )
        .expect("couldn't claim irq");
        log::debug!("claimed IRQ with state at {:x}", self as *mut CramiumUsb as usize);

        // enable both the corigine core IRQ and the software IRQ bit
        // software IRQ is used to initiate send/receive from software to the interrupt context
        self.irq_csr.wo(utra::irqarray1::EV_SOFT, 0);
        self.irq_csr.wo(utra::irqarray1::EV_EDGE_TRIGGERED, 0);
        self.irq_csr.wo(utra::irqarray1::EV_POLARITY, 0);

        self.wrapper.hw.lock().unwrap().init();
        self.wrapper.hw.lock().unwrap().start();
        self.wrapper.hw.lock().unwrap().update_current_speed();

        // irq must me enabled without dependency on the hw lock
        self.irq_csr.wo(utra::irqarray1::EV_PENDING, 0xFFFF_FFFF);
        self.irq_csr.wo(utra::irqarray1::EV_ENABLE, CORIGINE_IRQ_MASK | SW_IRQ_MASK);
    }

    pub fn sw_irq(&mut self, request_type: UsbIrqReq) {
        self.irq_req = Some(request_type);
        self.irq_csr.wfo(utra::irqarray1::EV_SOFT_TRIGGER, SW_IRQ_MASK);
    }

    /// Process an unplug event
    pub fn unplug(&mut self) {
        // disable all interrupts so we can safely go through initialization routines
        self.irq_csr.wo(utra::irqarray1::EV_ENABLE, 0);

        self.wrapper.hw.lock().unwrap().reset();
        self.wrapper.hw.lock().unwrap().init();
        self.wrapper.hw.lock().unwrap().start();
        self.wrapper.hw.lock().unwrap().update_current_speed();

        // reset all shared data structures
        self.device.force_reset().ok();
        self.fido_tx_queue = RefCell::new(VecDeque::new());
        self.kbd_tx_queue = RefCell::new(VecDeque::new());
        self.irq_req = None;
        self.wrapper.event = None;
        self.wrapper.address_is_set.store(false, Ordering::SeqCst);
        self.wrapper.ep_out_ready = (0..cramium_hal::usb::driver::CRG_EP_NUM + 1)
            .map(|_| core::sync::atomic::AtomicBool::new(false))
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
