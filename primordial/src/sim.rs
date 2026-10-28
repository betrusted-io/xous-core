#![allow(dead_code)]

/// Registers used by the simulation infrastructure to signal simulation state changes.
/// Some of these registers (the "main" block in particular) does *not* exist in actual hardware.

pub mod main {
    pub const MAIN_NUMREGS: usize = 2;

    pub const DONE: utralib::Register = utralib::Register::new(0, 0x1);
    pub const DONE_DONE: utralib::Field = utralib::Field::new(1, 0, DONE);

    pub const IRQTEST0: utralib::Register = utralib::Register::new(1, 0xffff);
    pub const IRQTEST0_TRIGGER: utralib::Field = utralib::Field::new(16, 0, IRQTEST0);

    pub const HW_MAIN_BASE: usize = 0x40081000;
}

pub mod csrtest {
    pub const CSRTEST_NUMREGS: usize = 2;

    pub const WTEST: utralib::Register = utralib::Register::new(0, 0xffffffff);
    pub const WTEST_WTEST: utralib::Field = utralib::Field::new(32, 0, WTEST);

    pub const RTEST: utralib::Register = utralib::Register::new(1, 0xffffffff);
    pub const RTEST_RTEST: utralib::Field = utralib::Field::new(32, 0, RTEST);

    pub const HW_CSRTEST_BASE: usize = 0xe0003000;
}
