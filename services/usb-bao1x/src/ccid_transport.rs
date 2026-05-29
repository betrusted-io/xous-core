// SPDX-License-Identifier: GPL-3.0-only
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

pub(crate) const CCID_BULK_MAX_PACKET: u16 = 512;
const CCID_INTERRUPT_MAX_PACKET: u16 = 8;
const CCID_INTERRUPT_INTERVAL_MS: u8 = 24;

pub(crate) const CCID_WIRE_MAX: usize = 530;

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

fn remove_tx_prefix(tx: &mut Vec<u8>, n: usize) {
    if n >= tx.len() {
        tx.clear();
        return;
    }
    tx.drain(..n);
}

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
            bulk_out: alloc.bulk(CCID_BULK_MAX_PACKET),
            bulk_in: alloc.bulk(CCID_BULK_MAX_PACKET),
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
        loop {
            if g.rx_assembly.len() < 10 {
                break;
            }
            let dw_len =
                u32::from_le_bytes([g.rx_assembly[1], g.rx_assembly[2], g.rx_assembly[3], g.rx_assembly[4]])
                    as usize;
            let total = 10usize.saturating_add(dw_len);
            if total > CCID_WIRE_MAX {
                g.rx_assembly.clear();
                break;
            }
            if g.rx_assembly.len() < total {
                break;
            }
            let frame: Vec<u8> = g.rx_assembly[..total].to_vec();
            g.rx_assembly.drain(..total);

            complete_rx.borrow_mut().push_back(frame);
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
            if !g.tx_pending || g.tx_buf.is_empty() {
                return;
            }
            let chunk_len = g.tx_buf.len().min(CCID_BULK_MAX_PACKET as usize);
            g.tx_buf[..chunk_len].to_vec()
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
            if g.rx_assembly.len().saturating_add(n) > CCID_WIRE_MAX {
                g.rx_assembly.clear();
                return;
            }
            g.rx_assembly.extend_from_slice(&tmp[..n]);
            Self::drain_complete_messages(&mut g, &*self.complete_rx, self.notify_cid);
        }
    }
}
