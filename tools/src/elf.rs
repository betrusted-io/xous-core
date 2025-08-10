use std::convert::TryInto;
use std::fmt;
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;

use bitflags::bitflags;
use log::debug;
use xmas_elf::ElfFile;
use xmas_elf::program::Type as ProgramType;
use xmas_elf::sections::ShType;
// Normal ELF flags
use xmas_elf::sections::{SHF_ALLOC, SHF_EXECINSTR, SHF_WRITE};

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
    process_program(&b, false)
}

#[allow(clippy::cognitive_complexity)]
pub fn read_loader<P: AsRef<Path>>(filename: P) -> Result<ProgramDescription, ElfReadError> {
    let mut b = Vec::new();
    {
        let mut fi = File::open(filename).map_err(ElfReadError::OpenElfError)?;
        fi.read_to_end(&mut b).map_err(ElfReadError::ReadFileError)?;
    }
    process_program(&b, true)
}

pub fn process_program(b: &[u8], rom_only: bool) -> Result<ProgramDescription, ElfReadError> {
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
    let mut data_copy = Vec::new();
    for s in elf.section_iter() {
        let name = s.get_name(&elf).unwrap_or("<<error>>");

        if s.address() == 0 {
            debug!("(Skipping section {} -- invalid address)", name);
            continue;
        }

        debug!("Section {}:", name);
        debug!("Official header:");
        debug!("{:x?}", s);
        debug!("Interpreted:");
        debug!("    flags:            {:?}", s.flags());
        debug!("    type:             {:?}", s.get_type());
        debug!("    address:          {:08x}", s.address());
        debug!("    offset:           {:08x}", s.offset());
        debug!("    size:             {:x?}", s.size());
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

            if rom_only {
                debug!(
                    "\n-- Not writing {}, type: {:?} flags: {:x}, len: {:x} -- ROM image requested --\n",
                    name,
                    s.get_type(),
                    s.flags(),
                    s.size(),
                );

                // This flag in particular causes the data section to be skipped. This "must be" the case
                // for the loader, because the loader doesn't have a loader. Thus as a requirement, the
                // loader must have a data region that is all 0. Check that this condition is met.
                let section_data = s.raw_data(&elf);
                if !section_data.iter().all(|&x| x == 0) {
                    // If you get this panic, this is why it happened, and what you need to do.
                    //
                    // The why: the loader itself doesn't have a loader. So, any .data required by
                    // the loader program can't be set up in advance for the loader.
                    //
                    // What causes this: generally, a `static mut` in the loader will cause some .data
                    // to be allocated. In the precursor/betrusted loader, there are no instances of this.
                    //
                    // However, in the baochip loaders, the USB handler needs to be a `static mut` because
                    // the interrupt handler needs to be able to find it at a globally known location, and
                    // the data has to persist beyond the scope of a single interrupt.
                    //
                    // Why we can skip it in the case of the loader: the reason we don't have to include
                    // the data section in the loader's ROM image is two-fold. 1) the data going into the
                    // `static mut` interrupt handler is assumed uninitialized (due to the wrapper being
                    // an Option<Usb> set to None); and 2) the RAM is fully zeroized by a small assembly
                    // routine that executes before the loader runs. (1) means that in practice, the contents
                    // of the .data section is always 0. (2) means we can just whack a pointer at where the
                    // data section should go and the assumptions are met for the loader.
                    //
                    // So, the `if` statement above assures us that we didn't do something like create
                    // a `static mut` which has a non-zero value that program execution relies upon.
                    //
                    // The basic answer for the loader is "don't do that". Because the loader doesn't have
                    // a loader, it needs to be self-sufficient in terms of setting up all of its state,
                    // so in the case that some global shared state is needed, there should be an explicit
                    // initializer somewhere in the code. If this panic triggers, look for the code that
                    // is assuming some data is magically pre-loaded for the loader, and eliminate that code.
                    data_copy.extend_from_slice(&section_data);

                    /*
                    println!("Loader data section is not all 0's. This case is not handled by the loader.");
                    println!("Here is what is non-zero, as (byte offset: byte) tuples:");
                    let mut printed = 0;
                    for (i, &d) in section_data.iter().enumerate() {
                        if d != 0 {
                            printed += 1;
                            println!("    ({:04x}: {:02x})", i, d);
                        }
                        if printed > 64 {
                            println!("** Output cut off due to debug length limit");
                            break;
                        }
                    }
                    */
                }
                continue;
            }
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
            "  s.offset: {:08x}  program_offset: {:08x}  Bytes: {:08x}",
            s.offset(),
            program_offset,
            s.raw_data(&elf).len(),
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

    if data_offset as usize % size_of::<u32>() == 0 {
        const SPACING: &'static str = "  ";
        let mut non_zero_tuples = Vec::<(usize, u32)>::new();
        for (i, chunk) in data_copy.chunks_exact(4).enumerate() {
            let word = u32::from_le_bytes(chunk.try_into().unwrap());
            if word != 0 {
                non_zero_tuples.push((i, word));
            }
        }
        println!("Code for loader/baremetal integration:\n");
        println!("// Define the .data region - bootstrap baremetal using these hard-coded parameters.");
        println!("const DATA_ORIGIN: usize = 0x{:x};", data_offset);
        println!(
            "const DATA_SIZE_BYTES: usize = 0x{:x};",
            // round up to the nearest u32 word. Includes .data, .bss, .stack, .heap - regions to be zero'd.
            ((data_size + bss_size) as usize + size_of::<u32>() - 1) & !(size_of::<u32>() - 1)
        );
        let mut init_str = String::new();
        init_str.push_str(&format!("const DATA_INIT: [(usize, u32); {}] = [\n", non_zero_tuples.len()));
        for (offset, data) in non_zero_tuples {
            init_str.push_str(&format!("{}(0x{:x}, 0x{:x}),\n", SPACING, offset, data));
        }
        init_str.push_str(&format!("];"));
        print!("{}", init_str);
        let boilerplate = r#"
// Clear .data, .bss, .stack, .heap regions & setup .data values
unsafe {
    let data_ptr = DATA_ORIGIN as *mut u32;
    for i in 0..DATA_SIZE_BYTES / size_of::<u32>() {
        data_ptr.add(i).write_volatile(0);
    }
    for (offset, data) in DATA_INIT {
        data_ptr.add(offset).write_volatile(data);
    }
}
        "#;
        println!("\n{}", boilerplate);
    } else {
        println!(
            "Data section is not word-aligned, check objdump in detail for how to initialize the section"
        );
    }
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
