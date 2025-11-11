use core::sync::atomic::{AtomicU32, Ordering};
use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use bao1x_hal_service::trng::api;
use rand_chacha::ChaCha8Rng;
// the 0.5.1 API is necessary for compatibility with curve25519-dalek crates
use rand_core::{CryptoRng, RngCore, SeedableRng};
use utralib::*;
use xous_bio_bdma::*;
const RESEED_INTERVAL: u32 = 128;
static RESEED: AtomicU32 = AtomicU32::new(0);
const BITS_PER_SAMPLE: usize = 4;

pub struct HwTrng {
    csprng: RefCell<rand_chacha::ChaCha8Rng>,
    bio_ss: Arc<Mutex<BioSharedState>>,
}
impl HwTrng {
    pub fn new(bio_ss_guarded: Arc<Mutex<BioSharedState>>) -> Self {
        let mut seed = [0u8; 32];
        {
            let mut bio_ss = bio_ss_guarded.lock().unwrap();

            let tt = ticktimer::Ticktimer::new().unwrap();

            let iox = crate::iox::IoxHal::new();
            let trng_pin = bao1x_hal::board::setup_trng_input_pin(&iox);
            let trng_power = bao1x_hal::board::setup_trng_power_pin(&iox);
            iox.set_gpio_pin_value(trng_power.0, trng_power.1, bao1x_api::IoxValue::High);
            tt.sleep_ms(50).ok(); // wait for power to stabilize on the avalanche generator

            // avalanche generator TRNG - maybe replace this with a proper API call once this has
            // been baked into something a little friendlier/easier to maintain?
            // stop all the machines, so that code can be loaded
            bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x0);
            bio_ss.load_code(avtrng_bio_code(), 0, BioCore::Core0);

            // don't use QDIV
            bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV0, 0x1_0001);

            // use extclock on channel 0, tied to the trng pin
            bio_ss.bio.wo(
                utra::bio_bdma::SFR_EXTCLOCK,
                bio_ss.bio.ms(utra::bio_bdma::SFR_EXTCLOCK_USE_EXTCLK, 0b0001)
                    | bio_ss.bio.ms(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_0, trng_pin as u32),
            );

            // start the machine
            bio_ss.set_core_run_states([true, false, false, false]);
            bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, trng_pin as u32); // start the sampling

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
            let mut raw32 = 0;
            for s in seed.chunks_mut(4) {
                for _ in 0..(size_of::<u32>() * 8) / BITS_PER_SAMPLE {
                    // wait for the next interval to arrive
                    while bio_ss.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL0) == 0 {}
                    let raw = bio_ss.bio.r(utra::bio_bdma::SFR_RXF0);
                    raw32 <<= BITS_PER_SAMPLE;
                    // shift right by one because bit 0 always samples as 0, due to instruction timing
                    raw32 |= (raw >> 1) & ((1 << BITS_PER_SAMPLE) - 1)
                }
                for (s_byte, &av_byte) in s.iter_mut().zip(raw32.to_le_bytes().iter()) {
                    *s_byte ^= av_byte;
                }
            }
        }
        HwTrng { csprng: RefCell::new(ChaCha8Rng::from_seed(seed)), bio_ss: bio_ss_guarded }
    }

    fn reseed(&self) {
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
            let mut raw32 = 0;
            let bio_ss = self.bio_ss.lock().unwrap();
            for s in seed.chunks_mut(4) {
                for _ in 0..(size_of::<u32>() * 8) / BITS_PER_SAMPLE {
                    // wait for the next interval to arrive
                    while bio_ss.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL0) == 0 {}
                    let raw = bio_ss.bio.r(utra::bio_bdma::SFR_RXF0);
                    raw32 <<= BITS_PER_SAMPLE;
                    // shift right by one because bit 0 always samples as 0, due to instruction timing
                    raw32 |= (raw >> 1) & ((1 << BITS_PER_SAMPLE) - 1)
                }
                for (s_byte, &av_byte) in s.iter_mut().zip(raw32.to_ne_bytes().iter()) {
                    *s_byte ^= av_byte;
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

#[rustfmt::skip]
bio_code!(avtrng_bio_code, BM_AVTRNG_BIO_START, BM_AVTRNG_BIO_END,
    "mv x1, x16", // get pin for trng input
    "li x2, 1",
    "sll x1, x2, x1", // shift the pin into a bitmask
    "mv x25, x1",  // make it an input
"10:",
    "mv x20, x0", // wait for quantum: this time, the toggle from the TRNG
    "mv x1, x31", // remember aclk time
    "mv x16, x1", // save result to FIFO
    "j 10b" // and do it again
);
