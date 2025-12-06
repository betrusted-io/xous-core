use bao1x_api::HardenedBool;
use bao1x_api::POSSIBLE_ATTACKS;
use bao1x_api::bollard;
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::RngCore;
use rand_chacha::rand_core::SeedableRng;
use utralib::*;

use crate::acram::OneWayCounter;
use crate::sigcheck::erase_secrets;

/// This is the range of delays we will pick from whenever we attempt a random delay,
/// expressed as a number of bits.
const DELAY_MARGIN_BITS: u64 = 8;

/// This checks if we're in the PLL mode. This is security-relevant because when we're running
/// off the PLL it is (a) harder to glitch the clock, because skipping a beat on the external
/// crystal doesn't *stop* the PLL and (b) harder to glitch the code because the CPU is running
/// much faster and the timing of the glitch has to be more precise.
#[inline(always)]
pub fn check_pll() {
    let cgu = CSR::new(utra::sysctrl::HW_SYSCTRL_BASE as *mut u32);
    if cgu.r(utra::sysctrl::SFR_CGUSEL0) & 1 == 0 {
        // we're not on the PLL: die
        die();
    }
}

/// This is wrapped in an API call so we can replace this if we need to
/// Always inline the die call, makes it a bit harder to glitch over
#[inline(always)]
pub fn die() -> ! {
    let owc = OneWayCounter::new();
    // safety: this is safe because the offset is from a checked, pre-defined value
    unsafe {
        owc.inc(POSSIBLE_ATTACKS).unwrap();
    }
    // we'll react to POSSIBLE_ATTACKS in boot1 - I think we want to be able to update this based on
    // the actual number of false positives we see in the wild!
    loop {
        crate::sigcheck::die_no_std();
    }
}

/// This is a non-cryptographic source of randomness. It mostly just needs to be "fairly unique" on boot
/// so we don't go through all of the hardening steps used for e.g. the key generation initialization.
///
/// The main purpose of this is to generate random delays to harden against glitching.
pub struct Csprng {
    _ro_trng: crate::sce::trng::Trng,
    csprng: ChaCha8Rng,
    entropy_bank: u64,
}

impl Csprng {
    pub fn new() -> Self {
        let mut ro_trng = crate::sce::trng::Trng::new(utra::trng::HW_TRNG_BASE);
        ro_trng.setup_raw_generation(32); // this is actually repeated, but it's safe to repeat
        let slot_mgr = crate::acram::SlotManager::new();

        // start with the UUID of the chip. Not random, but different per chip
        let mut seed: [u8; 32] = slot_mgr.read(&bao1x_api::UUID).unwrap().try_into().unwrap();

        // XOR in some words from uninitialized RAM. This is only somewhat random. The purpose is to
        // protect against a TRNG that has been tampered by e.g. cutting some wires in the
        // ring oscillator, under the theory that it's much more annoying to have to tie most of the bits
        // going to the IFRAM to defeat this countermeasure.
        //
        // XOR'ing the seed 8 times over is just a random constant picked to try and improve the odds of
        // picking up *anything* different on a reboot. I think in practice if the attacker is forcing
        // a soft-reset, the RAM contents won't change much, but this does help in the case that a full
        // power cycle is needed to recover the chip (which in some glitching cases this is true).
        let ifram_slice: &[u8] =
            unsafe { core::slice::from_raw_parts(HW_IFRAM0_MEM as *const u8, seed.len() * 8) };
        for chunk in ifram_slice.chunks_exact(seed.len()) {
            for (a, b) in seed.iter_mut().zip(chunk.iter()) {
                *a ^= *b;
            }
        }

        // do a quick health check on the TRNG before using it. Looks for repeated values over 8 samples.
        // this will rule out e.g. TRNG output tied to 0 or 1, or an external feed just feeding it repetitive
        // data. About 1 in 500 million chance of triggering falsely if the TRNG is truly random.
        let mut values = [0u32; 8];
        for value in values.iter_mut() {
            *value = ro_trng.get_raw();
        }
        for _ in 0..values.len() {
            if values.contains(&ro_trng.get_raw()) {
                // don't proceed if we see a stuck value
                die();
            }
        }

        // seed from the TRNG
        for word in seed.chunks_mut(4) {
            word.copy_from_slice(&ro_trng.get_raw().to_ne_bytes());
        }

        // crate::println!("seed: {:x?}", seed); // used to eyeball that things are working correctly
        let mut csprng = ChaCha8Rng::from_seed(seed);

        let entropy_bank = csprng.next_u64();

        Self { _ro_trng: ro_trng, csprng, entropy_bank }
    }

    /// In-lining means there isn't just one spot to patch to remove random delays from the bootloader
    #[inline(always)]
    pub fn random_delay(&mut self) {
        if self.entropy_bank == 0 {
            self.entropy_bank = self.csprng.next_u64();
        }
        let delay = (self.entropy_bank & ((1 << DELAY_MARGIN_BITS) - 1)) as u32;
        self.entropy_bank >>= DELAY_MARGIN_BITS;
        for _ in 0..delay {
            // the delay is a bollard!
            bao1x_api::bollard!(die, 4);
        }
    }
}

#[inline(always)]
pub fn paranoid_mode() {
    // enter paranoid mode
    let mut sensor = CSR::new(utra::sensorc::HW_SENSORC_BASE as *mut u32);
    bao1x_api::bollard!(die, 4);
    sensor.wo(utra::sensorc::SFR_VDMASK0, 0);
    bao1x_api::bollard!(die, 4);
    sensor.wo(utra::sensorc::SFR_VDMASK1, 0); // putting 0 here makes the chip reset on glitch detect
    // redundant write in case of glitching

    // TODO: set up glue cells & interrupts
}

/// Mesh is a bit challenging to check on a fast-boot system. This is because it
/// takes ~100ms (maybe even as much as 150ms) for the test signal to propagate
/// all the way through the high-capacitance mesh, and we don't want to pay
/// the price of just dead-waiting for that long.
///
/// Also, as a matter of best practice, it should be checked twice: every wire in
/// the mesh should go to a 1, and also go to a 0. This is so an attacker can't
/// just tie-off the mesh wires to a fixed value to bypass the check. This translates
/// to checking in both `state` values, with the same pattern. Pattern is perfectly
/// fine to be `None`, there is a default pattern applied, but the API allows one
/// to be specified in case someone wants to play with patterns, too.
pub fn mesh_setup(state: bool, pattern: Option<u32>) {
    let mut mesh = CSR::new(utra::mesh::HW_MESH_BASE as *mut u32);
    bao1x_api::bollard!(die, 4);
    mesh.wo(utra::mesh::SFR_MLIE_CR_MLIE0, 0); // into drive/update mode
    mesh.wo(utra::mesh::SFR_MLIE_CR_MLIE1, 0);
    bao1x_api::bollard!(die, 4);
    let pattern = pattern.unwrap_or(0x5a5a_5a5a);
    if state {
        mesh.wo(utra::mesh::SFR_MLDRV_CR_MLDRV0, pattern);
        mesh.wo(utra::mesh::SFR_MLDRV_CR_MLDRV1, !pattern);
    } else {
        // inverse pattern of above
        mesh.wo(utra::mesh::SFR_MLDRV_CR_MLDRV0, !pattern);
        mesh.wo(utra::mesh::SFR_MLDRV_CR_MLDRV1, pattern);
    }
    // now 100ms must elapse until checking
    // this is left as an exercise to the reader.
}

/// For lack of a better place to put the docu on this, the two spots where
/// the mesh is checked are:
/// 1. immediately on boot in boot0, state `false` is applied. On entry to `boot1`, it is checked. There is a
///    ~100ms delay between these states.
/// 2. After checking in `boot1`, the inverse `true` state is applied. On entry to the `loader`, it is
///    checked. Another ~100ms delay between the states.
/// The reason for the delay is the time it takes to do the ed25519 signature
/// check. There is a possibility the signature check goes too fast (if the
/// binary blobs are very small, the hash function will complete fast), but
/// I think in practice it should be OK.
///
/// Returns `HardenedBool::TRUE` if the mesh check is PASSING. Also returns the
/// raw failures count as an additional check, so that a glitcher can't simply
/// glitch past the translation of failure count into a HardenedBool.
pub fn mesh_check() -> (HardenedBool, u32) {
    let mut mesh = CSR::new(utra::mesh::HW_MESH_BASE as *mut u32);
    mesh.wo(utra::mesh::SFR_MLIE_CR_MLIE0, 0xffff_ffff); // into measure mode
    mesh.wo(utra::mesh::SFR_MLIE_CR_MLIE1, 0xffff_ffff);
    // read all the status registers; if any bit is set, then, there's a failure
    let mut failures = 0;
    // read the read-out registers 6 times over. No failure == 0. So, any failure will
    // lead to a non-zero value in failures, while a passing test will give you 0. This
    // means a glitcher has to bypass the read check six times, *or* completely skip
    // over the read entirely. The theory is that a controllable glitch is getting you forward
    // just a few instructions; I think the loop is big enough that it's hard to get a
    // controlled glitch that simply takes you over it.
    bollard!(die, 4);
    for _ in 0..6 {
        bollard!(die, 4);
        for i in 0..8 {
            bao1x_api::bollard!(die, 4);
            failures += unsafe { mesh.base().add(i + 8).read_volatile() };
        }
    }
    bollard!(die, 4);
    (if failures == 0 { HardenedBool::TRUE } else { HardenedBool::FALSE }, failures)
}

/// Applies a reaction policy to the mesh check. In this case, if we're not
/// in paranoid mode, just note the attack; if in paranoid mode, shut the chip down.
///
/// Considering adding a secret-wipe in response to attack, but, the timing on
/// mesh checking is not solid enough to apply that policy yet. Need to get more
/// hardware out there to make sure there is adequate margin between the mesh setup
/// and mesh check phase. If "normal users" find that POSSIBLE_ATTACKS does not
/// increment after a reasonable amount of time in the field, then, we can apply
/// a firmware updated that makes the reactive policy stronger.
pub fn mesh_check_and_react(csprng: &mut Csprng, one_way: &OneWayCounter) {
    bollard!(die, 4);
    csprng.random_delay();
    let (paranoid1, paranoid2) =
        one_way.hardened_get2(bao1x_api::PARANOID_MODE, bao1x_api::PARANOID_MODE_DUPE).unwrap();
    bollard!(die, 4);
    csprng.random_delay();
    let (passing, failures) = mesh_check();
    csprng.random_delay();
    match passing.is_true() {
        Some(true) => (),
        Some(false) => {
            bollard!(die, 4);
            if paranoid1 == 0 {
                unsafe {
                    one_way.inc(bao1x_api::POSSIBLE_ATTACKS).unwrap();
                }
            } else {
                die();
            }
            // b is checked *in sequence* because a could be glitched over
            csprng.random_delay();
            bollard!(die, 4);
            if paranoid2 == 0 {
                unsafe {
                    one_way.inc(bao1x_api::POSSIBLE_ATTACKS).unwrap();
                }
            } else {
                die();
            }
        }
        None => die(),
    }
    // repeat the above code - failures vs passing is a redundant check
    csprng.random_delay();
    if failures != 0 {
        bollard!(die, 4);
        if paranoid1 == 0 {
            unsafe {
                one_way.inc(bao1x_api::POSSIBLE_ATTACKS).unwrap();
            }
        } else {
            die();
        }
    }
    csprng.random_delay();
    if failures != 0 {
        // b is checked *in sequence* because a could be glitched over
        csprng.random_delay();
        bollard!(die, 4);
        if paranoid2 == 0 {
            unsafe {
                one_way.inc(bao1x_api::POSSIBLE_ATTACKS).unwrap();
            }
        } else {
            die();
        }
    }
}

pub fn apply_attack_policy(csprng: &mut Csprng, one_way: &OneWayCounter) {
    // use a large threshold initially, under the theory that e.g. fault injections tend
    // to take thousands of iterations to succeed, and we really don't want to accidentally
    // wipe customer data. For now, the policy is to only wipe to those who have consented
    // by turning on paranoid mode.
    //
    // Pick a number that has a high hamming distance from 0 - and so, prefer e.g. 127 vs 128.
    const WIPE_THRESHOLD: u32 = 127;

    bollard!(die, 4);
    csprng.random_delay();
    let (paranoid1, paranoid2) =
        one_way.hardened_get2(bao1x_api::PARANOID_MODE, bao1x_api::PARANOID_MODE_DUPE).unwrap();
    bollard!(die, 4);
    csprng.random_delay();
    if paranoid1 != 0 {
        bollard!(die, 4);
        if one_way.get(bao1x_api::POSSIBLE_ATTACKS).unwrap() > WIPE_THRESHOLD {
            bollard!(die, 4);
            erase_secrets(&mut Some(csprng)).ok();
            die();
        }
    }
    csprng.random_delay();
    // checked twice to force a double-glitch to fully bypass this
    if paranoid2 != 0 {
        bollard!(die, 4);
        if one_way.get(bao1x_api::POSSIBLE_ATTACKS).unwrap() > WIPE_THRESHOLD {
            bollard!(die, 4);
            erase_secrets(&mut Some(csprng)).ok();
            die();
        }
    }
}
