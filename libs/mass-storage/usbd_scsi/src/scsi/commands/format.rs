use packing::Packed;
use crate::scsi::{
    packing::ParsePackedStruct,
    commands::Control,
};

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(big_endian, lsb0)]
pub struct FormatCommand {
    #[pkd(7, 0, 0, 0)]
    pub op_code: u8,
    
    #[pkd(7, 6, 1, 1)]
    pub format_protection_information: u8,
    
    #[pkd(5, 5, 1, 1)]
    pub long_list: bool,
    
    #[pkd(4, 4, 1, 1)]
    pub format_data: bool,
    
    #[pkd(3, 3, 1, 1)]
    pub complete_list: bool,
    
    #[pkd(2, 0, 1, 1)]
    pub defect_list_format: u8,
            
    #[pkd(7, 0, 5, 5)]
    pub control: Control,
}
impl ParsePackedStruct for FormatCommand {}