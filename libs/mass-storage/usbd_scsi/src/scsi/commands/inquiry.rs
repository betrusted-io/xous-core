// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

use packing::{
    Packed,
};
use crate::scsi::{
    packing::ParsePackedStruct,
    commands::Control,
};



#[derive(Clone, Copy, Eq, PartialEq, Debug, Default, Packed)]
#[packed(big_endian, lsb0)]
pub struct InquiryCommand {
    #[pkd(7, 0, 0, 0)]
    pub op_code: u8,

    /// If set, return vital data related to the page_code field
    #[pkd(0, 0, 1, 1)]
    pub enable_vital_product_data: bool,

    /// What kind of vital data to return
    #[pkd(7, 0, 2, 2)]
    pub page_code: u8,

    ///TODO: (check) Should match data_transfer_length in CBW
    #[pkd(7, 0, 3, 4)]
    pub allocation_length: u16,
    #[pkd(7, 0, 5, 5)]
    pub control: Control,
}
impl ParsePackedStruct for InquiryCommand {}



/*
 if evpd
    return data related to page_code (spc-4 section 7.8)
    if unsupported(page_code)
        return CHECK_CONDITION and set SENSE:
            key: ILLEGAL_REQUEST
            additional code: INVALID_FIELD_IN_CBD

 if !evpd
    return standard inquiry data (spc-4 section 6.4.2)
    if page_code != 0
        return CHECK_CONDITION and set SENSE:
            key: ILLEGAL_REQUEST
            additional code: INVALID_FIELD_IN_CBD
*/


#[test]
fn test_inquiry() {
    let mut bytes = [0; 5];
    let mut cmd = InquiryCommand::default();
    assert_eq!(cmd, InquiryCommand::unpack(&bytes).unwrap());

    bytes[0] |= 0b00000001;
    cmd.enable_vital_product_data = true;
    assert_eq!(cmd, InquiryCommand::unpack(&bytes).unwrap());

    bytes[1] = 0x99;
    cmd.page_code = 0x99;
    assert_eq!(cmd, InquiryCommand::unpack(&bytes).unwrap());

    let al = 9999;
    bytes[2] = ((al >> 8) & 0xFF) as u8;
    bytes[3] = ((al >> 0) & 0xFF) as u8;
    cmd.allocation_length = al;
    assert_eq!(cmd, InquiryCommand::unpack(&bytes).unwrap());

    bytes[4] = 0x31;
    cmd.control = 0x31;
    assert_eq!(cmd, InquiryCommand::unpack(&bytes).unwrap());
}