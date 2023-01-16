// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

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
