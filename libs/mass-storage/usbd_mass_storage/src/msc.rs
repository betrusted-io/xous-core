// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

use usb_device::{
    class_prelude::*,
    Result,
};

use crate::{
    InterfaceSubclass,
    InterfaceProtocol,
    logging::*,
};

/// This should be used as `device_class` when building `UsbDevice`
///
/// Section 4.3 [USB Bulk Only Transport Spec](https://www.usb.org/document-library/mass-storage-bulk-only-10)
pub const USB_CLASS_MSC: u8 = 0x08;

/// # USB Mass Storage Class Device
///
/// So far only tested with the Bulk Only protocol and the SCSI transparent command set - see
/// [Scsi](struct.Scsi.html) and [Bulk Only Transport](struct.BulkOnlyTransport.html)
pub struct MscClass<'a, B: UsbBus> {
    pub(crate) msc_if: InterfaceNumber,
    pub(crate) read_ep: EndpointOut<'a, B>,
    pub(crate) write_ep: EndpointIn<'a, B>,
    pub(crate) write2_ep: EndpointIn<'a, B>,
    pub(crate) subclass: InterfaceSubclass,
    pub(crate) protocol: InterfaceProtocol,
}

impl<B: UsbBus> MscClass<'_, B> {
    pub fn new(
        alloc: &UsbBusAllocator<B>,
        max_packet_size: u16,
        subclass: InterfaceSubclass,
        protocol: InterfaceProtocol,
    ) -> MscClass<'_, B> {
        MscClass {
            msc_if: alloc.interface(),
            write_ep: alloc.bulk(max_packet_size),
            write2_ep: alloc.interrupt(64, 5),
            read_ep: alloc.bulk(max_packet_size),
            subclass,
            protocol,
        }
    }

    pub fn max_packet_size(&self) -> u16 {
        // The size is the same for both endpoints.
        self.read_ep.max_packet_size()
    }

    pub fn read_packet(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.read_ep.read(buf)
    }

    pub fn write_packet(&mut self, buf: &[u8]) -> Result<usize> {
        self.write_ep.write(buf)
    }

    pub fn correct_interface_number(&self, interface_number: u16) -> bool {
         interface_number == u8::from(self.msc_if) as u16
    }
}

impl<B: UsbBus> UsbClass<B> for MscClass<'_, B> {
    fn get_configuration_descriptors(&self, writer: &mut DescriptorWriter) -> Result<()> {
        writer.interface(self.msc_if,
            USB_CLASS_MSC,
            self.subclass.to_primitive(),
            self.protocol.to_primitive(),
        )?;

        writer.endpoint(&self.write_ep)?;
        writer.endpoint(&self.read_ep)?;
        writer.endpoint(&self.write2_ep)
    }

    fn reset(&mut self) { }

    fn control_in(&mut self, xfer: ControlIn<B>) {
        let req = xfer.request();
        if self.correct_interface_number(req.index) {
           trace_usb_control!("USB_CONTROL> Unhandled control-IN: {:?}", req);
        }
    }

    fn control_out(&mut self, xfer: ControlOut<B>) {
        let req = xfer.request();
        if self.correct_interface_number(req.index) {
            trace_usb_control!("USB_CONTRO> Unhandled control-OUT: {:?}", req);
        };
    }
}