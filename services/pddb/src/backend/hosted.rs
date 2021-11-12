#![allow(dead_code)]
use crate::api::*;

use std::sync::Once;
use std::mem::MaybeUninit;

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

pub struct EmuStorage {
}
impl EmuStorage {
    pub fn new() -> Self {
        EmuStorage {
        }
    }
    pub fn as_slice(&self) -> &[u8] {
        flashmem().memory.as_slice()
    }
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        flashmem().memory.as_mut_slice()
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