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

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub struct ModeSelectXCommand {
    // TBD
}

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(big_endian, lsb0)]
pub struct ModeSelect6Command {
    #[pkd(7, 0, 0, 0)]
    pub op_code: u8,

    #[pkd(4, 4, 1, 1)]
    pub page_format: bool,

    #[pkd(0, 0, 1, 1)]
    pub save_pages: bool,

    #[pkd(7, 0, 4, 4)]
    pub parameter_list_length: u8,

    #[pkd(7, 0, 5, 5)]
    pub control: Control,
}
impl ParsePackedStruct for ModeSelect6Command {}
impl From<ModeSelect6Command> for ModeSelectXCommand {
    fn from(_m: ModeSelect6Command) -> Self {
        Self { }
    }
}


#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(big_endian, lsb0)]
pub struct ModeSelect10Command {
    #[pkd(7, 0, 0, 0)]
    pub op_code: u8,

    #[pkd(4, 4, 1, 1)]
    pub page_format: bool,

    #[pkd(0, 0, 1, 1)]
    pub save_pages: bool,

    #[pkd(7, 0, 7, 8)]
    pub parameter_list_length: u16,

    #[pkd(7, 0, 9, 9)]
    pub control: Control,
}
impl ParsePackedStruct for ModeSelect10Command {}
impl From<ModeSelect10Command> for ModeSelectXCommand {
    fn from(_m: ModeSelect10Command) -> Self {
        Self { }
    }
}