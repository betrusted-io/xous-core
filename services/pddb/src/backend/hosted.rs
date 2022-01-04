#![allow(dead_code)]
use crate::api::*;

use std::sync::Once;
use std::mem::MaybeUninit;

use std::fs::File;
use std::io::prelude::*;

// This is considered bad practice for Rust to use a global singleton.
// However, this hack puts the burden of emulation on the emulator, while
// keeping the production code clean (otherwise we'd have large sections
// of production code with #cfg regions to deal with lifetime differences).
// Besides, in reality, FLASH memory is a static, globally mutable pool of data.
//
// Note that this is a concurrently accessed, unsafe, unchecked vector.
struct FlashSingleton {
    memory: Vec::<u8>,
}

fn flashmem() -> &'static mut FlashSingleton {
    static mut SINGLETON: MaybeUninit<FlashSingleton> = MaybeUninit::uninit();
    static ONCE: Once = Once::new();

    unsafe {
        ONCE.call_once(|| {
            let mut memory = Vec::<u8>::with_capacity(PDDB_A_LEN);
            for _ in 0..PDDB_A_LEN {
                memory.push(0xFF);
            }
            let flashmem = FlashSingleton {
                memory,
            };
            SINGLETON.write(flashmem);
        });
        SINGLETON.assume_init_mut()
    }
}

#[derive(Copy, Clone)]
pub struct KeyExport {
    pub basis_name: [u8; 64],
    pub key: [u8; 32],
}
pub struct EmuStorage {
}
impl EmuStorage {
    pub fn new() -> Self {
        EmuStorage {
        }
    }
    pub fn as_slice<T>(&self) -> &[T] {
        unsafe {
            core::slice::from_raw_parts(
                flashmem().memory.as_ptr() as *const T,
                flashmem().memory.len() / core::mem::size_of::<T>(),
            )
        }
    }
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        flashmem().memory.as_mut_slice()
    }
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
        }
        f.flush().unwrap();
    }
}

pub struct HostedSpinor {
}
impl HostedSpinor {
    pub fn new() -> Self {
        HostedSpinor {
        }
    }
    pub fn patch(&self, _region: &[u8], _region_base: u32, data: &[u8], offset: u32) -> Result<(), xous::Error> {
        for (&src, dst) in data.iter().zip(
            flashmem().memory.as_mut_slice()[offset as usize..offset as usize + data.len()].iter_mut()
        ) {
            *dst = src;
        }
        Ok(())
    }
}