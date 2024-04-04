use std::fmt;
use std::io;

use crate::xous_arguments::{XousArgument, XousArgumentCode, XousSize};

#[derive(Debug, Default, Clone, Copy)]
pub struct Swap {
    /// Starting offset (in bytes)
    pub offset: u32,

    /// Length (in bytes)
    pub size: u32,

    /// Region name (as a type)
    pub name: XousArgumentCode,

    /// Unused
    _padding: u32,
}

impl fmt::Display for Swap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "    Swap: {:08x} - {:08x}", self.offset, self.offset + self.size)?;
        Ok(())
    }
}

impl Swap {
    pub fn new(offset: XousSize, size: XousSize) -> Self {
        Swap { offset, size, name: u32::from_le_bytes(*b"Swap") as XousArgumentCode, _padding: 0 }
    }
}

impl XousArgument for Swap {
    fn code(&self) -> XousArgumentCode { self.name }

    fn length(&self) -> XousSize { 16 as XousSize }

    fn serialize(&self, output: &mut dyn io::Write) -> io::Result<usize> {
        let mut written = 0;
        output.write_all(&self.offset.to_le_bytes())?;
        output.write_all(&self.size.to_le_bytes())?;
        output.write_all(&self.name.to_le_bytes())?;
        output.write_all(&0u32.to_le_bytes())?;
        written += 4 * 4;
        Ok(written)
    }
}