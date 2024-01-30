// SPDX-FileCopyrightText: 2022 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2023 Foundation Devices, Inc <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use core::panic::PanicInfo;

use armv7;

#[cfg(feature = "atsama5d27")]
#[panic_handler]
fn handle_panic(_arg: &PanicInfo) -> ! {
    crate::println!("{}", _arg);

    armv7::asm::bkpt(); // Invoke a debugger breakpoint

    loop {}
}
