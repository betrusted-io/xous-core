pub mod bflg;
pub mod inie;
pub mod inif;
pub mod inis;
pub mod memory;
pub mod pnam;
pub mod swap;
pub mod xkrn;

pub(crate) const PAGE_SIZE: usize = 4096;

pub fn align_size_up(offset: usize, alignment_offset: usize) -> usize {
    if offset % PAGE_SIZE == alignment_offset {
        offset
    } else {
        if offset % PAGE_SIZE < alignment_offset {
            offset + (alignment_offset - offset % PAGE_SIZE)
        } else {
            (offset & !(PAGE_SIZE - 1)) + PAGE_SIZE + alignment_offset
        }
    }
}

pub fn align_data_up(data: &Vec<u8>, alignment_offset: usize) -> Vec<u8> {
    if data.len() % PAGE_SIZE == alignment_offset {
        data.to_vec()
    } else {
        let padding = align_size_up(data.len(), alignment_offset) - data.len();
        let pad = vec![0u8; padding];
        (&[&data[..], &pad[..]]).concat().to_vec()
    }
}
