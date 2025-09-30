use core::sync::atomic::Ordering;

use bao1x_hal::usb::driver::CorigineUsb;
use bao1x_hal::usb::driver::*;

use super::*;

pub static TX_IDLE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);

// Call from USB_REQ_SET_CONFIGURATION after setting device state to Configured
pub(crate) fn enable_serial_eps(this: &mut CorigineUsb) {
    // CDC notification IN (interrupt)
    this.ep_enable(2, USB_SEND, HS_INT_MPS as _, EpType::IntrInbound);

    // CDC data bulk OUT and IN
    this.ep_enable(3, USB_RECV, HS_BULK_MPS as _, EpType::BulkOutbound);
    this.ep_enable(3, USB_SEND, HS_BULK_MPS as _, EpType::BulkInbound);
}

pub fn get_descriptor_request(this: &mut CorigineUsb, value: u16, _index: usize, length: usize) {
    let ep0_buf = unsafe {
        core::slice::from_raw_parts_mut(
            this.ep0_buf.load(Ordering::SeqCst) as *mut u8,
            CRG_UDC_EP0_REQBUFSIZE,
        )
    };

    match (value >> 8) as u8 {
        USB_DT_DEVICE => {
            let mut dd = DeviceDescriptor::default_usb_acm();
            dd.b_max_packet_size0 = 64;
            let len = length.min(core::mem::size_of::<DeviceDescriptor>());
            ep0_buf[..len].copy_from_slice(&dd.as_ref()[..len]);
            this.ep0_send(this.ep0_buf.load(Ordering::SeqCst) as usize, len, 0);
        }
        USB_DT_DEVICE_QUALIFIER => {
            this.ep_halt(0, USB_RECV);
        }
        USB_DT_CONFIG => {
            let wrote = write_config_hs(ep0_buf);
            let buffsize = wrote.min(length);
            this.ep0_send(this.ep0_buf.load(Ordering::SeqCst) as usize, buffsize, 0);
        }
        USB_DT_OTHER_SPEED_CONFIG => {
            let wrote = write_config_fs(ep0_buf);
            let buffsize = wrote.min(length);
            this.ep0_send(this.ep0_buf.load(Ordering::SeqCst) as usize, buffsize, 0);
        }
        USB_DT_STRING => {
            let id = (value & 0xFF) as u8;
            let len = if id == 0 {
                ep0_buf[..4].copy_from_slice(&[4, USB_DT_STRING, 9, 4]);
                4
            } else {
                let s = match id {
                    1 => MANUFACTURER,
                    2 => PRODUCT,
                    _ => SERIAL,
                };
                let slen = 2 + s.len() * 2;
                ep0_buf[0] = slen as u8;
                ep0_buf[1] = USB_DT_STRING;
                for (dst, &src) in ep0_buf[2..].chunks_exact_mut(2).zip(s.as_bytes()) {
                    dst.copy_from_slice(&(src as u16).to_le_bytes());
                }
                slen
            };
            let buffsize = length.min(len);
            this.ep0_send(this.ep0_buf.load(Ordering::SeqCst) as usize, buffsize, 0);
        }
        USB_DT_BOS => {
            this.ep_halt(0, USB_RECV);
        }
        _ => {
            this.ep_halt(0, USB_RECV);
        }
    }
}

// ===== CDC Bulk OUT (EP3 OUT) =====
pub fn usb_ep3_bulk_out_complete(
    this: &mut CorigineUsb,
    buf_addr: usize,
    _info: u32,
    _error: u8,
    residual: u16,
) {
    // crate::println_d!("EP3 OUT: {:x} {:x}", info, _error);

    let actual = CRG_UDC_APP_BUF_LEN - residual as usize;

    if actual == 0 {
        return; // zero-length packet, ignore
    }

    // Slice of received data
    let buf = unsafe { core::slice::from_raw_parts(buf_addr as *const u8, actual as usize) };

    // For now: just print, or push into a ring buffer for your "virtual terminal"
    // crate::println_d!("CDC OUT received {} bytes: {:?}", actual, &buf);

    critical_section::with(|cs| {
        let mut queue = crate::USB_RX.borrow(cs).borrow_mut();
        for &d in buf {
            queue.push_back(d);
        }
    });

    // Re-arm OUT transfer so host can send more
    let acm_buf = this.cdc_acm_rx_slice();
    this.bulk_xfer(3, USB_RECV, acm_buf.as_ptr() as usize, acm_buf.len(), 0, 0);
}

pub fn flush() {
    unsafe {
        if let Some(ref mut usb_ref) = crate::platform::usb::USB {
            let usb = &mut *core::ptr::addr_of_mut!(*usb_ref);
            flush_tx(usb);
        }
    }
}

pub fn flush_tx(this: &mut CorigineUsb) {
    let mut written = 0;

    while !crate::platform::usb::TX_IDLE.swap(false, core::sync::atomic::Ordering::SeqCst) {
        // wait for tx to go idle
    }

    let tx_buf = this.cdc_acm_tx_slice();
    critical_section::with(|cs| {
        let mut queue = crate::USB_TX.borrow(cs).borrow_mut();
        let to_copy = queue.len().min(CRG_UDC_APP_BUF_LEN);

        let (a, b) = queue.as_slices();
        if to_copy <= a.len() {
            tx_buf[..to_copy].copy_from_slice(&a[..to_copy]);
            queue.drain(..to_copy);
            written = to_copy;
        } else {
            let first = a.len();
            let second = to_copy - first;
            tx_buf[..first].copy_from_slice(a);
            tx_buf[first..to_copy].copy_from_slice(&b[..second]);
            queue.drain(..to_copy);
            written = to_copy;
        }
    });

    if written > 0 {
        this.bulk_xfer(3, USB_SEND, tx_buf.as_ptr() as usize, written, 0, 0);
    } else {
        // release the lock
        TX_IDLE.store(true, Ordering::SeqCst);
    }
}

// ===== CDC Bulk IN (EP3 IN) =====
pub fn usb_ep3_bulk_in_complete(
    this: &mut CorigineUsb,
    _buf_addr: usize,
    _info: u32,
    _error: u8,
    _residual: u16,
) {
    // crate::println_d!("EP3 IN");
    // let length = CRG_UDC_APP_BUF_LEN - residual as usize;
    // crate::println_d!("CDC IN transfer complete, {} bytes sent", length);

    // signal that more stuff can be put into the pipe
    TX_IDLE.store(true, Ordering::SeqCst);

    // this may or may not initiate a new connection, depending on how full the Tx buffer is
    flush_tx(this);
}

// ===== CDC Notification IN (EP2 IN) =====
pub fn usb_ep2_int_in_complete(
    _this: &mut CorigineUsb,
    _buf_addr: usize,
    _info: u32,
    _error: u8,
    _residual: u16,
) {
    // crate::println_d!("EP2 INT");
    // let length = CRG_UDC_APP_BUF_LEN - residual as usize;
    // crate::println_d!("CDC notification sent, {} bytes", length);

    // Typically sends SERIAL_STATE bitmap (carrier detect etc).
    // Ignoring - this is a virtual terminal
}
