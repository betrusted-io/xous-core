use packing::Packed;

use crate::scsi::Error;

pub trait ParsePackedStruct: Packed 
where
    Error: From<<Self as Packed>::Error>,
{
    fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut ret = Self::unpack(data)?;
        ret.verify()?;
        Ok(ret)
    }
    fn verify(&mut self) -> Result<(), Error> {
        Ok(())
    }
}
