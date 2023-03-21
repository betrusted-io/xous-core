// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

pub mod aic;
pub mod pmc;
pub mod rand;
pub mod uart;
pub mod pio;

/// atsama5d2 specific initialization.
pub fn init() {
    // The order of these init calls is important,
    // don't rearrange them if you don't know what you're doing!

    self::pmc::init();
    self::pmc::enable_tc0();
    self::pmc::enable_pio();
    self::pmc::enable_aic();

    self::pio::init();
    self::pio::init_lcd_pins();

    self::pmc::enable_lcdc();

    self::aic::init();
    self::rand::init();
    self::uart::init();
}
