use crate::xous_arguments::{XousArgument, XousArgumentCode, XousSize};
use std::fmt;
use std::io;

#[derive(Debug)]
pub struct XousKernel {
    /// Address of PID1 in RAM (i.e. SPI flash)
    load_offset: u32,

    /// Virtual address of .text section in RAM
    text_offset: u32,

    /// Size of the kernel, in bytes
    text_size: u32,

    /// Virtual address of .data and .bss section in RAM
    data_offset: u32,

    /// Size of .data section
    data_size: u32,

    /// Size of the .bss section
    bss_size: u32,

    /// Virtual address of the entrypoint
    entrypoint: u32,

    /// Actual program contents
    program: Vec<u8>,
}

impl fmt::Display for XousKernel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "    kernel text: {} bytes long, loaded from {:08x} to {:08x} with entrypoint @ {:08x}, and {} bytes of data @ {:08x}, {} bytes of .bss",
            self.text_size, self.load_offset, self.text_offset, self.entrypoint,
            self.data_size, self.data_offset, self.bss_size)
    }
}

impl XousKernel {
    pub fn new(
        text_offset: u32,
        text_size: u32,
        data_offset: u32,
        data_size: u32,
        bss_size: u32,
        entrypoint: u32,
        mut program: Vec<u8>,
    ) -> XousKernel {
        // pad the program to 4 bytes
        while program.len() & 3 != 0 {
            program.push(0);
        }
        XousKernel {
            load_offset: 0,
            text_offset,
            text_size,
            data_offset,
            data_size,
            bss_size,
            entrypoint,
            program,
        }
    }
}

impl XousArgument for XousKernel {
    fn code(&self) -> XousArgumentCode {
        u32::from_le_bytes(*b"XKrn")
    }

    fn length(&self) -> XousSize {
        28 as XousSize
    }

    fn finalize(&mut self, offset: usize) -> usize {
        self.load_offset = offset as u32;
        assert!(self.text_offset > 0xff00_0000,
        "kernel text section is invalid: 0x{:08x} < 0xff000000 -- was it linked with a linker script?", self.text_offset);

        assert!(offset % crate::tags::PAGE_SIZE == 0, "XKrn load offset is not aligned");
        self.program = crate::tags::align_data_up(&self.program);

        self.program.len()
    }

    fn last_data(&self) -> &[u8] {
        &self.program
    }

    fn serialize(&self, output: &mut dyn io::Write) -> io::Result<usize> {
        let mut written = 0;
        written += output.write(&self.load_offset.to_le_bytes())?;
        written += output.write(&self.text_offset.to_le_bytes())?;
        written += output.write(&self.text_size.to_le_bytes())?;
        written += output.write(&self.data_offset.to_le_bytes())?;
        written += output.write(&self.data_size.to_le_bytes())?;
        written += output.write(&self.bss_size.to_le_bytes())?;
        written += output.write(&self.entrypoint.to_le_bytes())?;
        Ok(written)
    }
}
