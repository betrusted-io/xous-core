// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

use packing::Packed;

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
pub enum SpcVersion {
    //The device server does not claim conformance to any standard.
    None = 0x00,
    //The device server complies to ANSI INCITS 351-2001 (SPC-2).
    Spc2 = 0x04,
    //The device server complies to ANSI INCITS 408-2005 (SPC-3).
    Spc3 = 0x05,
    //The device server complies to SPC-4.
    Spc4 = 0x06,
}
impl Default for SpcVersion {
    fn default() -> Self {
        SpcVersion::Spc4
    }
}