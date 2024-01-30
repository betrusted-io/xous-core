// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

use packing::Packed;
use crate::scsi::{
    packing::ParsePackedStruct,
    commands::{
        Control,
        CommandLength,
    },
    enums::PageControl,
};


/* After a logical unit reset, the device server shall respond in the following manner:
a) if default values are requested, report the default values;
b) if saved values are requested, report valid restored mode parameters, or restore the mode parameters and
report them. If the saved values of the mode parameters are not able to be accessed from the nonvolatile
vendor specific location, the command shall be terminated with CHECK CONDITION status, with the
sense key set to NOT READY. If saved parameters are not implemented, respond as defined in 6.11.5; or
c) if current values are requested and the current values have been sent by the application client via a MODE
SELECT command, the current values shall be returned. If the current values have not been sent, the
device server shall return:
A) the saved values, if saving is implemented and saved values are available; or
B) the default values.
*/

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub struct ModeSenseXCommand {
    pub command_length: CommandLength,
    pub page_control: PageControl
}

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(big_endian, lsb0)]
pub struct ModeSense6Command {
    #[pkd(7, 0, 0, 0)]
    pub op_code: u8,

    #[pkd(3, 3, 1, 1)]
    pub disable_block_descriptors: bool,

    #[pkd(7, 6, 2, 2)]
    pub page_control: PageControl,

    #[pkd(5, 0, 2, 2)]
    pub page_code: u8,

    #[pkd(7, 0, 3, 3)]
    pub subpage_code: u8,

    #[pkd(7, 0, 4, 4)]
    pub allocation_length: u8,

    #[pkd(7, 0, 5, 5)]
    pub control: Control,
}
impl ParsePackedStruct for ModeSense6Command {}
impl From<ModeSense6Command> for ModeSenseXCommand {
    fn from(m: ModeSense6Command) -> Self {
        Self {
            command_length: CommandLength::C6,
            page_control: m.page_control,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(big_endian, lsb0)]
pub struct ModeSense10Command {
    #[pkd(7, 0, 0, 0)]
    pub op_code: u8,

    #[pkd(4, 4, 1, 1)]
    pub long_lba_accepted: bool,

    #[pkd(3, 3, 1, 1)]
    pub disable_block_descriptors: bool,

    #[pkd(7, 6, 2, 2)]
    pub page_control: PageControl,

    #[pkd(5, 0, 2, 2)]
    pub page_code: u8,

    #[pkd(7, 0, 3, 3)]
    pub subpage_code: u8,

    #[pkd(7, 0, 8, 9)]
    pub allocation_length: u16,

    #[pkd(7, 0, 10, 10)]
    pub control: Control,
}
impl ParsePackedStruct for ModeSense10Command {}
impl From<ModeSense10Command> for ModeSenseXCommand {
    fn from(m: ModeSense10Command) -> Self {
        Self {
            command_length: CommandLength::C10,
            page_control: m.page_control,
        }
    }
}