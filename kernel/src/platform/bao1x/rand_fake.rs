// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-FileCopyrightText: 2024 bunnie <bunnie@kosagi.com>
// SPDX-License-Identifier: Apache-2.0

use core::convert::TryInto;
use core::sync::atomic::{AtomicU32, Ordering};

use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::RngCore;
use rand_chacha::rand_core::SeedableRng;

// This is the sum total of state used for simulations
static LOCAL_RNG_STATE: [AtomicU32; 8] = [
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(1),
];

pub fn init() {}

/// This a fully deterministic PRNG that relies on Chacha8 for state evolution.
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

/// There is no TRNG in simulation, just return a constant and rely on the chacha8 whitener
pub fn get_raw_u32() -> u32 { 0 }
