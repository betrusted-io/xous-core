use std::fmt;
use std::io;

use crate::xous_arguments::{XousArgument, XousArgumentCode, XousSize};

#[derive(Debug, Default)]
pub struct Bflg {
    /// Disable copying data
    no_copy_: bool,

    /// Addresses are all absolute
    absolute_: bool,

    /// Set the SUM bit in $mstatus to allow Supervisor to access User memory
    debug_: bool,
}

impl fmt::Display for Bflg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "    Bflg:")?;
        if self.no_copy_ {
            write!(f, " +NO_COPY")?;
        } else {
            write!(f, " -no_copy")?;
        }
        if self.absolute_ {
            write!(f, " +ABSOLUTE")?;
        } else {
            write!(f, " -absolute")?;
        }

        if self.debug_ {
            write!(f, " +DEBUG")?;
        } else {
            write!(f, " -debug")?;
        }
        writeln!(f)
    }
}

impl Bflg {
    pub fn new() -> Bflg {
        Default::default()
        // Bflg {
        //     no_copy_: false,
        //     absolute_: false,
        //     debug_: false,
        // }
    }

    pub fn no_copy(mut self) -> Bflg {
        self.no_copy_ = true;
        self
    }

    pub fn absolute(mut self) -> Bflg {
        self.absolute_ = true;
        self
    }

    pub fn debug(mut self) -> Bflg {
        self.debug_ = true;
        self
    }
}

impl XousArgument for Bflg {
    fn code(&self) -> XousArgumentCode { u32::from_le_bytes(*b"Bflg") }

    fn length(&self) -> XousSize { 4 }

    fn serialize(&self, output: &mut dyn io::Write) -> io::Result<usize> {
        let mut written = 0;
        let mut val = 0u32;
        if self.no_copy_ {
            val |= 1;
        }
        if self.absolute_ {
            val |= 1 << 1;
        }
        if self.debug_ {
            val |= 1 << 2;
        }
        written += output.write(&val.to_le_bytes())?;
        Ok(written)
    }
}
