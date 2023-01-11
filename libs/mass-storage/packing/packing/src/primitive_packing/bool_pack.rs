use crate::{
    Endian,
    PackedBytes,
    PackedSize,
};
use core::convert::Infallible;

impl PackedSize for bool {
    const BYTES: usize = 1;
}

impl PackedBytes<[u8; Self::BYTES]> for bool {
    type Error = Infallible;
    fn to_bytes<En: Endian>(&self) -> Result<[u8; Self::BYTES], Self::Error> {
        Ok(if *self {
           [1]
        } else {
           [0]
        })
    }
    fn from_bytes<En: Endian>(bytes: [u8; Self::BYTES]) -> Result<Self, Self::Error> {
        Ok(bytes[0] == 1)
    }
}