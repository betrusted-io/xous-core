// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-FileCopyrightText: 2024 bunnie <bunnie@kosagi.com>
// SPDX-License-Identifier: Apache-2.0

use core::convert::TryInto;

use cramium_hal::sce;
use rand_chacha::rand_core::RngCore;
use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha8Rng;
use xous_kernel::{MemoryFlags, MemoryType, PID};

use crate::mem::MemoryManager;

/// The manually chosen virtual address has to be in the top 4MiB as it is the
/// only page shared among all processes.
///
/// See https://github.com/betrusted-io/xous-core/blob/master/docs/memory.md
pub const TRNG_KERNEL_ADDR: usize = 0xffce_0000;
pub static mut TRNG_KERNEL: Option<sce::trng::Trng> = None;
use core::sync::atomic::{AtomicU32, Ordering};
// these values are overwritten on boot with something out of the TRNG.
static LOCAL_RNG_STATE: [AtomicU32; 8] = [
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
];

/// Initialize TRNG driver.
///
/// Needed so that the kernel can allocate names.
/// This driver "owns" the hardware TRNG on the chip. Because there aren't two
/// TRNG ports on the Cramium implementation, it means that userspace applications
/// have to extract random numbers from the kernel through the SID request mechanism.
/// This is ostensibly harmless, if everything is implemented well, but if there is
/// a problem it does mean that the state of the kernel TRNG is disclosed to userspace.
/// What can you do though, this chip just has one TRNG.
///
/// The final implementation for user applications is recommended to have a supplementary
/// noise source in addition to the on-chip noise source, so that user applications
/// combine both the feed from the kernel and the supplementary source to seed a CSPRNG.
///
/// This particular application doesn't trust the on-chip TRNG so much: it takes
/// values from the TRNG and folds them into a seed pool that is then groomed by
/// a ChaCha8 RNG CSPRNG. Every request for a random number folds another 32 bits
/// from the TRNG into the CSPRNG pool, so even if the TRNG is fairly poor, quite
/// rapidly the pool of numbers should diverge. However, from the kernel's perspective,
/// it will function correctly even if the CSPRNG is fully deterministic, it just isn't
/// secure against attackers trying to guess a server address.

pub fn init() {
    // Map the TRNG so that we can allocate names
    MemoryManager::with_mut(|memory_manager| {
        memory_manager
            .map_range(
                utralib::generated::HW_TRNG_BASE as *mut u8,
                (TRNG_KERNEL_ADDR & !4095) as *mut u8,
                4096,
                PID::new(1).unwrap(),
                MemoryFlags::R | MemoryFlags::W,
                MemoryType::Default,
            )
            .expect("unable to map TRNG_KERNEL")
    });

    let mut trng_kernel = sce::trng::Trng::new(TRNG_KERNEL_ADDR);
    trng_kernel.setup_raw_generation(256);

    // Setup the initial seeds with raw data directly out of the TRNG. You'd think this
    // would give you 256 bits of entropy, but TRNGs can be imperfect so we assume it is
    // much less than this.
    for state in LOCAL_RNG_STATE.iter() {
        state.store(trng_kernel.get_u32().expect("TRNG error"), Ordering::SeqCst);
    }

    // move the trng_kernel object into the static mut state
    unsafe {
        TRNG_KERNEL = Some(trng_kernel);
    }

    // Accumulate more TRNG data, because I don't trust it.
    //
    // For the kernel, every 32 bits extracted is accompanied by a reseeding operation. Thus,
    // we can effectively improve the seed pool by just requesting u32's out of the pool and throwing
    // the result away.
    //
    // Each round pulls in 8*32 = 256 bits from HW TRNG 64 rounds of this would fold in 16,384 bits total.
    // Every subsequent call to get_u32() adds more entropy to the pool. This should give us about 100x safety
    // margin to a target of 128 bits of true entropy.
    //
    // The latest settings improvement on the TRNG makes me think this is extremely conservative, we
    // could probably do fine with a 10x margin; but, the operation is fast enough that this allows us
    // to be safe even if the TRNG is completely misconfigured.
    for _ in 0..64 {
        let _ = get_u32();
    }
}

/// Retrieve a true random `u32`. The data comes from the output of a CPRNG that is seeded by
/// the TRNG. Every pull of a `u32` adds more TRNG data to the CPRNG seed pool.
///
/// Note that ChaCha8's seed never changes: the output is just the seed + some generation distance from the
/// seed. Thus to change the seed, we have to extract it, XOR it with data, and put it back into the machine.
///
///   1. XOR in another round of HW TRNG data into the ChaCha8 state
///   2. Create the ChaCha8 cipher from the state
///   3. Run ChaCha8 and XOR its result into the state vector (obfuscate temporary dropouts in HW TRNG)
///   4. Store the state vector
pub fn get_u32() -> u32 {
    // Local storage for the seed.
    let mut seed = [0u8; 32];

    // Pull our true seed data from the static AtomicU32 variables. We have to do this because
    // we're the kernel: any machine persistent state is by definition, global mutable state.
    //
    // Mix in more data from the TRNG while recovering the state from the kernel holding area.
    for (s, state) in seed.chunks_mut(4).zip(LOCAL_RNG_STATE.iter()) {
        let incoming = get_raw_u32() ^ state.load(Ordering::SeqCst);
        for (s_byte, &incoming_byte) in s.iter_mut().zip(incoming.to_le_bytes().iter()) {
            *s_byte ^= incoming_byte;
        }
    }
    let mut cstrng = ChaCha8Rng::from_seed(seed);
    // Mix up the internal state with output from the CSPRNG. We do this because the TRNG bits
    // could be biased, and by running the CSPRNG forward based on the new seed, we have a chance to
    // diffuse any true entropy over all bits in the seed pool.
    for s in seed.chunks_mut(8) {
        for (s_byte, chacha_byte) in s.iter_mut().zip(cstrng.next_u64().to_le_bytes()) {
            *s_byte ^= chacha_byte;
        }
    }
    // Now extract one value from the CSPRNG: this is the number we reveal to the outside world.
    // It should not, in theory, be possible to deduce the seed from this value.
    let ret_val = cstrng.next_u32();

    // Save the mixed state into the kernel holding area
    for (s, state) in seed.chunks(4).zip(LOCAL_RNG_STATE.iter()) {
        state.store(u32::from_le_bytes(s.try_into().unwrap()), Ordering::SeqCst);
    }
    ret_val
}

/// This returns a raw, unwhitened, unprocessed TRNG value.
pub fn get_raw_u32() -> u32 {
    unsafe {
        TRNG_KERNEL
            .as_mut()
            .expect("TRNG_KERNEL driver not initialized")
            .get_u32()
            .expect("Error in random number generation")
    }
}
