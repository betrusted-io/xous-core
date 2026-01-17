use bao1x_api::HardenedBool;
use bao1x_api::POSSIBLE_ATTACKS;
use bao1x_api::bollard;
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::RngCore;
use rand_chacha::rand_core::SeedableRng;
use utralib::*;

use crate::acram::OneWayCounter;
use crate::sigcheck::erase_secrets;

/// use a large threshold initially, under the theory that e.g. fault injections tend
/// to take thousands of iterations to succeed, and we really don't want to accidentally
/// wipe customer data. For now, the policy is to only wipe to those who have consented
/// by turning on paranoid mode.
///
/// Pick a number that has a high hamming distance from 0 - and so, prefer e.g. 127 vs 128.
pub const WIPE_THRESHOLD: u32 = 127;
/// This number is empirically tuned. The main issue is we don't want to notch up the
/// counter during "slow shutdowns" where the power supply falls gradually enough that
/// the glitch detector can fire.
const DISTURB_THRESHOLD: u32 = 2;

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
    crate::println_d!("die!die!die!die!");
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
}

/// This call clears any current alarms and resets sensors, but does not
/// enable (or re-enable) interrupts
pub fn reset_sensors() {
    let glue = CSR::new(utra::gluechain::HW_GLUECHAIN_BASE as *mut u32);
    let mut sensor = CSR::new(utra::sensorc::HW_SENSORC_BASE as *mut u32);
    // reset the glue chain. Hard-coded constants are used here because
    // the UTRA did not extract these constants correctly.
    bao1x_api::bollard!(die, 4);
    unsafe {
        glue.base().add(4).write_volatile(0);
        glue.base().add(5).write_volatile(0);
    }
    // wait for reset to propagate
    for _ in 0..100 {
        bao1x_api::bollard!(4);
    }
    // re-arm
    bao1x_api::bollard!(die, 4);
    unsafe {
        glue.base().add(0).write_volatile(0x0);
        glue.base().add(1).write_volatile(0x0);
        glue.base().add(4).write_volatile(0xFFFF_FFFF);
        glue.base().add(5).write_volatile(0xFFFF_FFFF);
        glue.base().add(6).write_volatile(0x0);
        glue.base().add(7).write_volatile(0x0);
    }

    // this enables all the sensors to create non-resetting interrupts
    bao1x_api::bollard!(die, 4);
    sensor.wo(utra::sensorc::SFR_VDFR, 0x3f); // clear any voltage alarms
    sensor.wo(utra::sensorc::SFR_VDMASK0, 0);
    // DONT TOUCH THIS - paranoid mode sets this, and can set it BEFORE reset_sensors is called!
    // sensor.wo(utra::sensorc::SFR_VDMASK1, 0x3f);
    sensor.wo(utra::sensorc::SFR_LDIP_FD, 0x1ff); // setup filtering parameters
    sensor.wo(utra::sensorc::SFR_LDCFG, 0xc);
    sensor.wo(utra::sensorc::SFR_LDMASK, 0x0); // turn on both sensors
    bao1x_api::bollard!(die, 4);
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
                crate::println_d!("mesh");
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
                crate::println_d!("mesh");
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

pub fn glitch_handler(attacks_since_boot: u32) {
    let owc = OneWayCounter::new();
    let mut irq13 = CSR::new(utra::irqarray13::HW_IRQARRAY13_BASE as *mut u32);
    let mut irq15 = CSR::new(utra::irqarray15::HW_IRQARRAY15_BASE as *mut u32);
    // Only secirq is recoverable: this is a sensor that has triggered. This is the only
    // type of "attack" that we envision would need recovering from, because the sensors
    // could be too sensitive. Thus, for all other IRQ types, just halt.
    let reason = irq13.r(utra::irqarray13::EV_PENDING);

    let mut was_sensor = false;
    // figure out which subsystem glitched
    let sensor = irq15.r(utra::irqarray15::EV_PENDING);
    bao1x_api::bollard!(crate::sigcheck::die_no_std, 4);
    if reason & 0x8 != 0 {
        const GLUE_MASK: u32 = 1 << 0;
        const SENSOR_MASK: u32 = 1 << 1;
        const MESH_MASK: u32 = 1 << 2;
        if sensor & MESH_MASK != 0 {
            // mesh events are handled by a separate routine
            #[cfg(feature = "debug-countermeasures")]
            crate::println!("mesh");
        }
        if sensor & SENSOR_MASK != 0 || sensor & GLUE_MASK != 0 {
            #[cfg(feature = "debug-countermeasures")]
            crate::println!("Sensor {:x}", sensor);
            // this resets all the sensors that *can* be reset
            crate::hardening::reset_sensors();
            was_sensor = true;
        }
        // this transfers the code to a set of GPIOs that can be observed on dabao
        #[cfg(feature = "debug-countermeasures")]
        {
            let iox = crate::iox::Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
            iox.set_gpio_bank(bao1x_api::IoxPort::PC, (sensor as u16 & 0x7) << 9, 0b0000_1110_0000_0000);
        }

        irq15.wo(utra::irqarray15::EV_PENDING, sensor);
    }
    bao1x_api::bollard!(crate::sigcheck::die_no_std, 4);
    if !was_sensor {
        bao1x_api::bollard!(crate::sigcheck::die_no_std, 4);
        // mesh sensor will trigger while settling measurements; and there is no way to mask it
        irq13.wo(utra::irqarray13::EV_PENDING, reason);
        bao1x_api::bollard!(crate::sigcheck::die_no_std, 4);
        return;
    }
    bao1x_api::bollard!(crate::sigcheck::die_no_std, 4);

    // safety: this is safe because the offset is from a checked, pre-defined value
    let attacks = owc.get(bao1x_api::POSSIBLE_ATTACKS).unwrap_or(WIPE_THRESHOLD + 1);

    // on the paranoid path: wipe secrets if we exceed a wipe threshold
    // this path is measuring the accumulated attacks so far - the assumption is that a glitch attack
    // will have to "search" for the right timing with multiple tries, and thus this would rapidly be
    // triggered while doing the search.
    let (paranoid1, paranoid2) =
        owc.hardened_get2(bao1x_api::PARANOID_MODE, bao1x_api::PARANOID_MODE_DUPE).unwrap();
    bao1x_api::bollard!(crate::sigcheck::die_no_std, 4);
    if paranoid1 != 0 {
        if attacks > WIPE_THRESHOLD {
            crate::sigcheck::erase_secrets(&mut None).ok();
        }
        crate::sigcheck::die_no_std();
    }
    if paranoid2 != 0 {
        if attacks > WIPE_THRESHOLD {
            crate::sigcheck::erase_secrets(&mut None).ok();
        }
        crate::sigcheck::die_no_std();
    }

    // on the non-paranoid path: just wipe NV elements & halt if we see a lower threshold of attacks
    bao1x_api::bollard!(crate::sigcheck::die_no_std, 4);
    // this could be glitched over, but you'd have to have a repeatable glitch to skip it every time
    if attacks_since_boot > DISTURB_THRESHOLD {
        // don't start logging possible attacks until we've seen more than DISTURB_THRESHOLD
        // Reason: the sensors are naturally triggered during a slow power-off or power-on
        // and we want to filter out those transients.
        unsafe {
            // the increment is just OK'd because if for some reason it fails we still want
            // to execute the following code.
            owc.inc(bao1x_api::POSSIBLE_ATTACKS).ok();
        }
        // even output something on the DUART - this could be used as a trigger to prevent shutdown
        // but at this point diagnostics are useful for seeing why the system shut down unexpectedly.
        #[cfg(feature = "debug-countermeasures")]
        crate::println!("Attack thresh");
        crate::sigcheck::die_no_std();
    }

    bao1x_api::bollard!(crate::sigcheck::die_no_std, 4);
    // non-sensor reasons are non-recoverable: we'll just repeatedly re-enter the interrupt state
    // in this case, just erase the NV data and die.
    if reason & !0x8 != 0 {
        #[cfg(feature = "debug-countermeasures")]
        crate::println!("Halt {:x}", reason);
        crate::sigcheck::die_no_std();
    }
    irq13.wo(utra::irqarray13::EV_PENDING, reason);
}

/// check that pub keys in the images match those burned into the indelible key area
/// glitch_safety: I'd imagine that glitching in this routine would lead to good_compare being `false`,
/// so no additional hardening is done.
pub fn compare_refkeys(
    owc: &OneWayCounter,
    slot_mgr: &crate::acram::SlotManager,
    csprng: &mut Csprng,
    pubkey_ptr: *const bao1x_api::signatures::SignatureInFlash,
    fail_counter: usize,
) -> HardenedBool {
    let reference_keys =
        [bao1x_api::BAO1_PUBKEY, bao1x_api::BAO2_PUBKEY, bao1x_api::BETA_PUBKEY, bao1x_api::DEV_PUBKEY];
    // check that the pub keys match those burned into the indelible key area
    // glitch_safety: I'd imagine that glitching in this routine would lead to good_compare being `false`,
    // so no additional hardening is done.
    let pk_src: &bao1x_api::signatures::SignatureInFlash = unsafe { pubkey_ptr.as_ref().unwrap() };
    let mut good_compare = HardenedBool::TRUE;
    for (boot0_key, ref_key) in pk_src.sealed_data.pubkeys.iter().zip(reference_keys.iter()) {
        let ref_data = slot_mgr.read(&ref_key).unwrap();
        if ref_data != &boot0_key.pk {
            good_compare = HardenedBool::FALSE;
        }
    }
    csprng.random_delay();
    // The IFR (indelible) copy is weird, because the highest byte doesn't match (it's actually a flag that
    // indicates the region has to be write protected). Thus, the IFR keys are a set of four, disjointed
    // 31-byte memory areas, plus a collection of 4 bytes that correspond to the missing MSB.
    let ifr_keys = [
        unsafe { core::slice::from_raw_parts(0x6040_01A0 as *const u8, 31) },
        unsafe { core::slice::from_raw_parts(0x6040_01C0 as *const u8, 31) },
        unsafe { core::slice::from_raw_parts(0x6040_01E0 as *const u8, 31) },
        unsafe { core::slice::from_raw_parts(0x6040_0200 as *const u8, 31) },
    ];
    let ifr_msb = unsafe { core::slice::from_raw_parts(0x6040_0240 as *const u8, 4) };
    for (i, (boot0_key, ref_key)) in pk_src.sealed_data.pubkeys.iter().zip(ifr_keys).enumerate() {
        if ref_key != &boot0_key.pk[..31] || ifr_msb[i] != boot0_key.pk[31] {
            good_compare = HardenedBool::FALSE;
        }
    }
    csprng.random_delay();
    match good_compare.is_true() {
        Some(false) => {
            bollard!(die, 4);
            // don't over-increment this to avoid RRAM wear-out. However, do allow it to increment to higher
            // than 1 so we have a higher hamming distance on the counter than a single bit.
            if owc.get(fail_counter).unwrap() < 15 {
                // safety: the offset is from a pre-validated constant, which meets the safety requirement
                unsafe {
                    owc.inc(fail_counter).unwrap();
                }
            }
            // erase secrets (or ensure they are erased) if the boot pubkey doesn't check out.
            bollard!(die, 4);
            crate::sigcheck::erase_secrets(&mut Some(csprng)).inspect_err(|e| crate::println!("{}", e)).ok(); // "ok" because the expected error is a check on logic/configuration bugs, not attacks
        }
        Some(true) => (),
        None => die(),
    }
    bollard!(die, 4);
    good_compare
}
