use std::collections::BTreeMap;
use std::fmt;
use std::io;

use crate::xous_arguments::{XousArgument, XousArgumentCode, XousSize};

#[derive(Debug)]
pub struct ProcessNames {
    /// A vec of all known process names
    names: BTreeMap<u32, String>,
}

impl fmt::Display for ProcessNames {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "    process names:")?;
        for (pid, name) in self.names.iter() {
            writeln!(f, "        PID {}: {}", pid, name)?;
        }
        Ok(())
    }
}

impl ProcessNames {
    pub fn new() -> ProcessNames { ProcessNames { names: BTreeMap::new() } }

    pub fn set(&mut self, pid: u32, name: &str) { self.names.insert(pid, name.to_owned()); }
}

impl XousArgument for ProcessNames {
    fn code(&self) -> XousArgumentCode { u32::from_le_bytes(*b"PNam") }

    fn length(&self) -> XousSize {
        let mut size = 0;
        for val in self.names.values() {
            size += 4;
            size += 4;
            size += val.len();
            // Pad it to 4-bytes
            size += (4 - (val.len() & 3)) & 3;
        }
        size as XousSize
    }

    fn serialize(&self, output: &mut dyn io::Write) -> io::Result<usize> {
        let mut written = 0;
        for (pid, name) in self.names.iter() {
            written += output.write(&(*pid as u32).to_le_bytes())?;
            written += output.write(&(name.len() as u32).to_le_bytes())?;
            written += output.write(name.as_bytes())?;

            // Pad it to 4-bytes
            for _ in 0..(4 - (name.len() & 3)) & 3 {
                written += output.write(&[0])?;
            }
        }
        Ok(written)
    }
}
