use packing::Packed;

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(big_endian, lsb0)]
pub struct ReadCapacity10Response {
    #[pkd(7, 0, 0, 3)]
    pub max_lba: u32,

    #[pkd(7, 0, 4, 7)]
    pub block_size: u32,
}