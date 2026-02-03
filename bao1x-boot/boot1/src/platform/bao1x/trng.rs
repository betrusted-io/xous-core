use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::RngCore;
use rand_chacha::rand_core::SeedableRng;
use utralib::*;

pub struct ManagedTrng {
    ro_trng: bao1x_hal::sce::trng::Trng,
    csprng: ChaCha8Rng,
}

impl ManagedTrng {
    pub fn new() -> Self {
        let mut ro_trng = bao1x_hal::sce::trng::Trng::new(utra::trng::HW_TRNG_BASE);
        ro_trng.setup_raw_generation(32);

        // do a quick health check on the TRNG before using it. Looks for repeated values over 8 samples.
        // this will rule out e.g. TRNG output tied to 0 or 1, or an external feed just feeding it data.
        // About 1 in 500 million chance of triggering falsely.
        let mut values = [0u32; 8];
        for value in values.iter_mut() {
            *value = ro_trng.get_raw();
        }
        for _ in 0..values.len() {
            if values.contains(&ro_trng.get_raw()) {
                // don't proceed if we see a stuck value
                crate::println!("TRNG had stuck value, dying!");
                bao1x_hal::sigcheck::die_no_std();
            }
        }

        // The first seed is always 0.
        let mut seed = [0u8; 32];
        // println!("seed: {:x?}", seed); // used to eyeball that things are working correctly
        let mut csprng = ChaCha8Rng::from_seed(seed);

        // Accumulate TRNG data into the seed (which starts at 0).
        //
        // Each round pulls in 8*32 = 256 bits from HW TRNG
        // 64 rounds of this would fold in 16,384 bits total, about 100x safety margin
        // from the minimum target of 128 bits. This process is done to hedge against
        // potentially broken/damaged TRNGs that are hard to detect. In practice all the
        // TRNGs are measured to generate at least 7.9 bits/byte of entropy, but this
        // is a really important step so belt-and-suspenders are warranted.
        for _ in 0..64 {
            // extract the seed of the current version of the CSPRNG
            seed = csprng.get_seed();

            reseed(&mut ro_trng, &mut seed);

            // Make a new CSPRNG from the old seed that was XOR'd with the TRNG data
            csprng = ChaCha8Rng::from_seed(seed);

            // Mix up the seed with output from the CSPRNG (as-seeded). The idea is to diffuse the
            // TRNG data across all the bits of the state, just in case the TRNG has some biased bits.
            for s in seed.chunks_mut(8) {
                for (s_byte, chacha_byte) in s.iter_mut().zip(csprng.next_u64().to_le_bytes()) {
                    *s_byte ^= chacha_byte;
                }
            }

            // Make a final version of the CSPRNG based on the mixed state.
            csprng = ChaCha8Rng::from_seed(seed);
        }
        Self { ro_trng, csprng }
    }

    pub fn generate_key(&mut self) -> [u8; 32] {
        let mut key = [0u8; 32];
        self.csprng.fill_bytes(&mut key);

        // reseed the csprng with more random data
        let mut seed = self.csprng.get_seed();
        reseed(&mut self.ro_trng, &mut seed);
        self.csprng = ChaCha8Rng::from_seed(seed);
        // mix the reseeded data against the csprng itself
        for s in seed.chunks_mut(8) {
            for (s_byte, chacha_byte) in s.iter_mut().zip(self.csprng.next_u64().to_le_bytes()) {
                *s_byte ^= chacha_byte;
            }
        }
        self.csprng = ChaCha8Rng::from_seed(seed);
        key
    }
}

fn reseed(trng: &mut bao1x_hal::sce::trng::Trng, seed: &mut [u8]) {
    // XOR the seed with TRNG data
    for s in seed.chunks_mut(4) {
        let incoming = trng.get_u32().expect("TRNG error");
        for (s_byte, &ro_byte) in s.iter_mut().zip(incoming.to_le_bytes().iter()) {
            *s_byte ^= ro_byte;
        }
    }
}
