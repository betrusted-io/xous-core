// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

#[cfg(any(feature = "debug-print", feature = "print-panics"))]
pub mod uart;

/// Precursor specific initialization.
pub fn init() {
    #[cfg(any(feature = "debug-print", feature = "print-panics"))]
    self::uart::init();
}