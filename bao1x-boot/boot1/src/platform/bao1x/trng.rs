use bao1x_hal::iox::Iox;
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::RngCore;
use rand_chacha::rand_core::SeedableRng;
use utralib::*;
use xous_bio_bdma::*;

const BITS_PER_SAMPLE: usize = 4;

pub struct ManagedTrng {
    ro_trng: bao1x_hal::sce::trng::Trng,
    av_trng: Option<BioSharedState>,
    csprng: ChaCha8Rng,
}

impl ManagedTrng {
    pub fn new(board_type: &bao1x_api::BoardTypeCoding) -> Self {
        let mut ro_trng = bao1x_hal::sce::trng::Trng::new(utra::trng::HW_TRNG_BASE);
        ro_trng.setup_raw_generation(32);
        let av_trng = if *board_type == bao1x_api::BoardTypeCoding::Baosec {
            let mut bio_ss = BioSharedState::new();

            let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);

            let trng_pin = bao1x_hal::board::setup_trng_input_pin(&iox);
            let bio_bit = bao1x_api::bio::port_and_pin_to_bio_bit(trng_pin.0, trng_pin.1).unwrap();
            let trng_power = bao1x_hal::board::setup_trng_power_pin(&iox);
            iox.set_gpio_pin(trng_power.0, trng_power.1, bao1x_api::IoxValue::High);
            crate::delay(50); // wait for power to stabilize on the avalanche generator

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
                    | bio_ss.bio.ms(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_0, bio_bit.value() as u32),
            );

            // start the machine
            bio_ss.set_core_run_states([true, false, false, false]);
            bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, bio_bit.value() as u32); // start the sampling

            // check if the TRNG seems to be working. If not, reject the board setting and reboot
            /*
            let mut timer = utralib::CSR::new(utra::timer0::HW_TIMER0_BASE as *mut u32);
            timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);
            let av_good: bool;
            let mut ms_passed = 0;
            let mut samples_collected = 0;
            const TIMEOUT_MS: usize = 10;
            const SAMPLES_REQUIRED: usize = 10;
            loop {
                if timer.rf(utra::timer0::EV_PENDING_ZERO) != 0 {
                    ms_passed += 1;
                    timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);
                }
                if ms_passed > TIMEOUT_MS {
                    av_good = false;
                    break;
                }
                if bio_ss.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL0) != 0 {
                    let _ = bio_ss.bio.r(utra::bio_bdma::SFR_RXF0);
                    samples_collected += 1;
                    ms_passed = 0;
                }
                if samples_collected > SAMPLES_REQUIRED {
                    av_good = true;
                    break;
                }
            }
            if av_good {
                Some(bio_ss)
            } else {
                crate::println!(
                    "AV TRNG is not present or functioning. Setting board type to dabao, and rebooting!"
                );
                let one_way = bao1x_hal::acram::OneWayCounter::new();
                while one_way.get_decoded::<bao1x_api::BoardTypeCoding>().expect("owc coding error")
                    != bao1x_api::BoardTypeCoding::Dabao
                {
                    one_way.inc_coded::<bao1x_api::BoardTypeCoding>().expect("increment error");
                }
                // reset the system
                let mut rcurst = CSR::new(utra::sysctrl::HW_SYSCTRL_BASE as *mut u32);
                rcurst.wo(utra::sysctrl::SFR_RCURST0, 0x55AA);
                // this is actually unreachable
                None
            }*/
            None
        } else {
            None
        };

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

            reseed(&mut ro_trng, &av_trng, &mut seed);

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
        Self { ro_trng, av_trng, csprng }
    }

    pub fn generate_key(&mut self) -> [u8; 32] {
        let mut key = [0u8; 32];
        self.csprng.fill_bytes(&mut key);

        // reseed the csprng with more random data
        let mut seed = self.csprng.get_seed();
        reseed(&mut self.ro_trng, &self.av_trng, &mut seed);
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

fn reseed(trng: &mut bao1x_hal::sce::trng::Trng, av_trng: &Option<BioSharedState>, seed: &mut [u8]) {
    // XOR the seed with TRNG data
    for s in seed.chunks_mut(4) {
        let incoming = trng.get_u32().expect("TRNG error");
        for (s_byte, &ro_byte) in s.iter_mut().zip(incoming.to_le_bytes().iter()) {
            *s_byte ^= ro_byte;
        }
        // if the avalanche generator is available, use that too.
        if let Some(bio_ss) = av_trng {
            let mut raw32 = 0;
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
}

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
