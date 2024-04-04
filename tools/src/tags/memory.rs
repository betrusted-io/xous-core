use std::fmt;
use std::io;

use crate::xous_arguments::{XousArgument, XousArgumentCode, XousSize};

/// Convert a four-letter string into a 32-bit int.
macro_rules! make_type {
    ($fcc:expr) => {{
        let mut c: [u8; 4] = Default::default();
        c.copy_from_slice($fcc.as_bytes());
        u32::from_le_bytes(c)
    }};
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MemoryRegion {
    /// Starting offset (in bytes)
    pub start: u32,

    /// Length (in bytes)
    pub length: u32,

    /// Region name (as a type)
    pub name: XousArgumentCode,

    /// Unused
    _padding: u32,
}

#[derive(Default)]
pub struct MemoryRegions {
    regions: Vec<MemoryRegion>,
}

impl fmt::Display for MemoryRegions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "    Additional regions:")?;
        for region in &self.regions {
            let tag_name_bytes = region.name.to_le_bytes();
            let tag_name_str = String::from_utf8_lossy(&tag_name_bytes);
            writeln!(
                f,
                "        {} ({:08x}): {:08x} - {:08x}",
                tag_name_str,
                region.name,
                region.start,
                region.start + region.length
            )?;
        }
        Ok(())
    }
}

impl MemoryRegion {
    pub fn new(start: XousSize, length: XousSize, name: u32) -> MemoryRegion {
        MemoryRegion { start, length, name, _padding: 0 }
    }

    pub fn make_name(name: &str) -> u32 {
        match name {
            "sram_ext" => u32::from_le_bytes(*b"SrEx"),
            "sram" => u32::from_le_bytes(*b"SrIn"),
            "memlcd" => u32::from_le_bytes(*b"Disp"),
            "vexriscv_debug" => u32::from_le_bytes(*b"VexD"),
            "csr" => u32::from_le_bytes(*b"CSRs"),
            "audio" => u32::from_le_bytes(*b"Audi"),
            "rom" => u32::from_le_bytes(*b"Boot"),
            "spiflash" => u32::from_le_bytes(*b"SpFl"),
            other => {
                let mut region_name = other.to_owned();
                region_name.push_str("    ");
                region_name.truncate(4);
                make_type!(region_name)
            }
        }
    }
}

impl MemoryRegions {
    pub fn new() -> MemoryRegions { Default::default() }

    pub fn add(&mut self, region: MemoryRegion) { self.regions.push(region) }

    pub fn len(&self) -> usize { self.regions.len() }

    pub fn is_empty(&self) -> bool { self.regions.is_empty() }

    #[allow(dead_code)]
    pub fn patch(&mut self, region_name: &str, new_spec: MemoryRegion) {
        if let Some(index) = self.regions.iter().position(|&x| x.name == MemoryRegion::make_name(region_name))
        {
            self.regions.remove(index);
            self.regions.insert(index, new_spec);
        }
    }
}

impl XousArgument for MemoryRegions {
    fn code(&self) -> XousArgumentCode { u32::from_le_bytes(*b"MREx") }

    fn length(&self) -> XousSize { (self.regions.len() * std::mem::size_of::<MemoryRegion>()) as XousSize }

    fn serialize(&self, output: &mut dyn io::Write) -> io::Result<usize> {
        let mut written = 0;
        for region in &self.regions {
            output.write_all(&region.start.to_le_bytes())?;
            output.write_all(&region.length.to_le_bytes())?;
            output.write_all(&region.name.to_le_bytes())?;
            output.write_all(&0u32.to_le_bytes())?;
            written += 4 * 4;
        }
        Ok(written)
    }
}
