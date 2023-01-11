// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

use packing::Packed;

/// SCSI op codes as defined by SPC-3
#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
pub enum OpCode {
    TestUnitReady = 0x00,
    RequestSense = 0x03,
    Format = 0x04,
    Read6 = 0x08,
    Write6 = 0x0A,
    Inquiry = 0x12,
    ReadCapacity10 = 0x25,
    Read10 = 0x28,
    SendDiagnostic = 0x1D,
    ReportLuns = 0xA0,

    ModeSense6 = 0x1A,
    ModeSense10 = 0x5A,

    ModeSelect6 = 0x15,
    StartStopUnit = 0x1B,
    PreventAllowMediumRemoval = 0x1E,
    ReadFormatCapacities = 0x23,
    Write10 = 0x2A,
    Verify10 = 0x2F,
    SynchronizeCache10 = 0x35,
    ReadTocPmaAtip = 0x43,
    ModeSelect10 = 0x55,
    Read12 = 0xA8,
    Write12 = 0xAA,
}