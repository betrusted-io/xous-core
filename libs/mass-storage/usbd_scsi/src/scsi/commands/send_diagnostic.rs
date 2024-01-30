// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

use packing::Packed;
use crate::scsi::{
    packing::ParsePackedStruct,
    commands::Control,
};

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(big_endian, lsb0)]
pub struct SendDiagnosticCommand {
    #[pkd(7, 0, 0, 0)]
    pub op_code: u8,

    #[pkd(7, 5, 1, 1)]
    pub self_test_code: u8,

    #[pkd(4, 4, 1, 1)]
    pub page_format: bool,

    #[pkd(2, 2, 1, 1)]
    pub self_test: bool,

    #[pkd(1, 1, 1, 1)]
    pub device_offline: bool,

    #[pkd(0, 0, 1, 1)]
    pub unit_offline: bool,

    #[pkd(7, 0, 3, 4)]
    pub parameter_list_length: u16,

    #[pkd(7, 0, 5, 5)]
    pub control: Control,
}
impl ParsePackedStruct for SendDiagnosticCommand {}