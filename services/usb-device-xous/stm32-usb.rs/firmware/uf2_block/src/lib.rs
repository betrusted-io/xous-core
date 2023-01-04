#![no_std]
use packing::{ 
    Packed,
    PackedSize,
    Error as PackingError,
};

use core::fmt;

//use bitmask::bitmask;

pub const DATA_LENGTH: usize = 476;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Packed)]
#[packed(little_endian, lsb0)]
pub struct MagicStart {
    #[pkd(7, 0, 0, 3)]
    magic_0: u32,
    #[pkd(7, 0, 4, 7)]
    magic_1: u32,
}

impl Default for MagicStart {
    fn default() -> Self {
        const MAGIC_START0: u32 = 0x0A324655; // "UF2\n"
        const MAGIC_START1: u32 = 0x9E5D5157; // Randomly selected
        Self {
            magic_0: MAGIC_START0,
            magic_1: MAGIC_START1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Packed)]
#[packed(little_endian, lsb0)]
pub struct MagicEnd {
    #[pkd(7, 0, 0, 3)]
    magic: u32,
}

impl Default for MagicEnd {
    fn default() -> Self {
        const MAGIC_END: u32 = 0x0AB16F30; // Randomly selected
        Self {
            magic: MAGIC_END,
        }
    }
}

/*
bitmask! {
    #[derive(Debug, Default, Packed)]
    #[packed(little_endian, lsb0)]
    pub mask Flags: u32 where 
    
    #[derive(Debug)]
    flags Flag {
        /// Block should be skipped when writing the device flash; it can be used to store "comments" in the file, typically embedded source code or debug info that does not fit on the device flash
        NotMainFlash = 0x00000001, 
        /// Block contains part of a file to be written to some kind of filesystem on the device
        FileContainer = 0x00001000,
        /// When set, the file_size_or_family_id holds a value identifying the board family (usually corresponds to an MCU)
        FamilyIdPresent = 0x00002000,
        /// When set, the last 24 bytes of data contain an Md5Checksum
        Md5ChecksumPresent = 0x00004000,
    }
}
*/


#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Md5Checksum {
    address: u32,
    length: u32,
    checksum: [u8; 2],
}

#[derive(Clone, Packed)]
#[packed(little_endian, lsb0)]
pub struct Block {
    #[pkd(7, 0, 0, 7)]
    magic_start: MagicStart,
    
    #[pkd(7, 0, 8, 11)]
    pub flags: u32,//Flags,

    #[pkd(7, 0, 12, 15)]
    pub target_address: u32,

    #[pkd(7, 0, 16, 19)]
    pub payload_size: u32,

    #[pkd(7, 0, 20, 23)]
    pub block_number: u32,

    #[pkd(7, 0, 24, 27)]
    pub number_of_blocks: u32,

    #[pkd(7, 0, 28, 31)]
    pub file_size_or_family_id: u32,

    #[pkd(7, 0, 32, 507)]
    pub data: [u8; DATA_LENGTH],

    #[pkd(7, 0, 508, 511)]
    magic_end: MagicEnd,
}

impl fmt::Display for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Uf2: {{ target_address: 0x{:08X?}, payload_size: {}, block {} / {} blocks }}",
            self.target_address, self.payload_size, self.block_number, self.number_of_blocks)
    }
}

impl Default for Block {
    fn default() -> Self {
        Self {
            data: [0; DATA_LENGTH],

            magic_start: Default::default(),
            flags: Default::default(), 
            target_address: Default::default(),
            payload_size: Default::default(),
            block_number: Default::default(),
            number_of_blocks: Default::default(),
            file_size_or_family_id: Default::default(),
            magic_end: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Error {
    DataTooLong,
    InsufficientPackedBytes,
    PackingError(PackingError),
    IncorrectMagic,
}

impl From<PackingError> for Error {
    fn from(e: PackingError) -> Error {
        if e == PackingError::InsufficientBytes {
            Error::InsufficientPackedBytes
        } else {
            Error::PackingError(e)
        }
    }
}

impl Block {
    pub fn new(target_address: u32, data: &[u8]) -> Result<Self, Error> {
        if data.len() > DATA_LENGTH {
            Err(Error::DataTooLong)?
        }

        let payload_size = data.len() as u32;

        let mut new_block = Self {
            target_address,
            payload_size,
            .. Self::default()
        };

        new_block.data[..data.len()].copy_from_slice(data);

        Ok(new_block)
    }

    pub fn pack(&self) -> Result<[u8; 512], Error> {
        let mut ret = [0; Self::BYTES];
        Packed::pack(self, &mut ret)?;
        Ok(ret)
    }

    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let unpacked = Self::unpack(data)?;
        if unpacked.magic_start != MagicStart::default() ||
           unpacked.magic_end != MagicEnd::default()
        {
            Err(Error::IncorrectMagic)?;
        }
        Ok(unpacked)
    }
}