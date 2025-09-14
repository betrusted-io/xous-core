use utralib::*;
use xous_bio_bdma::*;

pub fn setup(bio_ss: &mut BioSharedState, trng_pin: u8) {
    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x0);
    bio_ss.load_code(avtrng_bio_code(), 0, BioCore::Core0);

    // don't use QDIV
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV0, 0x1_0000);

    // use extclock on channel 0, tied to the trng pin
    bio_ss.bio.wo(
        utra::bio_bdma::SFR_EXTCLOCK,
        bio_ss.bio.ms(utra::bio_bdma::SFR_EXTCLOCK_USE_EXTCLK, 0b0001)
            | bio_ss.bio.ms(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_0, trng_pin as u32),
    );

    // start the machine
    bio_ss.set_core_run_states([true, false, false, true]);
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, trng_pin as u32); // start the sampling
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
