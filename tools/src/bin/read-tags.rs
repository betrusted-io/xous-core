use crc::{crc16, Hasher16};
use std::env;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::path::Path;
use std::process;
use std::slice;
use xous_tools::make_type;

fn read_next_tag(b8: *mut u8, byte_offset: &mut usize) -> Result<(u32, u16, u32), ()> {
    let tag_name = u32::from_le(unsafe { (b8 as *mut u32).add(*byte_offset / 4).read() }) as u32;
    *byte_offset += 4;
    let crc = u16::from_le(unsafe { (b8 as *mut u16).add(*byte_offset / 2).read() }) as u16;
    *byte_offset += 2;
    let size = u16::from_le(unsafe { (b8 as *mut u16).add(*byte_offset / 2).read() }) as u32 * 4;
    *byte_offset += 2;
    Ok((tag_name, crc, size))
}

fn print_tag(b8: *mut u8, size: u32, crc: u16, byte_offset: &mut usize) -> Result<(), ()> {
    let data = unsafe { slice::from_raw_parts(b8.add(*byte_offset) as *const u8, size as usize) };
    let data_32 = unsafe { slice::from_raw_parts(b8.add(*byte_offset) as *const u32, size as usize / 4) };
    *byte_offset += size as usize;

    let mut digest = crc16::Digest::new(crc16::X25);
    digest.write(data);

    for byte in data_32 {
        print!(" {:08x}", byte);
    }

    if digest.sum16() == crc {
        print!("  CRC: OK");
    } else {
        print!("  CRC: FAIL (calc: {:04x})", digest.sum16());
    }
    println!("");
    Ok(())
}

fn process_tags(b8: *mut u8) {
    let mut byte_offset = 0;
    let mut total_words = 0u32;
    loop {
        let (tag_name, crc, size) =
            read_next_tag(b8, &mut byte_offset).expect("couldn't read next tag");
        if tag_name == make_type!("XArg") && size == 20 {
            total_words = unsafe { (b8 as *mut u32).add(byte_offset / 4).read() } * 4;
            println!(
                "Found Xous Args Size at offset {}, setting total_words to {}",
                byte_offset, total_words
            );
        }

        let tag_name_bytes = tag_name.to_le_bytes();
        let tag_name_str = String::from_utf8_lossy(&tag_name_bytes);
        print!(
            "{:08x} ({}) ({} bytes, crc: {:04x}):",
            tag_name, tag_name_str, size, crc
        );
        print_tag(b8, size, crc, &mut byte_offset).expect("couldn't read next data");

        if byte_offset as u32 == total_words {
            return;
        }
        if byte_offset as u32 > total_words {
            panic!(
                "exceeded total words ({}) with byte_offset of {}",
                total_words, byte_offset
            );
        }
    }
}

fn doit() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!(
            "Usage: {} args.bin",
            args.get(0).unwrap_or(&"read-tags".to_owned())
        );
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
fn main() {
    doit().unwrap();
}
