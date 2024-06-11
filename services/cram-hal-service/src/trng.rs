use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};

use rand_chacha::ChaCha8Rng;
use rand_core::{CryptoRng, RngCore, SeedableRng};

const RESEED_INTERVAL: u32 = 32;

static RESEED: AtomicU32 = AtomicU32::new(0);
pub const TRNG_TEST_BUF_LEN: usize = 2048;

#[derive(Debug)]
pub struct Trng {
    csprng: RefCell<rand_chacha::ChaCha8Rng>,
}
impl Trng {
    pub fn new(_xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        Ok(Trng {
            csprng: RefCell::new(ChaCha8Rng::seed_from_u64(
                (xous::create_server_id().unwrap().to_u32().0 as u64)
                    | ((xous::create_server_id().unwrap().to_u32().0 as u64) << 32),
            )),
        })
    }

    fn reseed(&self) {
        let reseed_ctr = match RESEED.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |x| Some(x + 1)) {
            Ok(x) => x,
            Err(x) => x,
        };
        if reseed_ctr > RESEED_INTERVAL {
            RESEED.store(0, Ordering::SeqCst);
            // incorporate randomness from the TRNG
            let half = self.csprng.borrow_mut().next_u32();
            self.csprng.replace(rand_chacha::ChaCha8Rng::seed_from_u64(
                (half as u64) << 32 | (xous::create_server_id().unwrap().to_u32().0 as u64),
            ));
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
        use core::mem::transmute;
        let mut left = dest;
        while left.len() >= 8 {
            let (l, r) = { left }.split_at_mut(8);
            left = r;
            let chunk: [u8; 8] = unsafe { transmute(self.next_u64().to_le()) };
            l.copy_from_slice(&chunk);
        }
        let n = left.len();
        if n > 4 {
            let chunk: [u8; 8] = unsafe { transmute(self.next_u64().to_le()) };
            left.copy_from_slice(&chunk[..n]);
        } else if n > 0 {
            let chunk: [u8; 4] = unsafe { transmute(self.next_u32().to_le()) };
            left.copy_from_slice(&chunk[..n]);
        }
    }

    /// Sets the test mode according to the argument. Blocks until mode is set.
    pub fn set_test_mode(&self, test_mode: api::TrngTestMode) {
        log::info!("TODO: trng test mode setting: {:?}", test_mode);
    }

    /// Gets test data from the TRNG.
    pub fn get_test_data(&self) -> Result<[u8; TRNG_TEST_BUF_LEN], xous::Error> {
        log::info!("TODO: get test data");
        Ok([0u8; TRNG_TEST_BUF_LEN])
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
