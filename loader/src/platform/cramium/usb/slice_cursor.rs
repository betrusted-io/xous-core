use core::fmt::Display;

use simple_fatfs::io::{IOBase, Read, Seek, SeekFrom, Write};
use simple_fatfs::{IOError, IOErrorKind};

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum IOErrorFatfs {
    UnexpectedEof,
    Interrupted,
    InvalidData,
    Description,
    SeekUnderflow,
    SeekOverflow,
    SeekOutOfBounds,
}

impl Display for IOErrorFatfs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            IOErrorFatfs::UnexpectedEof => write!(f, "Unexpected EOF"),
            IOErrorFatfs::Interrupted => write!(f, "Interrupted"),
            IOErrorFatfs::InvalidData => write!(f, "Invalid Data"),
            IOErrorFatfs::Description => write!(f, "Unsupported string description"),
            IOErrorFatfs::SeekOutOfBounds => write!(f, "Seek out of bounds"),
            IOErrorFatfs::SeekOverflow => write!(f, "Seek overflow"),
            IOErrorFatfs::SeekUnderflow => write!(f, "Seek underflow"),
        }
    }
}

impl From<&str> for IOErrorFatfs {
    fn from(_value: &str) -> Self { IOErrorFatfs::Description }
}
impl IOErrorKind for IOErrorFatfs {
    fn new_unexpected_eof() -> Self { Self::UnexpectedEof }

    fn new_invalid_data() -> Self { Self::InvalidData }

    fn new_interrupted() -> Self { Self::Interrupted }
}
impl IOError for IOErrorFatfs {
    type Kind = IOErrorFatfs;

    fn new<M>(kind: Self::Kind, _msg: M) -> Self
    where
        M: core::fmt::Display,
    {
        kind
    }

    fn kind(&self) -> Self::Kind { *self }
}

impl simple_fatfs::Error for IOErrorFatfs {}

impl IOBase for SliceCursor<'_> {
    type Error = IOErrorFatfs;
}
pub struct SliceCursor<'a> {
    slice: &'a mut [u8],
    pos: u64,
}

impl<'a> SliceCursor<'a> {
    pub fn new(slice: &'a mut [u8]) -> Self { Self { slice, pos: 0 } }
}

impl Seek for SliceCursor<'_> {
    fn seek(&mut self, seek_from: simple_fatfs::io::SeekFrom) -> Result<u64, Self::Error> {
        let new_pos = match seek_from {
            SeekFrom::Start(offset) => offset,
            SeekFrom::End(offset) => {
                let result = if offset < 0 {
                    self.slice.len().checked_sub((-offset) as usize).ok_or(IOErrorFatfs::SeekUnderflow)?
                } else {
                    self.slice.len().checked_add(offset as usize).ok_or(IOErrorFatfs::SeekOverflow)?
                };
                result as u64
            }
            SeekFrom::Current(offset) => {
                if offset < 0 {
                    self.pos.checked_sub((-offset) as u64).ok_or(IOErrorFatfs::SeekUnderflow)?
                } else {
                    self.pos.checked_add(offset as u64).ok_or(IOErrorFatfs::SeekUnderflow)?
                }
            }
        };

        if new_pos > self.slice.len() as u64 {
            Err(IOErrorFatfs::SeekOutOfBounds)
        } else {
            self.pos = new_pos;
            Ok(self.pos)
        }
    }
}

impl Read for SliceCursor<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let available = &self.slice[self.pos as usize..];
        let to_read = buf.len().min(available.len());
        buf[..to_read].copy_from_slice(&available[..to_read]);
        self.pos += to_read as u64;
        Ok(to_read)
    }
}
impl Write for SliceCursor<'_> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        let available = &mut self.slice[self.pos as usize..];
        let to_write = buf.len().min(available.len());
        available[..to_write].copy_from_slice(&buf[..to_write]);
        self.pos += to_write as u64;
        Ok(to_write)
    }

    fn flush(&mut self) -> Result<(), Self::Error> { Ok(()) }
}
