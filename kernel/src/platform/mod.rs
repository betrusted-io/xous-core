// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

#[cfg(any(feature = "precursor", feature = "renode"))]
pub mod precursor;

#[cfg(any(feature = "atsama5d27"))]
pub mod atsama5d2;

pub mod rand;

/// Platform specific initialization.
#[cfg(not(any(unix, windows)))]
pub fn init() {
    #[cfg(any(feature = "precursor", feature = "renode"))]
    self::precursor::init();

    #[cfg(any(feature = "atsama5d27"))]
    self::atsama5d2::init();
}
