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
pub struct ReadFormatCapacitiesCommand {
    #[pkd(7, 0, 0, 0)]
    pub op_code: u8,

    #[pkd(7, 5, 1, 1)]
    pub logical_unit_number: u8,

    #[pkd(7, 0, 7, 8)]
    pub allocation_length: u16,

    #[pkd(7, 0, 9, 9)]
    pub control: Control,
}
impl ParsePackedStruct for ReadFormatCapacitiesCommand {}

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(big_endian, lsb0)]
pub struct ReadFormatCapacitiesResponse {
    #[pkd(7, 0, 3, 3)]
    pub capacity_list_length: u8,

    #[pkd(7, 0, 4, 7)]
    pub number_of_blocks: u32,

    #[pkd(1, 0, 8, 8)]
    pub descriptor_code: u8,

    #[pkd(7, 0, 9, 11)]
    pub block_length: u32,
}
impl ParsePackedStruct for ReadFormatCapacitiesResponse {}