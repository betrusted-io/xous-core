use core::convert::TryFrom;
use core::sync::atomic::{AtomicPtr, Ordering};

use cramium_hal::usb::compat::AtomicCsr;
use cramium_hal::usb::driver::CorigineUsb;
use cramium_hal::usb::driver::*;
use cramium_hal::usb::utra::*;

use super::*;

pub(crate) static mut USB: Option<CorigineUsb> = None;

pub fn init_usb() {
    let mut usb = unsafe {
        cramium_hal::usb::driver::CorigineUsb::new(
            0,
            0,
            cramium_hal::board::CRG_UDC_MEMBASE,
            AtomicCsr::new(cramium_hal::usb::utra::CORIGINE_USB_BASE as *mut u32),
            AtomicCsr::new(utralib::utra::irqarray1::HW_IRQARRAY1_BASE as *mut u32),
        )
    };
    usb.assign_handler(handle_event);

    // initialize the "disk" area
    let disk = mass_storage::conjure_disk();
    disk.fill(0);
    disk[..MBR_TEMPLATE.len()].copy_from_slice(&MBR_TEMPLATE);
    //set block size
    disk[0xb..0xd].copy_from_slice(&SECTOR_SIZE.to_le_bytes());
    //set storage size
    disk[0x20..0x24].copy_from_slice(&(RAMDISK_LEN as u32).to_le_bytes());

    // install the interrupt handler
    // setup the stack & controller
    irq_setup();
    enable_irq(utralib::utra::irqarray1::IRQARRAY1_IRQ);
    // for testing
    enable_irq(utralib::utra::irqarray19::IRQARRAY19_IRQ);
    let mut irqarray19 = utralib::CSR::new(utralib::utra::irqarray19::HW_IRQARRAY19_BASE as *mut u32);
    irqarray19.wo(utralib::utra::irqarray19::EV_ENABLE, 0x80);

    unsafe {
        USB = Some(usb);
    }
}

pub unsafe fn test_usb() {
    if let Some(ref mut usb_ref) = USB {
        let usb = &mut *core::ptr::addr_of_mut!(*usb_ref);
        usb.reset();
        let mut poweron = 0;
        loop {
            usb.udc_handle_interrupt();
            if usb.pp() {
                poweron += 1; // .pp() is a sham. MPW has no way to tell if power is applied. This needs to be fixed for NTO.
            }
            delay(100);
            if poweron >= 4 {
                break;
            }
        }
        usb.reset();
        usb.init();
        usb.start();
        usb.update_current_speed();

        crate::println!("hw started...");
        /*
        let mut vbus_on = false;
        let mut vbus_on_count = 0;
        let mut in_u0 = false;
        loop {
            if vbus_on == false && vbus_on_count == 4 {
                crate::println!("vbus on");
                usb.init();
                usb.start();
                vbus_on = true;
                in_u0 = false;
            } else if usb.pp() == true && vbus_on == false {
                vbus_on_count += 1;
                delay(100);
            } else if usb.pp() == false && vbus_on == true {
                crate::println!("20230802 vbus off during while");
                usb.stop();
                usb.reset();
                vbus_on_count = 0;
                vbus_on = false;
                in_u0 = false;
            } else if in_u0 == true && vbus_on == true {
                crate::println!("USB stack started");
                break;
                // crate::println!("Would be uvc_bulk_thread()");
                // uvc_bulk_thread();
            } else if usb.ccs() == true && vbus_on == true {
                crate::println!("enter U0");
                in_u0 = true;
            }
        }
        */
        let mut i = 0;
        loop {
            // wait for interrupt handler to do something
            crate::println!("{}", i);
            i += 1;
            delay(10_000);
            // for testing interrupt handler
            let mut irqarray19 = utralib::CSR::new(utralib::utra::irqarray19::HW_IRQARRAY19_BASE as *mut u32);
            irqarray19.wfo(utralib::utra::irqarray19::EV_SOFT_TRIGGER, 0x80);
        }
    } else {
        crate::println!("USB core not allocated, skipping USB test");
    }
}

fn delay(quantum: usize) {
    use utralib::{CSR, utra};
    // abuse the d11ctime timer to create some time-out like thing
    let mut d11c = CSR::new(utra::d11ctime::HW_D11CTIME_BASE as *mut u32);
    d11c.wfo(utra::d11ctime::CONTROL_COUNT, 333_333); // 1.0ms per interval
    let mut polarity = d11c.rf(utra::d11ctime::HEARTBEAT_BEAT);
    for _ in 0..quantum {
        while polarity == d11c.rf(utra::d11ctime::HEARTBEAT_BEAT) {}
        polarity = d11c.rf(utra::d11ctime::HEARTBEAT_BEAT);
    }
    // we have to split this because we don't know where we caught the previous interval
    if quantum == 1 {
        while polarity == d11c.rf(utra::d11ctime::HEARTBEAT_BEAT) {}
    }
}

fn get_status_request(this: &mut CorigineUsb, request_type: u8, index: u16) {
    let ep0_buf = unsafe {
        core::slice::from_raw_parts_mut(
            this.ep0_buf.load(Ordering::SeqCst) as *mut u8,
            CRG_UDC_EP0_REQBUFSIZE,
        )
    };

    let mut status_val: u32 = 0;
    let recipient = request_type & 0x1f;
    let ep_num = index & 0x7f;
    let ep_dir = if index & 0x80 != 0 { USB_SEND } else { USB_RECV };

    match recipient {
        USB_RECIP_DEVICE => {
            status_val |= 0x1;
            ep0_buf[0] = status_val as u8;
            this.ep0_send(ep0_buf.as_ptr() as usize, 2, 0);
        }
        USB_RECIP_INTERFACE => {
            ep0_buf[0] = 0;
            ep0_buf[1] = 0;
            this.ep0_send(ep0_buf.as_ptr() as usize, 2, 0);
        }
        USB_RECIP_ENDPOINT => {
            if this.is_halted(ep_num as u8, ep_dir) {
                ep0_buf[0] = 1;
                this.ep0_send(ep0_buf.as_ptr() as usize, 2, 0);
            } else {
                ep0_buf[0] = 0;
                this.ep0_send(ep0_buf.as_ptr() as usize, 2, 0);
            }
        }
        _ => {
            this.ep_halt(0, USB_RECV);
        }
    }
}

fn handle_event(this: &mut CorigineUsb, event_trb: &mut EventTrbS) -> CrgEvent {
    // crate::println!("handle_event: {:x?}", event_trb);
    let pei = event_trb.get_endpoint_id();
    let ep_num = pei >> 1;
    let udc_ep = &mut this.udc_ep[pei as usize];
    let mut ret = CrgEvent::None;
    match event_trb.get_trb_type() {
        TrbType::EventPortStatusChange => {
            let portsc_val = this.csr.r(PORTSC);
            this.csr.wo(PORTSC, portsc_val);
            // this.print_status(portsc_val);

            let portsc = PortSc(portsc_val);
            crate::println!("{:?}", portsc);

            if portsc.prc() && !portsc.pr() {
                crate::println!("update_current_speed() - reset done");
                this.update_current_speed();
            }
            if portsc.csc() && portsc.ppc() && portsc.pp() && portsc.ccs() {
                crate::println!("update_current_speed() - cable connect");
                this.update_current_speed();
            }

            this.csr.rmwf(EVENTCONFIG_SETUP_ENABLE, 1);
        }
        TrbType::EventTransfer => {
            let comp_code =
                CompletionCode::try_from(event_trb.dw2.compl_code()).expect("Invalid completion code");

            // update the dequeue pointer
            // crate::println!("event_transfer {:x?}", event_trb);
            let deq_pt =
                unsafe { (event_trb.dw0 as *mut TransferTrbS).add(1).as_mut().expect("Couldn't deref ptr") };
            if deq_pt.get_trb_type() == TrbType::Link {
                udc_ep.deq_pt = AtomicPtr::new(udc_ep.first_trb.load(Ordering::SeqCst));
            } else {
                udc_ep.deq_pt = AtomicPtr::new(deq_pt as *mut TransferTrbS);
            }
            crate::println!("EventTransfer: comp_code {:?}, PEI {}", comp_code, pei);

            let dir = (pei & 1) != 0;
            if pei == 0 {
                if comp_code == CompletionCode::Success {
                    // ep0_xfer_complete
                    if dir == USB_SEND {
                        ret = CrgEvent::Data(0, 1, 0); // FIXME: this ordering contradicts the `dir` bit, but seems necessary to trigger the next packet send
                    } else {
                        ret = CrgEvent::Data(1, 0, 0); // FIXME: this ordering contradicts the `dir` bit, but seems necessary to trigger the next packet send
                    }
                } else {
                    crate::println!("EP0 unhandled comp_code: {:?}", comp_code);
                    ret = CrgEvent::None;
                }
            } else if pei >= 2 {
                if comp_code == CompletionCode::Success || comp_code == CompletionCode::ShortPacket {
                    crate::println!("EP{} xfer event, dir {}", ep_num, if dir { "OUT" } else { "IN" });
                    // xfer_complete
                    if let Some(f) = this.udc_ep[pei as usize].completion_handler {
                        // so unsafe. so unsafe. We're counting on the hardware to hand us a raw pointer
                        // that isn't corrupted.
                        let p_trb = unsafe { &*(event_trb.dw0 as *const TransferTrbS) };
                        f(this, p_trb.dplo as usize, p_trb.dw2.0, 0);
                    }
                } else if comp_code == CompletionCode::MissedServiceError {
                    crate::println!("MissedServiceError");
                } else {
                    crate::println!("EventTransfer {:?} event not handled", comp_code);
                }
            }
        }
        TrbType::SetupPkt => {
            let mut setup_storage = [0u8; 8];
            setup_storage.copy_from_slice(&event_trb.get_raw_setup());
            this.setup = Some(setup_storage);
            this.setup_tag = event_trb.get_setup_tag();

            let mut setup_pkt = CtrlRequest::default();
            setup_pkt.as_mut().copy_from_slice(&setup_storage);

            let w_value = setup_pkt.w_value;
            let w_index = setup_pkt.w_index;
            let w_length = setup_pkt.w_length;

            crate::println!(
                "  b_request={:x}, b_request_type={:x}, w_value={:04x}, w_index=0x{:x}, w_length={}",
                setup_pkt.b_request,
                setup_pkt.b_request_type,
                w_value,
                w_index,
                w_length
            );

            if (setup_pkt.b_request_type & USB_TYPE_MASK) == USB_TYPE_STANDARD {
                match setup_pkt.b_request {
                    USB_REQ_GET_STATUS => {
                        crate::println!("USB_REQ_GET_STATUS");
                        get_status_request(this, setup_pkt.b_request_type, w_index);
                    }
                    USB_REQ_SET_ADDRESS => {
                        crate::println!("USB_REQ_SET_ADDRESS: {}, tag: {}", w_value, this.setup_tag);
                        this.set_addr(w_value as u8, CRG_INT_TARGET);
                        // crate::println!(" ******* set address done {}", w_value & 0xff);
                    }
                    USB_REQ_SET_SEL => {
                        crate::println!("USB_REQ_SET_SEL");
                        this.ep0_receive(this.ep0_buf.load(Ordering::SeqCst) as usize, w_length as usize, 0);
                        delay(100);
                        /* do set sel */
                        crate::println!("SEL_VALUE NOT HANDLED");
                        /*
                        crg_udc->sel_value.u1_sel_value = *ep0_buf;
                        crg_udc->sel_value.u1_pel_value = *(ep0_buf+1);
                        crg_udc->sel_value.u2_sel_value = *(uint16_t*)(ep0_buf+2);
                        crg_udc->sel_value.u2_pel_value = *(uint16_t*)(ep0_buf+4);
                        */
                    }
                    USB_REQ_SET_ISOCH_DELAY => {
                        crate::println!("USB_REQ_SET_ISOCH_DELAY");
                        /* do set isoch delay */
                        this.ep0_send(0, 0, 0);
                    }
                    USB_REQ_CLEAR_FEATURE => {
                        crate::println!("USB_REQ_CLEAR_FEATURE");
                        crate::println!("*** UNSUPPORTED ***");
                        /* do clear feature */
                        // clear_feature_request(setup_pkt.b_request_type, w_index, w_value);
                    }
                    USB_REQ_SET_FEATURE => {
                        crate::println!("USB_REQ_SET_FEATURE\r\n");
                        crate::println!("*** UNSUPPORTED ***");
                        /* do set feature */
                        /*
                        if crg_udc_get_device_state() == USB_STATE_CONFIGURED {
                            set_feature_request(setup_pkt.b_request_type, w_index, w_value);
                        } else {
                            crg_udc_ep_halt(0, USB_RECV);
                        }
                        */
                    }
                    USB_REQ_SET_CONFIGURATION => {
                        crate::println!("USB_REQ_SET_CONFIGURATION");

                        let mut pass = false;
                        if w_value == 0 {
                            this.set_device_state(UsbDeviceState::Address);
                        } else if w_value == 1 {
                            this.set_device_state(UsbDeviceState::Configured);
                        } else {
                            this.ep_halt(0, USB_RECV);
                            pass = true;
                        }

                        if !pass {
                            this.assign_completion_handler(usb_ep1_bulk_in_complete, 1, USB_SEND);
                            this.assign_completion_handler(usb_ep1_bulk_out_complete, 1, USB_RECV);

                            enable_mass_storage_eps(this, 1);

                            this.bulk_xfer(1, USB_RECV, this.cbw_ptr(), 31, 0, 0);
                            this.ms_state = UmsState::CommandPhase;
                            this.ep0_send(0, 0, 0);
                        }
                    }
                    USB_REQ_GET_DESCRIPTOR => {
                        crate::println!("USB_REQ_GET_DESCRIPTOR");
                        get_descriptor_request(this, w_value, w_index as usize, w_length as usize);
                    }
                    USB_REQ_GET_CONFIGURATION => {
                        crate::println!("USB_REQ_GET_CONFIGURATION");
                        let ep0_buf = unsafe {
                            core::slice::from_raw_parts_mut(
                                this.ep0_buf.load(Ordering::SeqCst) as *mut u8,
                                CRG_UDC_EP0_REQBUFSIZE,
                            )
                        };
                        if this.get_device_state() != UsbDeviceState::Configured {
                            ep0_buf[0] = 0;
                        } else {
                            ep0_buf[0] = 1
                        }
                        this.ep0_send(ep0_buf.as_ptr() as usize, 1, 0);
                    }
                    USB_REQ_SET_INTERFACE => {
                        crate::println!("USB_REQ_SET_INTERFACE");
                        this.cur_interface_num = (w_value & 0xF) as u8;
                        crate::println!("USB_REQ_SET_INTERFACE altsetting {}", this.cur_interface_num);
                        this.ep0_send(0, 0, 0);
                    }
                    USB_REQ_GET_INTERFACE => {
                        crate::println!("USB_REQ_GET_INTERFACE");
                        let ep0_buf = unsafe {
                            core::slice::from_raw_parts_mut(
                                this.ep0_buf.load(Ordering::SeqCst) as *mut u8,
                                CRG_UDC_EP0_REQBUFSIZE,
                            )
                        };
                        ep0_buf[0] = this.cur_interface_num;
                        this.ep0_send(ep0_buf.as_ptr() as usize, 1, 0);
                    }
                    _ => {
                        crate::println!(
                            "USB_REQ default b_request=0x{:x}, b_request_type=0x{:x}",
                            setup_pkt.b_request,
                            setup_pkt.b_request_type
                        );
                        this.ep_halt(0, USB_RECV);
                    }
                }
            } else if (setup_pkt.b_request_type & USB_TYPE_MASK) == USB_TYPE_CLASS {
                match setup_pkt.b_request {
                    0xff => {
                        crate::println!("Mass Storage Reset\r\n");
                        if (0 == w_value)
                            && (InterfaceDescriptor::default_mass_storage().b_interface_number
                                == w_index as u8)
                            && (0 == w_length)
                        {
                            this.ep_unhalt(1, USB_SEND);
                            this.ep_unhalt(1, USB_RECV);
                            enable_mass_storage_eps(this, ep_num);
                            this.ms_state = UmsState::CommandPhase;
                            this.bulk_xfer(1, USB_RECV, this.cbw_ptr(), 31, 0, 0);
                            //crg_udc_ep0_status(false,0);
                            this.ep0_send(0, 0, 0);
                        } else {
                            this.ep_halt(0, USB_RECV);
                        }
                    }
                    0xfe => {
                        crate::println!("Get Max LUN");
                        if w_index != 0 || w_value != 0 || w_length != 1 {
                            this.ep_halt(0, USB_RECV);
                        } else {
                            let ep0_buf = unsafe {
                                core::slice::from_raw_parts_mut(
                                    this.ep0_buf.load(Ordering::SeqCst) as *mut u8,
                                    CRG_UDC_EP0_REQBUFSIZE,
                                )
                            };
                            ep0_buf[0] = 0;
                            this.ep0_send(ep0_buf.as_ptr() as usize, 1, 0);
                        }
                    }
                    _ => {
                        crate::println!("Unhandled!");
                    }
                }
            } else {
                this.ep_halt(0, USB_RECV);
            }

            ret = CrgEvent::Data(0, 0, 1);
        }
        TrbType::DataStage => {
            panic!("data stage needs handling");
        }
        _ => {
            crate::println!("Unexpected trb_type {:?}", event_trb.get_trb_type());
        }
    }
    ret
}
