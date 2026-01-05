use core::sync::atomic::{AtomicU32, Ordering};
use std::cell::RefCell;

use bao1x_hal_service::trng::api;
use bio_lib::av_trng::AvTrng;
use rand_chacha::ChaCha8Rng;
// the 0.5.1 API is necessary for compatibility with curve25519-dalek crates
use rand_core::{CryptoRng, RngCore, SeedableRng};

const RESEED_INTERVAL: u32 = 128;
static RESEED: AtomicU32 = AtomicU32::new(0);

pub struct HwTrng {
    csprng: RefCell<rand_chacha::ChaCha8Rng>,
    av_trng: AvTrng,
}
impl HwTrng {
    pub fn new(mut av_trng: AvTrng) -> Self {
        let mut seed = [0u8; 32];
        {
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

            // add AV TRNG data
            for s in seed.chunks_mut(4) {
                let raw32 = av_trng.get_u32();
                for (s_byte, &av_byte) in s.iter_mut().zip(raw32.to_le_bytes().iter()) {
                    *s_byte ^= av_byte;
                }
            }
        }
        HwTrng { csprng: RefCell::new(ChaCha8Rng::from_seed(seed)), av_trng }
    }

    fn reseed(&mut self) {
        let reseed_ctr = match RESEED.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |x| Some(x + 1)) {
            Ok(x) => x,
            Err(x) => x,
        };
        if reseed_ctr > RESEED_INTERVAL {
            log::debug!("reseeding CSPRNG from hardware TRNG sources");
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
            // add AV TRNG data
            for s in seed.chunks_mut(4) {
                let raw32 = self.av_trng.get_u32();
                for (s_byte, &av_byte) in s.iter_mut().zip(raw32.to_ne_bytes().iter()) {
                    *s_byte ^= av_byte;
                }
            }
            self.csprng.replace(rand_chacha::ChaCha8Rng::from_seed(seed));
        }
    }

    pub fn get_u32(&mut self) -> Result<u32, xous::Error> {
        self.reseed();
        Ok(self.csprng.borrow_mut().next_u32())
    }

    pub fn get_u64(&mut self) -> Result<u64, xous::Error> {
        self.reseed();
        Ok(self.csprng.borrow_mut().next_u64())
    }

    pub fn fill_buf(&mut self, data: &mut [u32]) -> Result<(), xous::Error> {
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

    pub fn get_tests(&self) -> api::HealthTests {
        todo!();
    }

    pub fn get_errors(&self) -> api::TrngErrors {
        todo!();
    }
}

impl RngCore for HwTrng {
    fn next_u32(&mut self) -> u32 { self.get_u32().expect("couldn't get random u32 from server") }

    fn next_u64(&mut self) -> u64 { self.get_u64().expect("couldn't get random u64 from server") }

    fn fill_bytes(&mut self, dest: &mut [u8]) { self.fill_bytes_via_next(dest); }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        Ok(self.fill_bytes(dest))
    }
}

impl CryptoRng for HwTrng {}
