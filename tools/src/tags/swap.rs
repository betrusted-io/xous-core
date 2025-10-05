use std::fmt;
use std::io;

use crate::xous_arguments::{XousArgument, XousArgumentCode, XousSize};

#[derive(Debug, Default, Clone, Copy)]
pub struct Swap {
    /// Starting offset (in bytes) of the swap RAM
    pub offset: u32,

    /// Total swap length (in bytes) - not all is usable, as a portion is reserved for the MAC codes
    pub size: u32,

    /// Region name (as a type)
    pub name: XousArgumentCode,

    /// Starting offset of swap in FLASH
    pub offset_flash: u32,
}

impl fmt::Display for Swap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "    Swap: {:08x} - {:08x}", self.offset, self.offset + self.size)?;
        Ok(())
    }
}

impl Swap {
    pub fn new(offset: XousSize, size: XousSize) -> Self {
        Swap {
            offset,
            size,
            name: u32::from_le_bytes(*b"Swap") as XousArgumentCode,
            // location for swap offset in FLASH (for precursor)
            offset_flash: 0x21200000,
        }
    }
}

impl XousArgument for Swap {
    fn code(&self) -> XousArgumentCode { self.name }

    fn length(&self) -> XousSize { size_of::<bao1x_api::signatures::SwapDescriptor>() as XousSize }

    fn serialize(&self, output: &mut dyn io::Write) -> io::Result<usize> {
        let mut descriptor = bao1x_api::signatures::SwapDescriptor::default();
        descriptor.ram_offset = self.offset;
        descriptor.ram_size = self.size;
        descriptor.name = self.name;
        descriptor.key.fill(0); // slot in the "dummy" key for new images - we could use anything, but it needs to be provided
        descriptor.flash_offset = self.offset_flash;

        output.write(descriptor.as_ref())?;
        Ok(descriptor.as_ref().len())
    }
}
