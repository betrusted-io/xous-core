#![cfg_attr(any(target_os = "none", target_os = "xous"), no_std)]

use digest::{BlockInput, FixedOutputDirty, Reset, Update};
use digest::consts::{U64, U128};
type BlockSize = U128;

use utralib::generated::*;

/// The SHA-512 hash algorithm with the SHA-512 initial hash value.
#[derive(Clone)]
pub struct Sha512 {
    csr: utralib::CSR<u32>,
    fifo: *mut u32,
}
impl Sha512 {
    // use this function instead of default for more control over configuration of the hardware engine
    pub fn new() -> Self {
        let mut csr = CSR::new(utra::sha512::HW_SHA512_BASE as *mut u32);
        csr.wfo(utra::sha512::POWER_ON, 1);
        // setup for sha512 operation
        csr.wo(utra::sha512::CONFIG,
            csr.ms(utra::sha512::CONFIG_DIGEST_SWAP, 1) |
            csr.ms(utra::sha512::CONFIG_ENDIAN_SWAP, 1) |
            csr.ms(utra::sha512::CONFIG_SHA_EN, 1)
        );
        csr.wfo(utra::sha512::COMMAND_HASH_START, 1);
        // csr.wfo(utra::sha512::EV_ENABLE_SHA512_DONE, 1);

        Sha512 {
            csr,
            fifo: utralib::HW_SHA512_MEM as *mut u32,
        }
    }
    // call before exit, to cleanup from bootloader -- otherwise the unit might be stuck on as it's assumed to be off going into Xous
    pub fn power_off(&mut self) {
        self.csr.wfo(utra::sha512::POWER_ON, 0);
    }
}

impl Default for Sha512 {
    fn default() -> Self {
        Sha512::new()
    }
}

impl Drop for Sha512 {
    fn drop(&mut self) {
        self.csr.wo(utra::sha512::CONFIG, 0);  // clear all config bits, including EN, which resets the unit
    }
}

impl BlockInput for Sha512 {
    type BlockSize = BlockSize;
}

impl Update for Sha512 {
    fn update(&mut self, input: impl AsRef<[u8]>) {
        let buf: &[u8] = input.as_ref();

        self.csr.wfo(utra::sha512::POWER_ON, 1);
        let sha = self.fifo;
        let sha_byte = self.fifo as *mut u8;

        for (_reg, chunk) in buf.chunks(4).enumerate() {
            let mut temp: [u8; 4] = Default::default();
            if chunk.len() == 4 {
                temp.copy_from_slice(chunk);
                let dword: u32 = u32::from_le_bytes(temp);

                while self.csr.rf(utra::sha512::FIFO_ALMOST_FULL) != 0 {
                }
                unsafe { sha.write_volatile(dword); }
            } else {
                for index in 0..chunk.len() {
                    while self.csr.rf(utra::sha512::FIFO_ALMOST_FULL) != 0 {
                    }
                    unsafe{ sha_byte.write_volatile(chunk[index]); }
                }
            }
        }
    }
}

impl FixedOutputDirty for Sha512 {
    type OutputSize = U64;

    fn finalize_into_dirty(&mut self, out: &mut digest::Output<Self>) {
        self.csr.wfo(utra::sha512::EV_PENDING_SHA512_DONE, 1); // clear the done if it's pending
        self.csr.wfo(utra::sha512::COMMAND_HASH_PROCESS, 1);
        while self.csr.rf(utra::sha512::EV_PENDING_SHA512_DONE) == 0 {
        }
        self.csr.wfo(utra::sha512::EV_PENDING_SHA512_DONE, 1);

        let _length_in_bits: u64 = (self.csr.r(utra::sha512::MSG_LENGTH0) as u64) | ((self.csr.r(utra::sha512::MSG_LENGTH1) as u64) << 32);
        let mut hash: [u8; 64] = [0; 64];
        let digest_regs: [utralib::Register; 16] = [
            utra::sha512::DIGEST00,
            utra::sha512::DIGEST01,
            utra::sha512::DIGEST10,
            utra::sha512::DIGEST11,
            utra::sha512::DIGEST20,
            utra::sha512::DIGEST21,
            utra::sha512::DIGEST30,
            utra::sha512::DIGEST31,
            utra::sha512::DIGEST40,
            utra::sha512::DIGEST41,
            utra::sha512::DIGEST50,
            utra::sha512::DIGEST51,
            utra::sha512::DIGEST60,
            utra::sha512::DIGEST61,
            utra::sha512::DIGEST70,
            utra::sha512::DIGEST71,
        ];
        let mut i = 0;
        for &reg in digest_regs.iter() {
            hash[i..i+4].clone_from_slice(&self.csr.r(reg).to_le_bytes());
            i += 4;
        }
        self.csr.wo(utra::sha512::CONFIG, 0);  // clear all config bits, including EN, which resets the unit

        // two-stage copy because we're porting code over. maybe this could be merged into the copy above, but let's optimize that once things are working.
        for (dest, &src) in out.chunks_exact_mut(1).zip(hash.iter()) {
            dest.copy_from_slice(&[src])
        }
    }
}

impl Reset for Sha512 {
    fn reset(&mut self) {
        if self.csr.rf(utra::sha512::FIFO_RUNNING) != 0 {
            // if it's running, call digest, then reset
            let sha = self.fifo;
            unsafe { sha.write_volatile(0x0); } // stuff a dummy byte, in case the hash was empty
            self.csr.wfo(utra::sha512::COMMAND_HASH_PROCESS, 1);
            self.csr.wfo(utra::sha512::EV_PENDING_SHA512_DONE, 1);
            while self.csr.rf(utra::sha512::EV_PENDING_SHA512_DONE) == 0 {
            }
        } else {
            // engine is already stopped, just clear the pending bits and reset the config
        }
        self.csr.wfo(utra::sha512::EV_PENDING_SHA512_DONE, 1);
        self.csr.wo(utra::sha512::CONFIG, 0);  // clear all config bits, including EN, which resets the unit
    }
}

opaque_debug::implement!(Sha512);

digest::impl_write!(Sha512);
