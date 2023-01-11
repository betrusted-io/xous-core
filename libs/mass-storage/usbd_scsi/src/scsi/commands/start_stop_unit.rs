use packing::Packed;
use crate::scsi::{
    packing::ParsePackedStruct,
    commands::Control,
};

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(big_endian, lsb0)]
pub struct StartStopUnitCommand {
    #[pkd(7, 0, 0, 0)]
    pub op_code: u8,
    
    #[pkd(0, 0, 1, 1)]
    pub immediate: bool,
    
    #[pkd(3, 0, 3, 3)]
    pub power_condition_modifier: u8,
    
    #[pkd(7, 4, 4, 4)]
    pub power_condition: u8,
    
    #[pkd(2, 2, 4, 4)]
    pub no_flush: bool,
    
    #[pkd(1, 1, 4, 4)]
    pub load_eject: bool,
    
    #[pkd(0, 0, 4, 4)]
    pub start: bool,
    
    #[pkd(7, 0, 5, 5)]
    pub control: Control,
}
impl ParsePackedStruct for StartStopUnitCommand {}