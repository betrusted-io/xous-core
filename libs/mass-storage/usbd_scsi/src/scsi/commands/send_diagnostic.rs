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