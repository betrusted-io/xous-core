// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

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
static LOCAL_RNG_STATE_LSB: AtomicU32 = AtomicU32::new(0);
static LOCAL_RNG_STATE_MSB: AtomicU32 = AtomicU32::new(0);

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

    // setup the initial seeds with stuff out of the TRNG.
    LOCAL_RNG_STATE_LSB.store(trng_kernel.get_u32().expect("TRNG error"), Ordering::SeqCst);
    LOCAL_RNG_STATE_MSB.store(trng_kernel.get_u32().expect("TRNG error"), Ordering::SeqCst);

    // move the trng_kernel object into the static mut state
    unsafe {
        TRNG_KERNEL = Some(trng_kernel);
    }

    // now run the CSPRNG a few cycles to fold more entropy into the pool before we start
    // to use it for reals. 16 iterations will extract 512 bits out of the TRNG, so even
    // if it is somewhat degraded, we'll be in good shape...
    for _ in 0..16 {
        let _ = get_u32();
    }
}

/// Retrieve random `u32`.
pub fn get_u32() -> u32 {
    let mut state = LOCAL_RNG_STATE_LSB.load(Ordering::SeqCst) as u64
        | (LOCAL_RNG_STATE_MSB.load(Ordering::SeqCst) as u64) << 32;

    // XOR in 32 bits of entropy from the HW TRNG pool.
    state ^= unsafe {
        TRNG_KERNEL
            .as_mut()
            .expect("TRNG_KERNEL driver not initialized")
            .get_u32()
            .expect("Error in random number generation")
    } as u64;

    let mut rng = ChaCha8Rng::seed_from_u64(state);
    let next_state = rng.next_u64();
    // next state is extracted before the returned value, to reduce state
    // leakage outside of the RNG.
    let ret_val = rng.next_u32();
    LOCAL_RNG_STATE_LSB.store(next_state as u32, Ordering::SeqCst);
    LOCAL_RNG_STATE_MSB.store((next_state >> 32) as u32, Ordering::SeqCst);
    ret_val
}
