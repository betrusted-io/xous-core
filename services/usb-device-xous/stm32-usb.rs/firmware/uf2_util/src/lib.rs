use goblin::{
    elf::{
        Elf,
        program_header::PT_LOAD,
    },
    error::Error as GoblinError,
};
use uf2_block::DATA_LENGTH;
use uf2_block::{ Block, Error as Uf2Error };
use log::trace;
use std::io::Error as IoError;

#[derive(Debug)]
pub enum Error {
    Uf2Error(Uf2Error),
    GoblinError(GoblinError),
    IoError(IoError),
}
impl From<Uf2Error> for Error {
    fn from(e: Uf2Error) -> Error {
        Error::Uf2Error(e)
    }
}
impl From<GoblinError> for Error {
    fn from(e: GoblinError) -> Error {
        Error::GoblinError(e)
    }
}
impl From<IoError> for Error {
    fn from(e: IoError) -> Error {
        Error::IoError(e)
    }
}

fn blockify(base_address: u32, block_size: usize, data: &[u8]) -> Result<Vec<Block>, Error> {
    let res: Result<Vec<_>, _> = data
        .chunks(block_size)
        .enumerate()
        .map(|(i, chunk)| Block::new(
                base_address + (i * block_size) as u32,
                chunk,
            )
        )
        .collect();
    Ok(res?)
}

fn finalize(blocks: Vec<Block>) -> Vec<u8> {
    let n = blocks.len() as u32;
    blocks.into_iter().enumerate().flat_map(|(i, mut b)| {
        b.block_number = i as u32;
        b.number_of_blocks = n;
        trace!("{}/{}: 0x{:X?} {}", i, n, b.target_address, b.payload_size);
        b.pack().expect("Error packing block").to_vec()
    }).collect()
}

fn block_size(page_size: u16) -> Result<usize, Error> {
    let page_size = page_size as usize;
    if page_size > DATA_LENGTH {
        Err(Uf2Error::DataTooLong)?
    }
    Ok((DATA_LENGTH / page_size) * page_size)
}

/// Parses provided bytes as an ELF file and converts contained PT_LOAD segments
/// into UF2 blocks. Will fail if bytes aren't a valid ELF file
pub fn convert_elf(data: &[u8], page_size: u16) -> Result<Vec<u8>, Error> {
    let block_size = block_size(page_size)?;
    let mut blocks = Vec::new();
    for header in Elf::parse(&data)?.program_headers {
        let length = header.p_filesz as usize;
        let start = header.p_offset as usize;
        if header.p_type == PT_LOAD && length > 0 {
            blocks.extend(blockify(
                header.p_paddr as u32,
                block_size,
                &data[start..(start+length)],
            )?);
        }
    }
    Ok(finalize(blocks))
}

/// Converts provided bytes into UF2 blocks assuming bytes are a BIN file
/// No checking on the data is performed
pub fn convert_bin(data: &[u8], page_size: u16, base_address: u32) -> Result<Vec<u8>, Error> {
    let block_size = block_size(page_size)?;
    let blocks = blockify(
        base_address,
        block_size,
        &data,
    )?;
    Ok(finalize(blocks))
}