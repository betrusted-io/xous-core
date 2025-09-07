use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};

use num_traits::ToBytes;
use rand_chacha::ChaCha8Rng;
use rand_core::{CryptoRng, RngCore, SeedableRng};

const RESEED_INTERVAL: u32 = 128;

static RESEED: AtomicU32 = AtomicU32::new(0);
pub const TRNG_TEST_BUF_LEN: usize = 2048;

#[derive(Debug)]
pub struct Trng {
    csprng: RefCell<rand_chacha::ChaCha8Rng>,
    mode: api::TrngTestMode,
}
impl Trng {
    pub fn new(_xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        let mut seed = [0u8; 32];

        // server_id is a random number from the hardware entropy pool. Note that this
        // has already been conditioned and whitened, so we can use it directly.
        let seed_l = xous::create_server_id().unwrap().to_array();
        let seed_h = xous::create_server_id().unwrap().to_array();
        for chunk in seed[..16].chunks_mut(4) {
            for s in seed_l {
                chunk.copy_from_slice(&s.to_le_bytes())
            }
        }
        for chunk in seed[16..].chunks_mut(4) {
            for s in seed_h {
                chunk.copy_from_slice(&s.to_le_bytes())
            }
        }
        Ok(Trng { csprng: RefCell::new(ChaCha8Rng::from_seed(seed)), mode: api::TrngTestMode::Raw })
    }

    fn reseed(&self) {
        let reseed_ctr = match RESEED.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |x| Some(x + 1)) {
            Ok(x) => x,
            Err(x) => x,
        };
        if reseed_ctr > RESEED_INTERVAL {
            RESEED.store(0, Ordering::SeqCst);
            // incorporate randomness from the TRNG
            let mut seed = self.csprng.borrow_mut().get_seed();
            // server_id is a random number from the hardware entropy pool
            let seed_l = xous::create_server_id().unwrap().to_array();
            let seed_h = xous::create_server_id().unwrap().to_array();
            for (sd, pool) in seed[..16].chunks_mut(4).into_iter().zip(seed_l.iter().map(|s| s.to_le_bytes()))
            {
                for (sd_byte, &pool_byte) in sd.iter_mut().zip(pool.iter()) {
                    *sd_byte ^= pool_byte;
                }
            }
            for (sd, pool) in seed[16..].chunks_mut(4).into_iter().zip(seed_h.iter().map(|s| s.to_le_bytes()))
            {
                for (sd_byte, &pool_byte) in sd.iter_mut().zip(pool.iter()) {
                    *sd_byte ^= pool_byte;
                }
            }
            self.csprng.replace(rand_chacha::ChaCha8Rng::from_seed(seed));
        }
    }

    pub fn get_u32(&self) -> Result<u32, xous::Error> {
        self.reseed();
        Ok(self.csprng.borrow_mut().next_u32())
    }

    pub fn get_u64(&self) -> Result<u64, xous::Error> {
        self.reseed();
        Ok(self.csprng.borrow_mut().next_u64())
    }

    pub fn fill_buf(&self, data: &mut [u32]) -> Result<(), xous::Error> {
        for d in data.iter_mut() {
            *d = self.get_u32()?;
        }
        Ok(())
    }

    /// This is copied out of the 0.5 API for rand_core
    pub fn fill_bytes_via_next(&mut self, dest: &mut [u8]) {
        let mut left = dest;
        while left.len() >= 8 {
            let (l, r) = { left }.split_at_mut(8);
            left = r;
            let chunk: [u8; 8] = self.next_u64().to_ne_bytes();
            l.copy_from_slice(&chunk);
        }
        let n = left.len();
        if n > 4 {
            let chunk: [u8; 8] = self.next_u64().to_ne_bytes();
            left.copy_from_slice(&chunk[..n]);
        } else if n > 0 {
            let chunk: [u8; 4] = self.next_u32().to_ne_bytes();
            left.copy_from_slice(&chunk[..n]);
        }
    }

    /// Sets the test mode according to the argument. Blocks until mode is set.
    pub fn set_test_mode(&mut self, test_mode: api::TrngTestMode) { self.mode = test_mode; }

    /// Gets test data from the TRNG.
    pub fn get_test_data(&mut self) -> Result<[u8; TRNG_TEST_BUF_LEN], xous::Error> {
        match self.mode {
            api::TrngTestMode::None => {
                let mut buf = [0u8; TRNG_TEST_BUF_LEN];
                self.fill_bytes(&mut buf);
                Ok(buf)
            }
            api::TrngTestMode::Raw => {
                let mut buf = [0u8; TRNG_TEST_BUF_LEN];
                for chunk in buf.chunks_mut(16) {
                    match xous::rsyscall(xous::SysCall::RawTrng(0, 0, 0, 0, 0, 0, 0))
                        .expect("RawTrng syscall failed")
                    {
                        xous::Result::Scalar5(r0, r1, r2, r3, _) => {
                            chunk[..4].copy_from_slice(&(r0 as u32).to_le_bytes());
                            chunk[4..8].copy_from_slice(&(r1 as u32).to_le_bytes());
                            chunk[8..12].copy_from_slice(&(r2 as u32).to_le_bytes());
                            chunk[12..].copy_from_slice(&(r3 as u32).to_le_bytes());
                        }
                        _ => panic!("Bad syscall result"),
                    }
                }
                Ok(buf)
            }
        }
    }
}

impl RngCore for Trng {
    fn next_u32(&mut self) -> u32 { self.get_u32().expect("couldn't get random u32 from server") }

    fn next_u64(&mut self) -> u64 { self.get_u64().expect("couldn't get random u64 from server") }

    fn fill_bytes(&mut self, dest: &mut [u8]) { self.fill_bytes_via_next(dest); }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        Ok(self.fill_bytes(dest))
    }
}

impl CryptoRng for Trng {}

pub mod api {
    #[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive, PartialEq, Eq, Copy, Clone)]
    pub enum TrngTestMode {
        // No test mode configured. Whitened data is sampled.
        None,
        // Raw TRNG output.
        Raw,
    }
}
