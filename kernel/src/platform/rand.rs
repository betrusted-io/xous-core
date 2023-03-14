// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

pub fn get_u32() -> u32 {
    // hosted rand code is coupled with arch code.
    #[cfg(any(windows, unix))]
    let rand = crate::arch::rand::get_u32();
    #[cfg(any(feature = "precursor", feature = "renode"))]
    let rand = crate::platform::precursor::rand::get_u32();
    #[cfg(any(feature = "atsama5d27"))]
    let rand = crate::platform::atsama5d2::rand::get_u32();

    rand
}
