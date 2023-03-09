// SPDX-FileCopyrightText: 2022 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2023 Foundation Devices, Inc <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(feature = "atsama5d27", allow(dead_code))]

extern "C" {
    pub fn start_kernel(
        stack: usize,       // r0
        ttbr: usize,        // r1
        entrypoint: usize,  // r2
        args: usize,        // r3
        ip: usize,
        rpt: usize,
        debug: bool,
        resume: bool,
    ) -> !;
}
