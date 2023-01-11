use crate::{
    Endian,
    PackedBytes,
    PackedSize,
};
use core::convert::Infallible;

impl PackedSize for f64 {
    const BYTES: usize = 8;
}

impl PackedBytes<[u8; Self::BYTES]> for f64 {
    type Error = Infallible;
    fn to_bytes<En: Endian>(&self) -> Result<[u8; Self::BYTES], Self::Error> {
        Ok(if En::IS_LITTLE {
            self.to_le_bytes()
        } else {
            self.to_be_bytes()
        })
    }
    fn from_bytes<En: Endian>(bytes: [u8; Self::BYTES]) -> Result<Self, Self::Error> {
        Ok(if En::IS_LITTLE {
            Self::from_le_bytes(bytes)
        } else {
            Self::from_be_bytes(bytes)
        })
    }
}
