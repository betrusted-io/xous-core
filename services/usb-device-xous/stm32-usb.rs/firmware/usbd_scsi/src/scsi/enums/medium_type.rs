use packing::Packed;

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
pub enum MediumType {
    Sbc = 0x00,
}

impl Default for MediumType {
    fn default() -> Self {
        MediumType::Sbc
    }
}