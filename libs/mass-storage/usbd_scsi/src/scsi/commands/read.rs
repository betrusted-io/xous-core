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
pub struct ReadXCommand {
    pub lba: u32,
    pub transfer_length: u32,
}


#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(big_endian, lsb0)]
pub struct Read6Command {
    #[pkd(7, 0, 0, 0)]
    pub op_code: u8,

    #[pkd(4, 0, 1, 3)]
    pub lba: u32,

    #[pkd(7, 0, 4, 4)]
    pub transfer_length: u8,

    #[pkd(7, 0, 5, 5)]
    pub control: Control,
}
impl ParsePackedStruct for Read6Command {}

impl From<Read6Command> for ReadXCommand {
    fn from(r: Read6Command) -> Self {
        Self {
            lba: r.lba.into(),
            transfer_length: r.transfer_length.into(),
        }
    }
}


#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(big_endian, lsb0)]
pub struct Read10Command {
    #[pkd(7, 0, 0, 0)]
    pub op_code: u8,

    #[pkd(7, 5, 1, 1)]
    pub rd_protect: u8,

    #[pkd(4, 4, 1, 1)]
    pub dpo: bool,

    #[pkd(3, 3, 1, 1)]
    pub fua: bool,

    #[pkd(1, 1, 1, 1)]
    pub fua_nv: bool,

    #[pkd(7, 0, 2, 5)]
    pub lba: u32,

    #[pkd(4, 0, 6, 6)]
    pub group_number: u8,

    #[pkd(7, 0, 7, 8)]
    pub transfer_length: u16,

    #[pkd(7, 0, 9, 9)]
    pub control: Control,
}
impl ParsePackedStruct for Read10Command {}
impl From<Read10Command> for ReadXCommand {
    fn from(r: Read10Command) -> Self {
        Self {
            lba: r.lba.into(),
            transfer_length: r.transfer_length.into(),
        }
    }
}


#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(big_endian, lsb0)]
pub struct Read12Command {
    #[pkd(7, 0, 0, 0)]
    pub op_code: u8,

    #[pkd(7, 5, 1, 1)]
    pub rd_protect: u8,

    #[pkd(4, 4, 1, 1)]
    pub dpo: bool,

    #[pkd(3, 3, 1, 1)]
    pub fua: bool,

    #[pkd(1, 1, 1, 1)]
    pub fua_nv: bool,

    #[pkd(7, 0, 2, 5)]
    pub lba: u32,

    #[pkd(7, 0, 6, 9)]
    pub transfer_length: u32,

    #[pkd(4, 0, 10, 10)]
    pub group_number: u8,

    #[pkd(7, 0, 11, 11)]
    pub control: Control,
}
impl ParsePackedStruct for Read12Command {}
impl From<Read12Command> for ReadXCommand {
    fn from(r: Read12Command) -> Self {
        Self {
            lba: r.lba.into(),
            transfer_length: r.transfer_length.into(),
        }
    }
}




#[test]
fn test_read10_parse() {
    let data = [0, 0, 0, 0x1E, 0x80, 0, 0, 0x8, 0, 0, 0, 0, 0, 0, 0];
    let cmd = Read10Command::parse(&data).unwrap();
    assert_eq!(cmd.lba, 0x1E80);
}