use packing::Packed;

/// This is the last byte on all commands
#[derive(Clone, Copy, Eq, PartialEq, Debug, Default, Packed)]
#[packed(big_endian, lsb0)]
pub struct Control {
    #[pkd(7, 6, 0, 0)]
    pub vendor_specific: u8,
    
    #[pkd(2, 2, 0, 0)]
    pub normal_aca: bool,
}