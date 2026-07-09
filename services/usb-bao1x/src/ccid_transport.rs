// SPDX-License-Identifier: Apache-2.0
//
//! USB CCID bulk transport: assembles PC_to_RDR frames and streams RDR_to_PC replies.
//! No APDU or OpenPGP interpretation; an external process supplies raw RDR bytes.

use std::cell::RefCell;
use std::collections::VecDeque;

use num_traits::ToPrimitive;
use usb_device::Result as UsbResult;
use usb_device::UsbError;
use usb_device::class::UsbClass;
use usb_device::class_prelude::*;
use usb_device::descriptor::DescriptorWriter;

use crate::api::Opcode;
use usb_bao1x::ccid_framing::{
    append_bulk_out, consume_tx_chunk, drain_complete_frames, next_tx_chunk,
    CCID_BULK_MAX_PACKET as CCID_BULK_MAX_PACKET_BYTES,
};

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
const CCID_MAX_MESSAGE_LENGTH: u32 = 0x10F;
const CCID_CLASS_GET_RESPONSE: u8 = 0xFF;
const CCID_CLASS_ENVELOPE: u8 = 0xFF;
const CCID_LCD_LAYOUT: u16 = 0;
const CCID_PIN_SUPPORT: u8 = 0x00;
const CCID_MAX_BUSY_SLOTS: u8 = 0x01;

pub(crate) const CCID_BULK_MAX_PACKET: u16 = CCID_BULK_MAX_PACKET_BYTES as u16;
const CCID_INTERRUPT_MAX_PACKET: u16 = 8;
const CCID_INTERRUPT_INTERVAL_MS: u8 = 24;

fn ccid_class_descriptor_bytes() -> [u8; 54] {
    let mut b = [0u8; 54];
    b[0] = 0x21;
    b[1] = 0x36;
    b[2..4].copy_from_slice(&CCID_BCD_CCID.to_le_bytes());
    b[4] = CCID_MAX_SLOT_INDEX;
    b[5] = CCID_VOLTAGE_SUPPORT;
    b[6..10].copy_from_slice(&CCID_DW_PROTOCOLS.to_le_bytes());
    b[10..14].copy_from_slice(&CCID_DW_DEFAULT_CLOCK.to_le_bytes());
    b[14..18].copy_from_slice(&CCID_DW_MAXIMUM_CLOCK.to_le_bytes());
    b[18..22].copy_from_slice(&CCID_DW_DATA_RATE.to_le_bytes());
    b[22..26].copy_from_slice(&CCID_DW_MAX_DATA_RATE.to_le_bytes());
    b[26..30].copy_from_slice(&CCID_DW_MAX_IFSD.to_le_bytes());
    b[30..34].copy_from_slice(&CCID_DW_SYNCH_PROTOCOLS.to_le_bytes());
    b[34..38].copy_from_slice(&CCID_DW_MECHANICAL.to_le_bytes());
    b[38..42].copy_from_slice(&CCID_DW_FEATURES.to_le_bytes());
    b[42..46].copy_from_slice(&CCID_MAX_MESSAGE_LENGTH.to_le_bytes());
    b[46] = CCID_CLASS_GET_RESPONSE;
    b[47] = CCID_CLASS_ENVELOPE;
    b[48..50].copy_from_slice(&CCID_LCD_LAYOUT.to_le_bytes());
    b[50] = CCID_PIN_SUPPORT;
    b[51] = CCID_MAX_BUSY_SLOTS;
    b
}

fn remove_tx_prefix(tx: &mut Vec<u8>, n: usize) { consume_tx_chunk(tx, n); }

struct CcidTransportInner {
    rx_assembly: Vec<u8>,
    tx_buf: Vec<u8>,
    tx_pending: bool,
}

/// CCID USB class: bulk IN/OUT + interrupt IN (notifications stub).
pub struct CcidTransportClass<'a, B: UsbBus> {
    iface: InterfaceNumber,
    bulk_out: EndpointOut<'a, B>,
    bulk_in: EndpointIn<'a, B>,
    interrupt_in: EndpointIn<'a, B>,
    inner: RefCell<CcidTransportInner>,
    complete_rx: std::rc::Rc<RefCell<VecDeque<Vec<u8>>>>,
    notify_cid: xous::CID,
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
            interrupt_in: alloc.interrupt(CCID_INTERRUPT_MAX_PACKET, CCID_INTERRUPT_INTERVAL_MS),
            inner: RefCell::new(CcidTransportInner {
                rx_assembly: Vec::new(),
                tx_buf: Vec::new(),
                tx_pending: false,
            }),
            complete_rx,
            notify_cid,
        }
    }

    /// Queue raw RDR_to_PC bytes from an external handler. They are chunked on bulk IN.
    pub fn enqueue_response(&self, data: Vec<u8>) {
        let mut g = self.inner.borrow_mut();
        g.tx_buf = data;
        g.tx_pending = !g.tx_buf.is_empty();
    }

    fn drain_complete_messages(
        g: &mut CcidTransportInner,
        complete_rx: &RefCell<VecDeque<Vec<u8>>>,
        notify_cid: xous::CID,
    ) {
        let new_frames = {
            let mut q = complete_rx.borrow_mut();
            let before = q.len();
            drain_complete_frames(&mut g.rx_assembly, &mut *q);
            q.len() - before
        };
        if new_frames > 0 {
            xous::try_send_message(
                notify_cid,
                xous::Message::new_scalar(Opcode::IrqCcidRx.to_usize().unwrap(), 0, 0, 0, 0),
            )
            .ok();
        }
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
        writer.endpoint(&self.interrupt_in)?;
        Ok(())
    }

    fn reset(&mut self) {
        let mut g = self.inner.borrow_mut();
        g.rx_assembly.clear();
        g.tx_buf.clear();
        g.tx_pending = false;
        self.complete_rx.borrow_mut().clear();
    }

    fn poll(&mut self) { self.poll_bulk_in(); }

    fn endpoint_out(&mut self, addr: EndpointAddress) {
        if addr != self.bulk_out.address() {
            return;
        }
        let mut tmp = vec![0u8; CCID_BULK_MAX_PACKET as usize];
        if let Ok(n) = self.bulk_out.read(&mut tmp) {
            if n == 0 {
                return;
            }
            let mut g = self.inner.borrow_mut();
            if append_bulk_out(&mut g.rx_assembly, &tmp[..n]).is_err() {
                return;
            }
            Self::drain_complete_messages(&mut g, &*self.complete_rx, self.notify_cid);
        }
    }
}
