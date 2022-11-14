// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

#[cfg(any(feature="precursor", feature="renode"))]
pub mod precursor;

pub mod rand;

/// Platform specific initialization.
#[cfg(not(any(unix, windows)))]
pub fn init() {
    #[cfg(any(feature="precursor", feature="renode"))]
    self::precursor::init();
}