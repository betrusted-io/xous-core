// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

//! USB interface protocol
use packing::Packed;

/// This specifies the protocol of the USB interface
///
/// Section 3 [USB Mass Storage Class Overview](https://www.usb.org/document-library/mass-storage-class-specification-overview-14)
#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
pub enum InterfaceProtocol {
    /// USB Mass Storage Class Control/Bulk/Interrupt (CBI) Transport (with command completion interrupt)
    CbiWithCCInterrupt = 0x00,
    /// USB Mass Storage Class Control/Bulk/Interrupt (CBI) Transport (with no command completion interrupt)
    CbiNoCCInterrupt = 0x01,
    /// USB Mass Storage Class Bulk-Only (BBB) Transport
    BulkOnlyTransport= 0x50,
    /// Allocated by USB-IF for UAS. UAS is defined outside of USB
    Uas = 0x62,
    /// Specific to device vendor. De facto use
    VendorSpecific= 0xFF,
}