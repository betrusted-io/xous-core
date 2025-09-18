// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

#[cfg(any(feature = "precursor", feature = "renode"))]
pub mod precursor;

#[cfg(any(feature = "atsama5d27"))]
pub mod atsama5d2;

#[cfg(any(any(feature = "bao1x")))]
pub mod bao1x;

#[cfg(any(any(feature = "bao1x")))]
pub use bao1x::rand;
#[cfg(not(any(feature = "bao1x")))]
pub mod rand;

/// Platform specific initialization.
#[cfg(not(any(unix, windows)))]
pub fn init() {
    #[cfg(any(feature = "precursor", feature = "renode"))]
    self::precursor::init();

    #[cfg(any(feature = "atsama5d27"))]
    self::atsama5d2::init();

    #[cfg(any(feature = "bao1x"))]
    self::bao1x::init();
}
