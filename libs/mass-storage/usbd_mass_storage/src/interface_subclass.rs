// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

//! USB interface subclass
use packing::Packed;

/// This specifies the subclass of the USB interface
///
/// Section 2 [USB Mass Storage Class Overview](https://www.usb.org/document-library/mass-storage-class-specification-overview-14)
#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
pub enum InterfaceSubclass {
    /// SCSI command set not reported. De facto use
    ScsiCommandSetNotReported = 0x00,
    /// Allocated by USB-IF for RBC. RBC is defined outside of USB
    Rbc = 0x01,
    /// Allocated by USB-IF for MMC-5. MMC-5 is defined outside of USB
    Mmc5Atapi = 0x02,
    /// Specifies how to interface Floppy Disk Drives to USB
    Ufi = 0x04,
    /// Allocated by USB-IF for SCSI. SCSI standards are defined outside of USB
    ScsiTransparentCommandSet = 0x06,
    /// LSDFS specifies how host has to negotiate access before trying SCSI
    LsdFs = 0x07,
    /// Allocated by USB-IF for IEEE 1667. IEEE 1667 is defined outside of USB
    Ieee1667 = 0x08,
    /// Specific to device vendor. De facto use
    VendorSpecific = 0xFF,
}