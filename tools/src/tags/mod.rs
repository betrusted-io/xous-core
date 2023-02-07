pub mod bflg;
pub mod inie;
pub mod memory;
pub mod pnam;
pub mod xkrn;
pub mod inif;

pub (crate) const PAGE_SIZE: usize = 4096;

pub fn align_size_up(offset: usize) -> usize {
    if offset % PAGE_SIZE == 0 {
        offset
    } else {
        (offset + PAGE_SIZE) & !(PAGE_SIZE - 1)
    }
}

pub fn align_data_up(data: &Vec<u8>) -> Vec::<u8> {
    if data.len() % PAGE_SIZE == 0 {
        data.to_vec()
    } else {
        let padding = align_size_up(data.len()) - data.len();
        let pad = vec![0u8; padding];
        (&[&data[..], &pad[..]]).concat().to_vec()
    }
}