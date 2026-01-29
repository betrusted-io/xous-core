use std::env;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::path::Path;
use std::process;
use std::slice;

use crc::{Hasher16, crc16};

fn read_u32_from_ptr(p: *const u8) -> u32 {
    let sl = unsafe { slice::from_raw_parts(p, 4) };
    let mut arr: [u8; 4] = Default::default();
    arr.copy_from_slice(sl);
    u32::from_le_bytes(arr)
}

fn read_u16_from_ptr(p: *const u8) -> u16 {
    let sl = unsafe { slice::from_raw_parts(p, 2) };
    let mut arr: [u8; 2] = Default::default();
    arr.copy_from_slice(sl);
    u16::from_le_bytes(arr)
}

fn read_next_tag(b8: *mut u8, byte_offset: &mut usize) -> Result<(u32, u16, u32), ()> {
    let tag_name = read_u32_from_ptr(b8.wrapping_add(*byte_offset));
    *byte_offset += 4;

    let crc = read_u16_from_ptr(b8.wrapping_add(*byte_offset));
    *byte_offset += 2;

    let size = read_u16_from_ptr(b8.wrapping_add(*byte_offset)) as u32 * 4;
    *byte_offset += 2;

    Ok((tag_name, crc, size))
}

fn print_tag(b8: *mut u8, size: u32, crc: u16, byte_offset: &mut usize) -> Result<(), ()> {
    let data = unsafe { slice::from_raw_parts(b8.add(*byte_offset) as *const u8, size as usize) };
    *byte_offset += size as usize;

    let mut digest = crc16::Digest::new(crc16::X25);
    digest.write(data);

    let mut word_arr: [u8; 4] = Default::default();
    for bytes in data.chunks(4) {
        word_arr.copy_from_slice(bytes);
        let word = u32::from_le_bytes(word_arr);
        print!(" {:08x}", word);
    }

    if digest.sum16() == crc {
        print!("  CRC: OK");
    } else {
        print!("  CRC: FAIL (calc: {:04x})", digest.sum16());
    }
    println!();
    Ok(())
}

fn process_tags(b8: *mut u8) {
    let mut byte_offset = 0;
    let mut total_words = 0u32;
    loop {
        let (tag_name, crc, size) = read_next_tag(b8, &mut byte_offset).expect("couldn't read next tag");
        if tag_name == u32::from_le_bytes(*b"XArg") && size == 20 {
            total_words = read_u32_from_ptr(b8.wrapping_add(byte_offset)) * 4;
            println!(
                "Found Xous Args Size at offset {}, setting total_words to {}",
                byte_offset, total_words
            );
        }

        let tag_name_bytes = tag_name.to_le_bytes();
        let tag_name_str = String::from_utf8_lossy(&tag_name_bytes);
        print!("{:08x} ({}) ({} bytes, crc: {:04x}):", tag_name, tag_name_str, size, crc);
        print_tag(b8, size, crc, &mut byte_offset).expect("couldn't read next data");

        if byte_offset as u32 == total_words {
            return;
        }
        if byte_offset as u32 > total_words {
            panic!("exceeded total words ({}) with byte_offset of {}", total_words, byte_offset);
        }
    }
}

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: {} [xous_presign.img]", args.get(0).map(|a| a.as_str()).unwrap_or("read-tags"));
        process::exit(1);
    }

    let input_filename = Path::new(args.get(1).unwrap()).to_path_buf();

    let mut tag_buf = vec![];
    {
        let mut f = File::open(input_filename)?;
        f.read_to_end(&mut tag_buf)?;
    }

    let byte_buffer = tag_buf.as_mut_ptr();
    process_tags(byte_buffer);
    Ok(())
}
