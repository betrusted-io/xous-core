// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-FileCopyrightText: 2024 bunnie <bunnie@kosagi.com>
// SPDX-License-Identifier: Apache-2.0

#[cfg(feature = "cramium-soc")]
pub mod rand;
#[cfg(feature = "cramium-fpga")]
pub mod rand_fake;
#[cfg(feature = "cramium-fpga")]
pub use rand_fake as rand;

#[cfg(any(feature = "debug-print", feature = "print-panics"))]
pub mod uart;

/// Precursor specific initialization.
pub fn init() {
    self::rand::init();
    #[cfg(any(feature = "debug-print", feature = "print-panics"))]
    self::uart::init();
}
