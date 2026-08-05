// SPDX-License-Identifier: Apache-2.0
//
//! USB CCID bulk transport: assembles PC_to_RDR frames and streams RDR_to_PC replies.
//! No APDU or OpenPGP interpretation; an external process supplies raw RDR bytes.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};

use num_traits::ToPrimitive;
use usb_bao1x::ccid_framing::{
    CCID_BULK_MAX_PACKET as CCID_BULK_MAX_PACKET_BYTES, CCID_HEADER_LEN, append_bulk_out, consume_tx_chunk,
    frame_total_len, is_get_slot_status, next_tx_chunk, rdr_to_pc_slot_status_ok,
};
use usb_device::Result as UsbResult;
use usb_device::UsbError;
use usb_device::class::UsbClass;
use usb_device::class_prelude::*;
use usb_device::descriptor::DescriptorWriter;

use crate::api::Opcode;

/// USB interface class: CCID.
pub const USB_INTERFACE_CLASS_CCID: u8 = 0x0B;
const USB_INTERFACE_SUBCLASS_CCID: u8 = 0x00;
const USB_INTERFACE_PROTOCOL_CCID: u8 = 0x00;

const CCID_BCD_CCID: u16 = 0x0110;
const CCID_MAX_SLOT_INDEX: u8 = 0x00;
const CCID_VOLTAGE_SUPPORT: u8 = 0x07;
const CCID_DW_PROTOCOLS: u32 = 0x0000_0002;
const CCID_DW_DEFAULT_CLOCK: u32 = 0x0000_0FA0;
const CCID_DW_MAXIMUM_CLOCK: u32 = 0x0000_0FA0;
const CCID_DW_DATA_RATE: u32 = 0x0000_2580;
const CCID_DW_MAX_DATA_RATE: u32 = 0x0000_2580;
const CCID_DW_MAX_IFSD: u32 = 0xFE;
const CCID_DW_SYNCH_PROTOCOLS: u32 = 0;
const CCID_DW_MECHANICAL: u32 = 0;
const CCID_DW_FEATURES: u32 = 0x0004_00FE;
/// Must match `usb_bao1x::ccid_framing::CCID_WIRE_MAX` (Option A: short APDU).
const CCID_MAX_MESSAGE_LENGTH: u32 = 0x10F;
const CCID_CLASS_GET_RESPONSE: u8 = 0xFF;
const CCID_CLASS_ENVELOPE: u8 = 0xFF;
const CCID_LCD_LAYOUT: u16 = 0;
const CCID_PIN_SUPPORT: u8 = 0x00;
const CCID_MAX_BUSY_SLOTS: u8 = 0x01;
/// 0 = continuous clock range (no discrete clock table).
const CCID_NUM_CLOCK_SUPPORTED: u8 = 0x00;
/// 0 = continuous data-rate range (no discrete rate table).
const CCID_NUM_DATA_RATES_SUPPORTED: u8 = 0x00;

pub(crate) const CCID_BULK_MAX_PACKET: u16 = CCID_BULK_MAX_PACKET_BYTES as u16;

/// USB CCID 1.1 class functional descriptor (54 bytes / `bLength` = 0x36).
///
/// Layout matches the USB CCID Class Specification 1.1 §5.1 table (including
/// `bNumClockSupported` at offset 18 and `bNumDataRatesSupported` at offset 27).
const fn ccid_class_descriptor_bytes() -> [u8; 54] {
    let mut b = [0u8; 54];
    b[0] = 0x36; // bLength
    b[1] = 0x21; // bDescriptorType (CCID functional)
    let bcd = CCID_BCD_CCID.to_le_bytes();
    b[2] = bcd[0];
    b[3] = bcd[1];
    b[4] = CCID_MAX_SLOT_INDEX;
    b[5] = CCID_VOLTAGE_SUPPORT;
    let protocols = CCID_DW_PROTOCOLS.to_le_bytes();
    b[6] = protocols[0];
    b[7] = protocols[1];
    b[8] = protocols[2];
    b[9] = protocols[3];
    let def_clk = CCID_DW_DEFAULT_CLOCK.to_le_bytes();
    b[10] = def_clk[0];
    b[11] = def_clk[1];
    b[12] = def_clk[2];
    b[13] = def_clk[3];
    let max_clk = CCID_DW_MAXIMUM_CLOCK.to_le_bytes();
    b[14] = max_clk[0];
    b[15] = max_clk[1];
    b[16] = max_clk[2];
    b[17] = max_clk[3];
    b[18] = CCID_NUM_CLOCK_SUPPORTED;
    let data_rate = CCID_DW_DATA_RATE.to_le_bytes();
    b[19] = data_rate[0];
    b[20] = data_rate[1];
    b[21] = data_rate[2];
    b[22] = data_rate[3];
    let max_data_rate = CCID_DW_MAX_DATA_RATE.to_le_bytes();
    b[23] = max_data_rate[0];
    b[24] = max_data_rate[1];
    b[25] = max_data_rate[2];
    b[26] = max_data_rate[3];
    b[27] = CCID_NUM_DATA_RATES_SUPPORTED;
    let max_ifsd = CCID_DW_MAX_IFSD.to_le_bytes();
    b[28] = max_ifsd[0];
    b[29] = max_ifsd[1];
    b[30] = max_ifsd[2];
    b[31] = max_ifsd[3];
    let synch = CCID_DW_SYNCH_PROTOCOLS.to_le_bytes();
    b[32] = synch[0];
    b[33] = synch[1];
    b[34] = synch[2];
    b[35] = synch[3];
    let mechanical = CCID_DW_MECHANICAL.to_le_bytes();
    b[36] = mechanical[0];
    b[37] = mechanical[1];
    b[38] = mechanical[2];
    b[39] = mechanical[3];
    let features = CCID_DW_FEATURES.to_le_bytes();
    b[40] = features[0];
    b[41] = features[1];
    b[42] = features[2];
    b[43] = features[3];
    let max_msg = CCID_MAX_MESSAGE_LENGTH.to_le_bytes();
    b[44] = max_msg[0];
    b[45] = max_msg[1];
    b[46] = max_msg[2];
    b[47] = max_msg[3];
    b[48] = CCID_CLASS_GET_RESPONSE;
    b[49] = CCID_CLASS_ENVELOPE;
    let lcd = CCID_LCD_LAYOUT.to_le_bytes();
    b[50] = lcd[0];
    b[51] = lcd[1];
    b[52] = CCID_PIN_SUPPORT;
    b[53] = CCID_MAX_BUSY_SLOTS;
    b
}

const _: () = assert!(ccid_class_descriptor_bytes().len() == 54);
const _: () = assert!(ccid_class_descriptor_bytes()[0] == 0x36);
const _: () = assert!(ccid_class_descriptor_bytes()[1] == 0x21);
const _: () = assert!(ccid_class_descriptor_bytes()[18] == 0x00);
const _: () = assert!(ccid_class_descriptor_bytes()[27] == 0x00);
const _: () = assert!(ccid_class_descriptor_bytes()[44] == 0x0F);
const _: () = assert!(ccid_class_descriptor_bytes()[45] == 0x01);

fn remove_tx_prefix(tx: &mut Vec<u8>, n: usize) { consume_tx_chunk(tx, n); }

struct CcidTransportInner {
    rx_assembly: Vec<u8>,
    tx_buf: Vec<u8>,
    tx_pending: bool,
}

/// CCID USB class: bulk IN/OUT only.
///
/// The CCID interrupt IN endpoint (RDR_to_PC_NotifySlotChange) is intentionally
/// omitted. Corigine `alloc_ep` pairs IN/OUT by `EndpointType` and will reuse an
/// Interrupt EP number when the other direction is free — a lone CCID interrupt
/// IN allocated before HID caused NKRO to overwrite that PEI (host EPROTO/-71).
pub struct CcidTransportClass<'a, B: UsbBus> {
    iface: InterfaceNumber,
    bulk_out: EndpointOut<'a, B>,
    bulk_in: EndpointIn<'a, B>,
    inner: RefCell<CcidTransportInner>,
    complete_rx: std::rc::Rc<RefCell<VecDeque<Vec<u8>>>>,
    notify_cid: xous::CID,
    /// Set by [`UsbClass::reset`]; main clears deferred `CcidRxDeferred` waiter.
    session_hangup: AtomicBool,
}

impl<'a, B: UsbBus> CcidTransportClass<'a, B> {
    pub fn new(
        alloc: &'a UsbBusAllocator<B>,
        complete_rx: std::rc::Rc<RefCell<VecDeque<Vec<u8>>>>,
        notify_cid: xous::CID,
    ) -> Self {
        Self {
            iface: alloc.interface(),
            bulk_out: alloc.bulk(CCID_BULK_MAX_PACKET as u16),
            bulk_in: alloc.bulk(CCID_BULK_MAX_PACKET as u16),
            inner: RefCell::new(CcidTransportInner {
                rx_assembly: Vec::new(),
                tx_buf: Vec::new(),
                tx_pending: false,
            }),
            complete_rx,
            notify_cid,
            session_hangup: AtomicBool::new(false),
        }
    }

    /// Returns true once after a USB reset/unplug cleared transport state.
    pub fn take_session_hangup(&self) -> bool { self.session_hangup.swap(false, Ordering::SeqCst) }

    /// Queue raw RDR_to_PC bytes from an external handler. They are chunked on bulk IN.
    pub fn enqueue_response(&self, data: Vec<u8>) {
        let mut g = self.inner.borrow_mut();
        g.tx_buf = data;
        g.tx_pending = !g.tx_buf.is_empty();
    }

    /// USB endpoint index for CCID bulk OUT (for Corigine `ep_out_ready` force-rearm).
    pub(crate) fn bulk_out_index(&self) -> usize { self.bulk_out.address().index() }

    /// Arm Corigine bulk OUT by attempting a read (side effect queues a receive TRB).
    ///
    /// The bao1x `UsbBus::read` path only calls `bulk_xfer(CRG_OUT, …)` when the endpoint
    /// is not already marked ready and `address_is_set` is true. Without at least one
    /// successful arm after SET_ADDRESS / SET_CONFIGURATION, the host's first CCID bulk
    /// OUT times out (`LIBUSB_ERROR_TIMEOUT`). Buffer length must be the bulk max packet
    /// size — the driver sizes the TRB from `buf.len()`.
    ///
    /// Call sites: once after SE0 release in `main`, and from `endpoint_out` (re-arm
    /// after receiving data). Do not call from `poll()` — that runs in the USB IRQ
    /// path and a register-touching `read()` there breaks SET_ADDRESS timing (-71).
    pub(crate) fn prime_bulk_out(&self) {
        let mut tmp = [0u8; CCID_BULK_MAX_PACKET as usize];
        match self.bulk_out.read(&mut tmp) {
            Ok(0) | Err(UsbError::WouldBlock) => {
                // No payload (or already armed). WouldBlock still arms when needed
                // once `address_is_set` is true; before that it is a no-op.
            }
            Ok(n) => {
                // Rare: data arrived during prime — fold into the normal RX path.
                let mut g = self.inner.borrow_mut();
                if append_bulk_out(&mut g.rx_assembly, &tmp[..n]).is_ok() {
                    Self::drain_complete_messages(&mut g, &*self.complete_rx, self.notify_cid);
                }
            }
            Err(_) => {}
        }
    }

    fn drain_complete_messages(
        g: &mut CcidTransportInner,
        complete_rx: &RefCell<VecDeque<Vec<u8>>>,
        notify_cid: xous::CID,
    ) -> bool {
        // Returns true if any GetSlotStatus was answered inline (caller must poll_bulk_in).
        let mut answered_inline = false;
        let mut new_frames = 0usize;
        loop {
            let Some(total) = frame_total_len(&g.rx_assembly) else {
                if g.rx_assembly.len() >= CCID_HEADER_LEN {
                    g.rx_assembly.clear();
                }
                break;
            };
            if g.rx_assembly.len() < total {
                break;
            }
            let frame = g.rx_assembly[..total].to_vec();
            g.rx_assembly.drain(..total);

            // libccid CreateChannel uses two GetSlotStatus probes with a 100 ms
            // ReadUSB timeout. Answer those in IRQ context — do not wake the stub.
            if is_get_slot_status(&frame) {
                let slot = frame[5];
                let seq = frame[6];
                let resp = rdr_to_pc_slot_status_ok(slot, seq);
                g.tx_buf.clear();
                g.tx_buf.extend_from_slice(&resp);
                g.tx_pending = true;
                answered_inline = true;
                continue;
            }

            complete_rx.borrow_mut().push_back(frame);
            new_frames += 1;
        }
        if new_frames > 0 {
            xous::try_send_message(
                notify_cid,
                xous::Message::new_scalar(Opcode::IrqCcidRx.to_usize().unwrap(), 0, 0, 0, 0),
            )
            .ok();
        }
        answered_inline
    }

    fn poll_bulk_in(&self) {
        let chunk: Vec<u8> = {
            let g = self.inner.borrow();
            if !g.tx_pending {
                return;
            }
            match next_tx_chunk(&g.tx_buf) {
                Some(c) => c.to_vec(),
                None => return,
            }
        };
        match self.bulk_in.write(&chunk) {
            Ok(n) => {
                let mut g = self.inner.borrow_mut();
                remove_tx_prefix(&mut g.tx_buf, n);
                if g.tx_buf.is_empty() {
                    g.tx_pending = false;
                }
            }
            Err(UsbError::WouldBlock) => {}
            Err(_) => {
                let mut g = self.inner.borrow_mut();
                g.tx_pending = false;
                g.tx_buf.clear();
            }
        }
    }
}

impl<'a, B: UsbBus> UsbClass<B> for CcidTransportClass<'a, B> {
    fn get_configuration_descriptors(&self, writer: &mut DescriptorWriter) -> UsbResult<()> {
        writer.interface(
            self.iface,
            USB_INTERFACE_CLASS_CCID,
            USB_INTERFACE_SUBCLASS_CCID,
            USB_INTERFACE_PROTOCOL_CCID,
        )?;
        let fd = ccid_class_descriptor_bytes();
        writer.write(0x21, &fd[2..])?;
        writer.endpoint(&self.bulk_out)?;
        writer.endpoint(&self.bulk_in)?;
        Ok(())
    }

    fn reset(&mut self) {
        let mut g = self.inner.borrow_mut();
        g.rx_assembly.clear();
        g.tx_buf.clear();
        g.tx_pending = false;
        self.complete_rx.borrow_mut().clear();
        // Wake main so deferred CcidRxDeferred waiters get Hangup (arg1=1).
        self.session_hangup.store(true, Ordering::SeqCst);
        xous::try_send_message(
            self.notify_cid,
            xous::Message::new_scalar(Opcode::IrqCcidRx.to_usize().unwrap(), 1, 0, 0, 0),
        )
        .ok();
    }

    fn poll(&mut self) { self.poll_bulk_in(); }

    fn endpoint_out(&mut self, addr: EndpointAddress) {
        if addr != self.bulk_out.address() {
            return;
        }
        // Stack buffer: IRQ context must not allocate (same size as bulk max packet).
        let mut tmp = [0u8; CCID_BULK_MAX_PACKET as usize];
        if let Ok(n) = self.bulk_out.read(&mut tmp) {
            if n == 0 {
                // Still re-arm so the next host write has a TRB.
                self.prime_bulk_out();
                return;
            }
            let answered_inline = {
                let mut g = self.inner.borrow_mut();
                if append_bulk_out(&mut g.rx_assembly, &tmp[..n]).is_err() {
                    drop(g);
                    self.prime_bulk_out();
                    return;
                }
                Self::drain_complete_messages(&mut g, &*self.complete_rx, self.notify_cid)
            };
            // Flush SlotStatus (and any other pending IN) before leaving IRQ.
            if answered_inline {
                self.poll_bulk_in();
            }
            // Corigine `read` re-arms after consuming a packet; call again so a failed
            // internal re-arm still leaves OUT ready for the next WriteUSB.
            self.prime_bulk_out();
        }
    }
}
