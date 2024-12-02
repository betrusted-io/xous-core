#![allow(dead_code)]
use std::fs::File;
use std::fs::OpenOptions;
use std::io::SeekFrom;
use std::io::prelude::*;
use std::mem::MaybeUninit;
use std::sync::Once;

use crate::api::*;

// This is considered bad practice for Rust to use a global singleton.
// However, this hack puts the burden of emulation on the emulator, while
// keeping the production code clean (otherwise we'd have large sections
// of production code with #cfg regions to deal with lifetime differences).
// Besides, in reality, FLASH memory is a static, globally mutable pool of data.
//
// Note that this is a concurrently accessed, unsafe, unchecked vector.
struct FlashSingleton {
    memory: Vec<u8>,
    disk: File,
}

fn flashmem() -> &'static mut FlashSingleton {
    static mut SINGLETON: MaybeUninit<FlashSingleton> = MaybeUninit::uninit();
    static ONCE: Once = Once::new();

    unsafe {
        ONCE.call_once(|| {
            let mut disk = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open("../tools/pddb-images/hosted.bin")
                .expect("Can't open a PDDB image file for writing");

            let mut memory = Vec::<u8>::with_capacity(PDDB_A_LEN);
            if disk.metadata().unwrap().len() == 0 {
                for _ in 0..PDDB_A_LEN {
                    memory.push(0xFF);
                }
                disk.write(&memory).expect("couldn't create initial disk image");
            } else {
                match disk.read_to_end(&mut memory) {
                    Ok(bytes_read) => {
                        if bytes_read != PDDB_A_LEN {
                            log::warn!(
                                "PDDB disk image is of an incorrect size: got {}, expected {}",
                                bytes_read,
                                PDDB_A_LEN
                            );
                        }
                    }
                    _ => {
                        panic!("Can't read PDDB disk image, refusing to run!");
                    }
                }
            }

            let flashmem = FlashSingleton { memory, disk };
            (&mut *(&raw mut SINGLETON)).write(flashmem);
        });
        (&mut *(&raw mut SINGLETON)).assume_init_mut()
    }
}

#[derive(Copy, Clone)]
pub struct KeyExport {
    pub basis_name: [u8; 64],
    /// data key
    pub key: [u8; 32],
    /// page table key
    pub pt_key: [u8; 32],
}
pub struct EmuStorage {}
impl EmuStorage {
    pub fn new() -> Self { EmuStorage {} }

    pub unsafe fn as_slice<T>(&self) -> &[T] {
        core::slice::from_raw_parts(
            flashmem().memory.as_ptr() as *const T,
            flashmem().memory.len() / core::mem::size_of::<T>(),
        )
    }

    pub unsafe fn as_mut_slice(&mut self) -> &mut [u8] { flashmem().memory.as_mut_slice() }

    /// used to reset the storage for repeated test case generation
    pub fn reset(&mut self) {
        for b in flashmem().memory.as_mut_slice() {
            *b = 0xFF;
        }
    }

    pub fn dump_fs(&self, name: &Option<String>) {
        let defaultname = String::from("pddb");
        let rootname = name.as_ref().unwrap_or(&defaultname);
        let mut f = File::create(format!("../tools/pddb-images/{}.bin", rootname)).unwrap();
        f.write_all(flashmem().memory.as_slice()).unwrap();
        f.flush().unwrap();
    }

    pub fn dump_keys(&self, known_keys: &[KeyExport], name: &Option<String>) {
        let defaultname = String::from("pddb");
        let rootname = name.as_ref().unwrap_or(&defaultname);
        let mut f = File::create(format!("../tools/pddb-images/{}.key", rootname)).unwrap();
        f.write_all(&(known_keys.len() as u32).to_le_bytes()).unwrap();
        for key in known_keys {
            f.write_all(&key.basis_name).unwrap();
            f.write_all(&key.key).unwrap();
            f.write_all(&key.pt_key).unwrap();
        }
        f.flush().unwrap();
    }
}

pub struct HostedSpinor {}
impl HostedSpinor {
    pub fn new() -> Self { HostedSpinor {} }

    pub fn patch(
        &self,
        _region: &[u8],
        _region_base: u32,
        data: &[u8],
        offset: u32,
    ) -> Result<(), xous::Error> {
        // println!("patch at {:x}+{}", offset, data.len());
        for (&src, dst) in data
            .iter()
            .zip(flashmem().memory.as_mut_slice()[offset as usize..offset as usize + data.len()].iter_mut())
        {
            *dst = src;
        }
        flashmem().disk.seek(SeekFrom::Start(offset as u64)).expect("couldn't seek PDDB");
        flashmem().disk.write(data).expect("couldn't write PDDB");
        Ok(())
    }

    pub fn bulk_erase(&self, start: u32, len: u32) -> Result<(), xous::Error> {
        for b in flashmem().memory.as_mut_slice()
            [(start - xous::PDDB_LOC) as usize..(start - xous::PDDB_LOC + len) as usize]
            .iter_mut()
        {
            *b = 0xFF;
        }
        flashmem().disk.seek(SeekFrom::Start(start as u64)).expect("couldn't seek PDDB");
        let mut blank = Vec::<u8>::with_capacity(len as usize);
        for _ in 0..len {
            blank.push(0xFF);
        }
        flashmem().disk.write(&blank).expect("couldn't write PDDB");
        Ok(())
    }
}
