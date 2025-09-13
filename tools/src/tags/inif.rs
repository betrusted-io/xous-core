use std::fmt;
use std::io;

use crate::elf::MiniElfFlags;
use crate::elf::MiniElfSection;
use crate::xous_arguments::{XousArgument, XousArgumentCode, XousSize};

#[derive(Debug)]
pub struct IniF {
    /// Address in flash
    load_offset: u32,

    /// Virtual address entry point
    entrypoint: u32,

    /// Array of minielf sections
    sections: Vec<MiniElfSection>,

    /// Actual program data
    data: Vec<u8>,

    /// Alignment offset
    alignment_offset: usize,
}

impl fmt::Display for IniF {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "    IniF: entrypoint @ {:08x}, loaded from {:08x}.  Sections:",
            self.entrypoint, self.load_offset
        )?;
        let mut load_offset = self.load_offset;
        for section in &self.sections {
            writeln!(f, "        Loaded from {:08x} - {}", load_offset, section)?;
            load_offset += section.size as u32;
        }
        writeln!(
            f,
            "        Size on disk: {} ({}kiB)",
            load_offset - self.load_offset,
            (load_offset - self.load_offset) / 1024
        )?;
        Ok(())
    }
}

impl IniF {
    pub fn new(
        entrypoint: u32,
        sections: Vec<MiniElfSection>,
        mut data: Vec<u8>,
        alignment_offset: usize,
    ) -> IniF {
        // pad the data to 4 bytes
        while data.len() & 3 != 0 {
            data.push(0);
        }
        IniF { load_offset: 0, entrypoint, sections, data, alignment_offset }
    }
}

impl XousArgument for IniF {
    fn code(&self) -> XousArgumentCode { u32::from_le_bytes(*b"IniF") }

    fn length(&self) -> XousSize { 4 + 4 + (self.sections.len() * 8) as XousSize }

    fn finalize(&mut self, offset: usize) -> usize {
        self.load_offset = offset as u32;

        assert!(offset % crate::tags::PAGE_SIZE == self.alignment_offset, "IniF load offset is not aligned");
        self.data = crate::tags::align_data_up(&self.data, 0);
        self.data.len()
    }

    fn last_data(&self) -> &[u8] { &self.data }

    fn alignment_offset(&self) -> usize { self.alignment_offset }

    fn serialize(&self, output: &mut dyn io::Write) -> io::Result<usize> {
        let mut written = 0;
        written += output.write(&self.load_offset.to_le_bytes())?;
        written += output.write(&self.entrypoint.to_le_bytes())?;
        let mut flash_offset = self.load_offset;
        for section in &self.sections {
            log::debug!("section.virt: {:x} <> flash_offset: {:x}", section.virt, flash_offset);
            // Check that the padding fix worked
            if section.flags & MiniElfFlags::NOCOPY == MiniElfFlags::NONE {
                // Only modestly informative panic, because we're deep in a serialize
                // routine and we don't have metadata about the process or anything:
                // But if these offsets don't line up, it's definitely a problem.
                assert!(
                    section.virt & 0xFFF == flash_offset & 0xFFF,
                    "IniF page offsets must align: ({:x}){:x} <> ({:x}){:x} ({:x?})",
                    section.virt >> 12,
                    section.virt & 0xFFF,
                    flash_offset >> 12,
                    flash_offset & 0xFFF,
                    section
                );
            }
            written += output.write(&section.virt.to_le_bytes())?;
            let mut word2 = section.size.to_le_bytes();
            word2[3] = section.flags.bits();
            written += output.write(&word2)?;
            flash_offset += section.size;
        }
        Ok(written)
    }
}
