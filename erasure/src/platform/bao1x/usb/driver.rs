use core::convert::TryFrom;
use core::sync::atomic::{AtomicPtr, Ordering};

use bao1x_hal::usb::compat::AtomicCsr;
use bao1x_hal::usb::driver::CorigineUsb;
use bao1x_hal::usb::driver::*;
use bao1x_hal::usb::utra::*;

use super::*;

pub(crate) static mut USB: Option<CorigineUsb> = None;

pub unsafe fn init_usb() {
    let mut usb = unsafe {
        bao1x_hal::usb::driver::CorigineUsb::new(
            bao1x_hal::board::CRG_UDC_MEMBASE,
            AtomicCsr::new(bao1x_hal::usb::utra::CORIGINE_USB_BASE as *mut u32),
            AtomicCsr::new(utralib::utra::irqarray1::HW_IRQARRAY1_BASE as *mut u32),
        )
    };
    usb.assign_handler(handle_event);

    // install the interrupt handler
    enable_irq(utralib::utra::irqarray1::IRQARRAY1_IRQ);

    unsafe {
        USB = Some(usb);
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
    // crate::println_d!("handle_event: {:x?}", event_trb);
    let pei = event_trb.get_endpoint_id();
    let _ep_num = pei >> 1;
    let udc_ep = &mut this.udc_ep[pei as usize];
    let mut ret = CrgEvent::None;
    match event_trb.get_trb_type() {
        TrbType::EventPortStatusChange => {
            let portsc_val = this.csr.r(PORTSC);
            this.csr.wo(PORTSC, portsc_val);
            // this.print_status(portsc_val);

            let portsc = PortSc(portsc_val);
            // crate::println_d!("{:?}", portsc);

            if portsc.prc() && !portsc.pr() {
                crate::println_d!("update_current_speed() - reset done");
                this.update_current_speed();
            }
            if portsc.csc() && portsc.ppc() && portsc.pp() && portsc.ccs() {
                crate::println_d!("update_current_speed() - cable connect");
                this.update_current_speed();
            }

            this.csr.rmwf(EVENTCONFIG_SETUP_ENABLE, 1);
        }
        TrbType::EventTransfer => {
            let comp_code =
                CompletionCode::try_from(event_trb.dw2.compl_code()).expect("Invalid completion code");

            #[cfg(feature = "verbose-debug")]
            crate::println_d!(
                "e_trb {:x} {:x} {:x} {:x}",
                event_trb.dw0,
                event_trb.dw1,
                event_trb.dw2.0,
                event_trb.dw3.0
            );
            let residual_length = event_trb.dw2.trb_tran_len() as u16;
            // update the dequeue pointer
            // crate::println_d!("event_transfer {:x?}", event_trb);
            let deq_pt =
                unsafe { (event_trb.dw0 as *mut TransferTrbS).add(1).as_mut().expect("Couldn't deref ptr") };
            if deq_pt.get_trb_type() == TrbType::Link {
                udc_ep.deq_pt = AtomicPtr::new(udc_ep.first_trb.load(Ordering::SeqCst));
            } else {
                udc_ep.deq_pt = AtomicPtr::new(deq_pt as *mut TransferTrbS);
            }
            // crate::println_d!("EventTransfer: comp_code {:?}, PEI {}", comp_code, pei);

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
                    crate::println_d!("EP0 unhandled comp_code: {:?}", comp_code);
                    ret = CrgEvent::None;
                }
            } else if pei >= 2 {
                if comp_code == CompletionCode::Success || comp_code == CompletionCode::ShortPacket {
                    // crate::println_d!("EP{} xfer event, dir {}", ep_num, if dir { "OUT" } else { "IN" });
                    // xfer_complete
                    if let Some(f) = this.udc_ep[pei as usize].completion_handler {
                        // so unsafe. so unsafe. We're counting on the hardware to hand us a raw pointer
                        // that isn't corrupted.
                        let p_trb = unsafe { &*(event_trb.dw0 as *const TransferTrbS) };
                        #[cfg(feature = "verbose-debug")]
                        crate::println_d!(
                            "p_trb {:x} {:x} {:x} {:x}",
                            p_trb.dphi,
                            p_trb.dplo,
                            p_trb.dw2.0,
                            p_trb.dw3.0
                        );
                        f(this, p_trb.dplo as usize, p_trb.dw2.0, 0, residual_length);
                    }
                } else if comp_code == CompletionCode::MissedServiceError {
                    crate::println_d!("MissedServiceError");
                } else {
                    crate::println_d!("EventTransfer {:?} event not handled", comp_code);
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

            /*
            crate::println_d!(
                "  b_request={:x}, b_request_type={:x}, w_value={:04x}, w_index=0x{:x}, w_length={}",
                setup_pkt.b_request,
                setup_pkt.b_request_type,
                w_value,
                w_index,
                w_length
            );
            */

            if (setup_pkt.b_request_type & USB_TYPE_MASK) == USB_TYPE_STANDARD {
                match setup_pkt.b_request {
                    USB_REQ_GET_STATUS => {
                        crate::println_d!("USB_REQ_GET_STATUS");
                        get_status_request(this, setup_pkt.b_request_type, w_index);
                    }
                    USB_REQ_SET_ADDRESS => {
                        crate::println_d!("USB_REQ_SET_ADDRESS: {}, tag: {}", w_value, this.setup_tag);
                        this.set_addr(w_value as u8, CRG_INT_TARGET);
                        // crate::println_d!(" ******* set address done {}", w_value & 0xff);
                    }
                    USB_REQ_SET_SEL => {
                        crate::println_d!("USB_REQ_SET_SEL");
                        this.ep0_receive(this.ep0_buf.load(Ordering::SeqCst) as usize, w_length as usize, 0);
                        delay(100);
                        /* do set sel */
                        crate::println_d!("SEL_VALUE NOT HANDLED");
                    }
                    USB_REQ_SET_ISOCH_DELAY => {
                        crate::println_d!("USB_REQ_SET_ISOCH_DELAY");
                        /* do set isoch delay */
                        this.ep0_send(0, 0, 0);
                    }
                    USB_REQ_CLEAR_FEATURE => {
                        crate::println_d!("USB_REQ_CLEAR_FEATURE");
                        crate::println_d!("*** UNSUPPORTED ***");
                        this.ep_halt(0, USB_RECV);
                    }
                    USB_REQ_SET_FEATURE => {
                        crate::println_d!("USB_REQ_SET_FEATURE\r\n");
                        crate::println_d!("*** UNSUPPORTED ***");
                        this.ep_halt(0, USB_RECV);
                    }
                    USB_REQ_SET_CONFIGURATION => {
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
                            // USB-CDC-ACM
                            this.assign_completion_handler(usb_ep3_bulk_in_complete, 3, USB_SEND);
                            this.assign_completion_handler(usb_ep3_bulk_out_complete, 3, USB_RECV);
                            this.assign_completion_handler(usb_ep2_int_in_complete, 2, USB_SEND);
                            enable_serial_eps(this);

                            // Kick off CDC-ACM
                            let acm_buf_rx = this.cdc_acm_rx_slice();
                            this.bulk_xfer(3, USB_RECV, acm_buf_rx.as_ptr() as usize, acm_buf_rx.len(), 0, 0);

                            this.ep0_send(0, 0, 0);
                        }
                    }
                    USB_REQ_GET_DESCRIPTOR => {
                        crate::println_d!("USB_REQ_GET_DESCRIPTOR");
                        get_descriptor_request(this, w_value, w_index as usize, w_length as usize);
                    }
                    USB_REQ_GET_CONFIGURATION => {
                        crate::println_d!("USB_REQ_GET_CONFIGURATION");
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
                        crate::println_d!("USB_REQ_SET_INTERFACE");
                        this.cur_interface_num = (w_value & 0xF) as u8;
                        crate::println_d!("USB_REQ_SET_INTERFACE altsetting {}", this.cur_interface_num);
                        this.ep0_send(0, 0, 0);
                    }
                    USB_REQ_GET_INTERFACE => {
                        crate::println_d!("USB_REQ_GET_INTERFACE");
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
                        crate::println_d!(
                            "USB_REQ default b_request=0x{:x}, b_request_type=0x{:x}",
                            setup_pkt.b_request,
                            setup_pkt.b_request_type
                        );
                        this.ep_halt(0, USB_RECV);
                    }
                }
            } else if (setup_pkt.b_request_type & USB_TYPE_MASK) == USB_TYPE_CLASS {
                match setup_pkt.b_request {
                    0x20 => {
                        // SET_LINE_CODING (host -> device, 7 bytes)
                        crate::println_d!("CDC SET_LINE_CODING");
                        let length = w_length as usize;
                        if length == 7 {
                            // queue EP0 OUT to receive the line coding structure
                            this.ep0_receive(this.ep0_buf.load(Ordering::SeqCst) as usize, length, 0);
                            // when complete, just ignore or store it
                        } else {
                            this.ep_halt(0, USB_RECV);
                        }
                    }
                    0x21 => {
                        // GET_LINE_CODING (device -> host, 7 bytes)
                        crate::println_d!("CDC GET_LINE_CODING");
                        let ep0_buf = unsafe {
                            core::slice::from_raw_parts_mut(
                                this.ep0_buf.load(Ordering::SeqCst) as *mut u8,
                                CRG_UDC_EP0_REQBUFSIZE,
                            )
                        };
                        // Fill with a dummy 115200 8N1 config
                        ep0_buf[0..4].copy_from_slice(&115200u32.to_le_bytes()); // dwDTERate
                        ep0_buf[4] = 0; // 1 stop bit
                        ep0_buf[5] = 0; // no parity
                        ep0_buf[6] = 8; // 8 data bits
                        this.ep0_send(ep0_buf.as_ptr() as usize, 7, 0);
                    }
                    0x22 => {
                        // SET_CONTROL_LINE_STATE (host -> device, 0 length)
                        crate::println_d!("CDC SET_CONTROL_LINE_STATE, DTR/RTS = {:x}", w_value);
                        // wValue bit0 = DTR, bit1 = RTS
                        // You can ignore since no real UART

                        this.ep0_send(0, 0, 0);
                    }
                    _ => {
                        crate::println_d!("Unhandled class request bRequest=0x{:x}", setup_pkt.b_request);
                        this.ep_halt(0, USB_RECV);
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
            crate::println_d!("Unexpected trb_type {:?}", event_trb.get_trb_type());
        }
    }
    ret
}
