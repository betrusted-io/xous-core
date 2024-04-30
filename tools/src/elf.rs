use std::fmt;
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;

use bitflags::bitflags;
use log::debug;
use xmas_elf::program::Type as ProgramType;
use xmas_elf::sections::ShType;
// Normal ELF flags
use xmas_elf::sections::{SHF_ALLOC, SHF_EXECINSTR, SHF_WRITE};
use xmas_elf::ElfFile;

bitflags! {
    pub struct MiniElfFlags: u8 {
        const NONE = 0;
        const WRITE = 1;
        const NOCOPY = 2;
        const EXECUTE = 4;
        const EH_FRAME = 8;
        const EH_HEADER = 0x10;
    }
}

pub struct ProgramDescription {
    /// Virtual address of .text section in RAM
    pub text_offset: u32,

    /// Size of the .text section in RAM
    pub text_size: u32,

    /// Virtual address of .data section in RAM
    pub data_offset: u32,

    /// Size of .data section
    pub data_size: u32,

    /// Size of the .bss section
    pub bss_size: u32,

    /// Virtual address of the entrypoint
    pub entry_point: u32,

    /// Program contents
    pub program: Vec<u8>,
}

#[derive(Debug)]
pub struct MiniElfSection {
    pub virt: u32,
    pub size: u32,
    pub flags: MiniElfFlags,
    pub name: String,
}

impl fmt::Display for MiniElfSection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Section {:13} {:6} bytes loading into {:08x}..{:08x} flags: {:?}",
            self.name,
            self.size,
            self.virt,
            self.virt + self.size,
            self.flags
        )
    }
}

/// Describes a Mini ELF file, suitable for loading into RAM
pub struct MiniElf {
    /// Virtual address of the entrypoint
    pub entry_point: u32,

    /// All of the sections inside this file
    pub sections: Vec<MiniElfSection>,

    /// Actual section data
    pub program: Vec<u8>,

    /// Alignment offset for page mapping
    pub alignment_offset: usize,
}

#[derive(Debug)]
pub enum ElfReadError {
    /// Read an unexpected number of bytes
    WrongReadSize(u64 /* expected */, u64 /* actual */),

    /// "Couldn't seek to end of file"
    SeekFromEndError(std::io::Error),

    /// Couldn't read ELF file
    ReadFileError(std::io::Error),

    /// Couldn't open the ELF file
    OpenElfError(std::io::Error),

    /// Couldn't parse the ELF file
    ParseElfError(&'static str),

    /// Section wasn't in range
    SectionRangeError,

    /// Section wasn't word-aligned
    SectionNotAligned(String /* section name */, usize /* section size */),

    /// Couldn't seek the file to write the section
    FileSeekError(std::io::Error),

    /// Couldn't write the section to the file
    WriteSectionError(std::io::Error),
}

impl fmt::Display for ElfReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ElfReadError::*;
        match self {
            WrongReadSize(e, a) => {
                write!(f, "expected to read {} bytes, but instead read {}", e, a)
            }
            SeekFromEndError(e) => write!(f, "couldn't seek from the end of the file: {}", e),
            ReadFileError(e) => write!(f, "couldn't read from the file: {}", e),
            OpenElfError(e) => write!(f, "couldn't open the elf file: {}", e),
            ParseElfError(e) => write!(f, "couldn't parse the elf file: {}", e),
            SectionRangeError => write!(f, "elf section pointed outside of the file"),
            SectionNotAligned(s, a) => write!(f, "elf section {} had unaligned length {}", s, a),
            FileSeekError(e) => write!(f, "couldn't seek in the output file: {}", e),
            WriteSectionError(e) => write!(f, "couldn't write a section to the output file: {}", e),
        }
    }
}

#[allow(clippy::cognitive_complexity)]
pub fn read_program<P: AsRef<Path>>(filename: P) -> Result<ProgramDescription, ElfReadError> {
    let mut b = Vec::new();
    {
        let mut fi = File::open(filename).map_err(ElfReadError::OpenElfError)?;
        fi.read_to_end(&mut b).map_err(ElfReadError::ReadFileError)?;
    }
    process_program(&b)
}

pub fn process_program(b: &[u8]) -> Result<ProgramDescription, ElfReadError> {
    let elf = ElfFile::new(&b).map_err(|x| ElfReadError::ParseElfError(x))?;
    let entry_point = elf.header.pt2.entry_point() as u32;
    let mut program_data = Cursor::new(Vec::new());

    let mut size = 0;
    let mut data_offset = 0;
    let mut data_size = 0;
    let mut text_offset = 0;
    let mut text_size = 0;
    let mut bss_size = 0;
    let mut phys_offset = 0;

    debug!("ELF: {:?}", elf.header);
    for ph in elf.program_iter() {
        debug!("Program Header: {:?}", ph);
        if ph.get_type() == Ok(ProgramType::Load) && phys_offset == 0 {
            phys_offset = ph.physical_addr();
        }
        debug!("Physical address: {:08x}", ph.physical_addr());
        debug!("Virtual address: {:08x}", ph.virtual_addr());
        debug!("Offset: {:08x}", ph.offset());
        debug!("Size: {:08x}", ph.file_size());
    }
    debug!("Program starts at 0x{:x}", entry_point);

    let mut program_offset = 0;
    for s in elf.section_iter() {
        let name = s.get_name(&elf).unwrap_or("<<error>>");

        if s.address() == 0 {
            debug!("(Skipping section {} -- invalid address)", name);
            continue;
        }

        debug!("Section {}:", name);
        debug!("Official header:");
        debug!("{:?}", s);
        debug!("Interpreted:");
        debug!("    flags:            {:?}", s.flags());
        debug!("    type:             {:?}", s.get_type());
        debug!("    address:          {:08x}", s.address());
        debug!("    offset:           {:08x}", s.offset());
        debug!("    size:             {:?}", s.size());
        debug!("    link:             {:?}", s.link());
        size += s.size();
        // Pad the section so it's a multiple of 4 bytes.
        // It's unclear if this is necessary, since this seems to indicate
        // that something has gone horribly wrong.
        size += (4 - (size & 3)) & 3;
        if size & 3 != 0 {
            return Err(ElfReadError::SectionNotAligned(name.to_owned(), s.size() as usize));
        }

        if name == ".data" {
            data_offset = s.address() as u32;
            data_size += s.size() as u32;
        } else if s.get_type() == Ok(ShType::NoBits) {
            // Add bss-type sections to the data section
            bss_size += s.size() as u32;
            debug!("Skipping copy of {} @ {:08x} because nobits", name, s.address());
            continue;
        } else if text_offset == 0 && (s.address() != 0 || s.size() != 0) {
            text_offset = s.address() as u32;
            text_size += s.size() as u32;
        } else {
            if text_offset + text_size != s.address() as u32 {
                let bytes_to_add = s.address() - (text_offset + text_size) as u64;
                debug!("Padding text size by {} bytes...", bytes_to_add);
                program_data
                    .seek(SeekFrom::Current(bytes_to_add as i64))
                    .map_err(ElfReadError::FileSeekError)?;
                text_size += bytes_to_add as u32;
                program_offset += bytes_to_add as u64;
                // panic!(
                //     "size not correct!  should be {:08x}, was {:08x}, need to add {} bytes",
                //     text_offset + text_size,
                //     s.address(),
                //     s.address() - (text_offset + text_size) as u64,
                // );
            }
            text_size += s.size() as u32;
        }
        if s.size() == 0 {
            debug!("Skipping {} because size is 0", name);
            continue;
        }
        debug!("Adding {} to the file", name);
        debug!(
            "s offset: {:08x}  program_offset: {:08x}  Bytes: {}  seek: {}",
            s.offset(),
            program_offset,
            s.raw_data(&elf).len(),
            program_offset
        );
        let section_data = s.raw_data(&elf);
        debug!(
            "Section start: {:02x} {:02x} {:02x} {:02x} going into offset 0x{:08x}",
            section_data[0], section_data[1], section_data[2], section_data[3], program_offset
        );
        program_data.seek(SeekFrom::Start(program_offset)).map_err(ElfReadError::FileSeekError)?;
        program_data.write(section_data).map_err(ElfReadError::WriteSectionError)?;
        program_offset += section_data.len() as u64;
    }
    let observed_size = program_data.seek(SeekFrom::End(0)).map_err(ElfReadError::SeekFromEndError)?;

    debug!("Text size: {} bytes", text_size);
    debug!("Text offset: {:08x}", text_offset);
    debug!("Data size: {} bytes", data_size);
    debug!("Data offset: {:08x}", data_offset);
    debug!("Program size: {} bytes", observed_size);
    Ok(ProgramDescription {
        entry_point,
        program: program_data.into_inner(),
        data_size,
        data_offset,
        text_offset,
        text_size,
        bss_size,
    })
}

/// Read an ELF file into a mini ELF file.
#[allow(clippy::cognitive_complexity)]
pub fn read_minielf<P: AsRef<Path>>(filename: P) -> Result<MiniElf, ElfReadError> {
    let mut b = Vec::new();
    {
        let mut fi = File::open(filename).map_err(ElfReadError::OpenElfError)?;
        fi.read_to_end(&mut b).map_err(ElfReadError::ReadFileError)?;
    }
    process_minielf(&b)
}

pub fn process_minielf(b: &[u8]) -> Result<MiniElf, ElfReadError> {
    let elf = ElfFile::new(&b).map_err(|x| ElfReadError::ParseElfError(x))?;
    let entry_point = elf.header.pt2.entry_point() as u32;
    let mut program_data = Cursor::new(Vec::new());
    let mut alignment_offset = 0;

    let mut sections = vec![];

    debug!("ELF: {:?}", elf.header);
    for ph in elf.program_iter() {
        debug!("Program Header: {:?}", ph);
        debug!("Physical address: {:08x}", ph.physical_addr());
        debug!("Virtual address: {:08x}", ph.virtual_addr());
        debug!("Offset: {:08x}", ph.offset());
        debug!("Size: {:08x}", ph.file_size());
    }
    debug!("Program starts at 0x{:x}", entry_point);

    // This keeps a running offset of where data is getting copied.
    let mut program_offset = 0;
    let mut section_iter = elf.section_iter().peekable();
    let mut init_offset = 0;
    while let Some(s) = section_iter.next() {
        let mut flags = MiniElfFlags::NONE;
        let name = s.get_name(&elf).unwrap_or("<<error>>");

        if s.address() == 0 {
            debug!("(Skipping section {} -- invalid address)", name);
            // only extract the initial offset once per ELF file
            if init_offset == 0 {
                init_offset = if let Some(s) = section_iter.peek() { s.offset() } else { 0 };
            }
            continue;
        }
        if alignment_offset == 0 {
            alignment_offset = s.address() & 0xFFF;
        }

        debug!("Section {}:", name);
        debug!("{} official header: {:x?}", name, s);
        debug!("Interpreted:");
        debug!("    flags:            {:?}", s.flags());
        debug!("    type:             {:?}", s.get_type());
        debug!("    address:          {:08x}", s.address());
        debug!("    offset:           {:08x}", s.offset());
        debug!("    size:             {:?}", s.size());
        debug!("    link:             {:?}", s.link());
        let mut size = s.size();

        let no_copy = s.get_type() == Ok(ShType::NoBits);

        if s.flags() & SHF_ALLOC == 0 {
            debug!("section has no allocations -- skipping");
            continue;
        }
        if no_copy {
            flags |= MiniElfFlags::NOCOPY;
        }
        if s.flags() & SHF_EXECINSTR != 0 {
            flags |= MiniElfFlags::EXECUTE;
        }
        if s.flags() & SHF_WRITE != 0 {
            flags |= MiniElfFlags::WRITE;
        }
        if name == ".eh_frame_hdr" {
            flags |= MiniElfFlags::EH_HEADER
        } else if name == ".eh_frame" {
            flags |= MiniElfFlags::EH_FRAME;
        }

        debug!("Adding {} to the file", name);
        debug!(
            "{} offset: {:08x}  program_offset: {:08x}  bytes: {}  seek: {}",
            name,
            s.offset(),
            program_offset,
            if no_copy { 0 } else { s.raw_data(&elf).len() },
            program_offset
        );

        // If this section gets copied, add it to the program stream.
        if s.get_type() != Ok(ShType::NoBits) {
            let section_data = s.raw_data(&elf);
            let pad_amount = if let Some(next_section) = section_iter.peek() {
                if (section_data.len() + program_offset as usize + init_offset as usize)
                    % next_section.align() as usize
                    != 0
                {
                    let pad_amount = next_section.align() as usize
                        - ((section_data.len() + program_offset as usize + init_offset as usize)
                            % next_section.align() as usize);
                    if s.address() + size + pad_amount as u64 > next_section.address() {
                        (next_section.address() - (s.address() + size)) as usize
                    } else {
                        pad_amount
                    }
                } else {
                    0
                }
            } else {
                0
            };

            debug!(
                "Section start: {:02x} {:02x} {:02x} {:02x} going into offset 0x{:08x}",
                section_data[0], section_data[1], section_data[2], section_data[3], program_offset
            );
            program_data.seek(SeekFrom::Start(program_offset)).map_err(ElfReadError::FileSeekError)?;
            program_data.write(section_data).map_err(ElfReadError::WriteSectionError)?;
            program_offset += section_data.len() as u64;

            if pad_amount != 0 {
                let pad = vec![0u8; pad_amount];
                program_data.write(&pad).map_err(ElfReadError::WriteSectionError)?;
                program_offset += pad_amount as u64;
                size += pad_amount as u64;
            }
        } else {
            // we leave the nocopy sections mis-aligned.
            // They don't exist in the file, they are just zero'd on spec.
            // This works so long as the nocopy sections are at the end of the MiniElf.
        }
        sections.push(MiniElfSection {
            virt: s.address() as u32,
            size: size as u32,
            name: name.to_string(),
            flags,
        });
    }
    let observed_size = program_data.seek(SeekFrom::End(0)).map_err(ElfReadError::SeekFromEndError)?;

    debug!("Program size: {} bytes", observed_size);
    Ok(MiniElf {
        entry_point,
        sections,
        program: program_data.into_inner(),
        alignment_offset: alignment_offset as usize,
    })
}
