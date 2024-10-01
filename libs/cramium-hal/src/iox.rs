use utralib::generated::utra::iox;
use utralib::generated::*;

/// Create an immutable-friendly shared CSR object for the I/O ports. The consequence
/// of this is that we don't get the borrow checker to check the global shared state of
/// the I/O pins status, but the truth is, it's global shared state and there's nothing
/// you can do about it. Might as well make the APIs cleaner so we have less work to
/// do maintaining APIs and can pay attention to sharing/allocating the shared state
/// correctly.
#[derive(Debug)]
pub struct SharedCsr<T> {
    pub base: *const T,
}
impl<T> SharedCsr<T>
where
    T: core::convert::TryFrom<usize> + core::convert::TryInto<usize> + core::default::Default,
{
    pub fn new(base: *const T) -> Self { SharedCsr { base: base as *const T } }

    pub unsafe fn base(&self) -> *mut T { self.base as *mut T }

    pub fn clone(&self) -> Self { SharedCsr { base: self.base.clone() } }

    /// Read the contents of this register
    pub fn r(&self, reg: Register) -> T {
        // prevent re-ordering
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        unsafe { usize_base.add(reg.offset()).read_volatile() }.try_into().unwrap_or_default()
    }

    /// Read a field from this CSR
    pub fn rf(&self, field: Field) -> T {
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        ((unsafe { usize_base.add(field.register().offset()).read_volatile() } >> field.offset())
            & field.mask())
        .try_into()
        .unwrap_or_default()
    }

    /// Read-modify-write a given field in this CSR
    pub fn rmwf(&self, field: Field, value: T) {
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        let value_as_usize: usize = value.try_into().unwrap_or_default() << field.offset();
        let previous = unsafe { usize_base.add(field.register().offset()).read_volatile() }
            & !(field.mask() << field.offset());
        unsafe { usize_base.add(field.register().offset()).write_volatile(previous | value_as_usize) };
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }

    /// Write a given field without reading it first
    pub fn wfo(&self, field: Field, value: T) {
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        let value_as_usize: usize = (value.try_into().unwrap_or_default() & field.mask()) << field.offset();
        unsafe { usize_base.add(field.register().offset()).write_volatile(value_as_usize) };
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }

    /// Write the entire contents of a register without reading it first
    pub fn wo(&self, reg: Register, value: T) {
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        unsafe { usize_base.add(reg.offset()).write_volatile(value_as_usize) };
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }

    /// Zero a field from a provided value
    pub fn zf(&self, field: Field, value: T) -> T {
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        (value_as_usize & !(field.mask() << field.offset())).try_into().unwrap_or_default()
    }

    /// Shift & mask a value to its final field position
    pub fn ms(&self, field: Field, value: T) -> T {
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        ((value_as_usize & field.mask()) << field.offset()).try_into().unwrap_or_default()
    }
}

macro_rules! set_pin_in_bank {
    ($self:expr, $register:expr, $port:expr, $pin:expr, $val:expr) => {{
        assert!($pin < 16, "pin must be in range of 0-15");
        // safety: it is safe to create this raw pointer and cast it because the
        // code below does not add to the raw pointer outside of its approved range,
        // thanks to the constraints placed by the enum type of IoxPort.
        unsafe {
            let ptr = $self.csr.base();

            ptr.add($register.offset() + $port as usize).write_volatile(
                (ptr.add($register.offset() + $port as usize).read_volatile() & !(1u32 << ($pin as u32)))
                    | (($val as u32) << ($pin as u32)),
            )
        }
    }};
}

#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Debug, Copy, Clone, num_derive::FromPrimitive, num_derive::ToPrimitive)]
#[repr(u32)]
pub enum IoxPort {
    PA = 0,
    PB = 1,
    PC = 2,
    PD = 3,
    PE = 4,
    PF = 5,
}

#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Debug)]
#[repr(u32)]
pub enum IoxFunction {
    Gpio = 0b00,
    AF1 = 0b01,
    AF2 = 0b10,
    AF3 = 0b11,
}

#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Debug)]
#[repr(u32)]
pub enum IoxDriveStrength {
    Drive2mA = 0b00,
    Drive4mA = 0b01,
    Drive8mA = 0b10,
    Drive12mA = 0b11,
}

#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Debug)]
#[repr(u32)]
pub enum IoxDir {
    Input = 0,
    Output = 1,
}

#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Debug)]
#[repr(u32)]
pub enum IoxEnable {
    Disable = 0,
    Enable = 1,
}

#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Debug, Copy, Clone)]
#[repr(u32)]
pub enum IoxValue {
    Low = 0,
    High = 1,
}
/// The From trait for IoxValue takes any non-zero value and interprets it as "high".
impl From<u32> for IoxValue {
    fn from(value: u32) -> Self { if value == 0 { IoxValue::Low } else { IoxValue::High } }
}

/// Use a trait that will allow us to share code between both `std` and `no-std` implementations
pub trait IoSetup {
    fn setup_pin(
        &self,
        port: IoxPort,
        pin: u8,
        direction: Option<IoxDir>,
        function: Option<IoxFunction>,
        schmitt_trigger: Option<IoxEnable>,
        pullup: Option<IoxEnable>,
        slow_slew: Option<IoxEnable>,
        strength: Option<IoxDriveStrength>,
    );
}

pub struct Iox {
    pub csr: SharedCsr<u32>,
}

impl Iox {
    pub fn new(base_address: *mut u32) -> Self { Iox { csr: SharedCsr::new(base_address) } }

    pub fn set_gpio_dir(&self, port: IoxPort, pin: u8, direction: IoxDir) {
        set_pin_in_bank!(self, iox::SFR_GPIOOE_CRGOE0, port, pin, direction)
    }

    pub fn set_gpio_pullup(&self, port: IoxPort, pin: u8, enable: IoxEnable) {
        set_pin_in_bank!(self, iox::SFR_GPIOPU_CRGPU0, port, pin, enable)
    }

    pub fn set_gpio_pin(&self, port: IoxPort, pin: u8, value: IoxValue) {
        set_pin_in_bank!(self, iox::SFR_GPIOOUT_CRGO0, port, pin, value)
    }

    pub fn set_gpio_bank(&self, port: IoxPort, value: u16, mask: u16) {
        // safety: it is safe to manipulate a raw pointer because IoxPort constrains
        // the offset to be within range.
        unsafe {
            let ptr = self.csr.base();
            ptr.add(iox::SFR_GPIOOUT_CRGO0.offset() + port as usize).write_volatile(
                ptr.add(iox::SFR_GPIOOUT_CRGO0.offset() + port as usize).read_volatile() & !(mask as u32)
                    | value as u32,
            )
        }
    }

    pub fn set_gpio_schmitt_trigger(&self, port: IoxPort, pin: u8, enable: IoxEnable) {
        set_pin_in_bank!(self, iox::SFR_CFG_SCHM_CR_CFG_SCHMSEL0, port, pin, enable)
    }

    pub fn set_slow_slew_rate(&self, port: IoxPort, pin: u8, enable: IoxEnable) {
        set_pin_in_bank!(self, iox::SFR_CFG_SLEW_CR_CFG_SLEWSLOW0, port, pin, enable)
    }

    pub fn get_gpio_pin(&self, port: IoxPort, pin: u8) -> IoxValue {
        assert!(pin < 16, "pin must be in range of 0-15");
        // safety: it is safe to create this raw pointer and cast it because the
        // code below does not add to the raw pointer outside of its approved range,
        // thanks to the constraints placed by the enum type of IoxPort.
        unsafe {
            let oe_ptr = self.csr.base();
            IoxValue::from(
                oe_ptr.add(iox::SFR_GPIOIN_SRGI0.offset() + port as usize).read_volatile()
                    & (1u32 << (pin as u32)),
            )
        }
    }

    pub fn get_gpio_bank(&self, port: IoxPort) -> u16 {
        // safety: it is safe to create this raw pointer and cast it because the
        // code below does not add to the raw pointer outside of its approved range,
        // thanks to the constraints placed by the enum type of IoxPort.
        unsafe {
            let oe_ptr = self.csr.base();
            oe_ptr.add(iox::SFR_GPIOIN_SRGI0.offset() + port as usize).read_volatile() as u16
        }
    }

    pub fn set_alternate_function(&self, port: IoxPort, pin: u8, function: IoxFunction) {
        assert!(pin < 16, "pin must be in range of 0-15");
        match port {
            IoxPort::PA => {
                if pin < 8 {
                    self.csr.wo(
                        iox::SFR_AFSEL_CRAFSEL0,
                        self.csr.r(iox::SFR_AFSEL_CRAFSEL0) & !(0b11 << (pin * 2))
                            | (function as u32) << (pin * 2),
                    )
                } else if pin >= 8 && pin < 16 {
                    self.csr.wo(
                        iox::SFR_AFSEL_CRAFSEL1,
                        self.csr.r(iox::SFR_AFSEL_CRAFSEL1) & !(0b11 << ((pin - 8) * 2))
                            | (function as u32) << ((pin - 8) * 2),
                    )
                }
            }
            IoxPort::PB => {
                if pin < 8 {
                    self.csr.wo(
                        iox::SFR_AFSEL_CRAFSEL2,
                        self.csr.r(iox::SFR_AFSEL_CRAFSEL2) & !(0b11 << (pin * 2))
                            | (function as u32) << (pin * 2),
                    )
                } else if pin >= 8 && pin < 16 {
                    self.csr.wo(
                        iox::SFR_AFSEL_CRAFSEL3,
                        self.csr.r(iox::SFR_AFSEL_CRAFSEL3) & !(0b11 << ((pin - 8) * 2))
                            | (function as u32) << ((pin - 8) * 2),
                    )
                }
            }
            IoxPort::PC => {
                if pin < 8 {
                    self.csr.wo(
                        iox::SFR_AFSEL_CRAFSEL4,
                        self.csr.r(iox::SFR_AFSEL_CRAFSEL4) & !(0b11 << (pin * 2))
                            | (function as u32) << (pin * 2),
                    )
                } else if pin >= 8 && pin < 16 {
                    self.csr.wo(
                        iox::SFR_AFSEL_CRAFSEL5,
                        self.csr.r(iox::SFR_AFSEL_CRAFSEL5) & !(0b11 << ((pin - 8) * 2))
                            | (function as u32) << ((pin - 8) * 2),
                    )
                }
            }
            IoxPort::PD => {
                if pin < 8 {
                    self.csr.wo(
                        iox::SFR_AFSEL_CRAFSEL6,
                        self.csr.r(iox::SFR_AFSEL_CRAFSEL6) & !(0b11 << (pin * 2))
                            | (function as u32) << (pin * 2),
                    )
                } else if pin >= 8 && pin < 16 {
                    self.csr.wo(
                        iox::SFR_AFSEL_CRAFSEL7,
                        self.csr.r(iox::SFR_AFSEL_CRAFSEL7) & !(0b11 << ((pin - 8) * 2))
                            | (function as u32) << ((pin - 8) * 2),
                    )
                }
            }
            IoxPort::PE => {
                if pin < 8 {
                    self.csr.wo(
                        iox::SFR_AFSEL_CRAFSEL8,
                        self.csr.r(iox::SFR_AFSEL_CRAFSEL8) & !(0b11 << (pin * 2))
                            | (function as u32) << (pin * 2),
                    )
                } else if pin >= 8 && pin < 16 {
                    self.csr.wo(
                        iox::SFR_AFSEL_CRAFSEL9,
                        self.csr.r(iox::SFR_AFSEL_CRAFSEL9) & !(0b11 << ((pin - 8) * 2))
                            | (function as u32) << ((pin - 8) * 2),
                    )
                }
            }
            IoxPort::PF => {
                if pin < 8 {
                    self.csr.wo(
                        iox::SFR_AFSEL_CRAFSEL10,
                        self.csr.r(iox::SFR_AFSEL_CRAFSEL10) & !(0b11 << (pin * 2))
                            | (function as u32) << (pin * 2),
                    )
                } else if pin >= 8 && pin < 16 {
                    self.csr.wo(
                        iox::SFR_AFSEL_CRAFSEL11,
                        self.csr.r(iox::SFR_AFSEL_CRAFSEL11) & !(0b11 << ((pin - 8) * 2))
                            | (function as u32) << ((pin - 8) * 2),
                    )
                }
            }
        }
    }

    /// This function takes a 32-bit bitmask, corresponding to PIO 31 through 0, where
    /// a `1` indicates to map that PIO to a GPIO.
    ///
    /// This function will automatically remap the AF and PIO settings for the PIO pins
    /// specified in the bitmask, corresponding to the PIO GPIO pin number. If a `0` is
    /// present in a bit position, it will turn off the PIO mux, but not change the AF setting.
    ///
    /// Returns: a 32-entry array which records which GPIO bank and pin number was affected
    /// by the mapping request. The index of the array corresponds to the bit position in
    /// the bitmask. You may use this to pass as arguments to further functions
    /// that do things like control slew rate or apply pull-ups.
    pub fn set_ports_from_pio_bitmask(&self, enable_bitmask: u32) -> [Option<(IoxPort, u8)>; 32] {
        let mut mapping: [Option<(IoxPort, u8)>; 32] = [None; 32];

        for i in 0..32 {
            let enable = ((enable_bitmask >> i) & 1) != 0;

            if enable {
                let map: Option<(IoxPort, u8)> = match i {
                    // For NTO the ports should be in correct order
                    0 => Some((IoxPort::PB, 0)),
                    1 => Some((IoxPort::PB, 1)),
                    2 => Some((IoxPort::PB, 2)),
                    3 => Some((IoxPort::PB, 3)),
                    4 => Some((IoxPort::PB, 4)),
                    5 => Some((IoxPort::PB, 5)),
                    6 => Some((IoxPort::PB, 6)),
                    7 => Some((IoxPort::PB, 7)),
                    8 => Some((IoxPort::PB, 8)),
                    9 => Some((IoxPort::PB, 9)),
                    10 => Some((IoxPort::PB, 10)),
                    11 => Some((IoxPort::PB, 11)),
                    12 => Some((IoxPort::PB, 12)),
                    13 => Some((IoxPort::PB, 13)),
                    14 => Some((IoxPort::PB, 14)),
                    15 => Some((IoxPort::PB, 15)),
                    // Port C
                    16 => Some((IoxPort::PC, 0)),
                    17 => Some((IoxPort::PC, 1)),
                    18 => Some((IoxPort::PC, 2)),
                    19 => Some((IoxPort::PC, 3)),
                    20 => Some((IoxPort::PC, 4)),
                    21 => Some((IoxPort::PC, 5)),
                    22 => Some((IoxPort::PC, 6)),
                    23 => Some((IoxPort::PC, 7)),
                    24 => Some((IoxPort::PC, 8)),
                    25 => Some((IoxPort::PC, 9)),
                    26 => Some((IoxPort::PC, 10)),
                    27 => Some((IoxPort::PC, 11)),
                    28 => Some((IoxPort::PC, 12)),
                    29 => Some((IoxPort::PC, 13)),
                    30 => Some((IoxPort::PC, 14)),
                    31 => Some((IoxPort::PC, 15)),
                    _ => None,
                };
                if let Some((port, pin)) = map {
                    // AF1 must be selected
                    self.set_alternate_function(port, pin, IoxFunction::AF1);
                    // then the PIO register must have its bit flipped to 1
                    self.csr.wo(iox::SFR_PIOSEL, self.csr.r(iox::SFR_PIOSEL) | (1 << i));
                    mapping[i] = Some((port, pin));
                }
            } else {
                mapping[i] = None;
                // ensure that the PIO register bit is not set
                self.csr.wo(iox::SFR_PIOSEL, self.csr.r(iox::SFR_PIOSEL) & !(1 << i));
            }
        }
        mapping
    }

    /// Returns the PIO bit that was enabled based on the port and pin specifier given;
    /// returns `None` if the proposed mapping is invalid.
    pub fn set_pio_bit_from_port_and_pin(&self, port: IoxPort, pin: u8) -> Option<u8> {
        match port {
            IoxPort::PA => None,
            IoxPort::PB => {
                if pin >= 16 {
                    None
                } else {
                    self.set_alternate_function(port, pin, IoxFunction::AF1);
                    let pio_bit = 15 - pin;
                    self.csr.wo(iox::SFR_PIOSEL, self.csr.r(iox::SFR_PIOSEL) | (1 << pio_bit as u32));
                    Some(pio_bit)
                }
            }
            IoxPort::PC => {
                if (pin != 6 && pin != 11 && pin != 14 && pin != 15) && pin < 16 {
                    self.set_alternate_function(port, pin, IoxFunction::AF1);
                    let pio_bit = pin + 16;
                    self.csr.wo(iox::SFR_PIOSEL, self.csr.r(iox::SFR_PIOSEL) | (1 << pio_bit as u32));
                    Some(pio_bit)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Returns the PIO bit that was disabled based on the port and pin specifier given;
    /// returns `None` if the proposed mapping is invalid. Does not change the AF mapping,
    /// simply disables the bit in the PIO mux register.
    pub fn unset_pio_bit_from_port_and_pin(&self, port: IoxPort, pin: u8) -> Option<u8> {
        match port {
            IoxPort::PA => None,
            IoxPort::PB => {
                if pin >= 16 {
                    None
                } else {
                    let pio_bit = 15 - pin;
                    self.csr.wo(iox::SFR_PIOSEL, self.csr.r(iox::SFR_PIOSEL) & !(1 << pio_bit as u32));
                    Some(pio_bit)
                }
            }
            IoxPort::PC => {
                if (pin != 6 && pin != 11 && pin != 14 && pin != 15) && pin < 16 {
                    let pio_bit = pin + 16;
                    self.csr.wo(iox::SFR_PIOSEL, self.csr.r(iox::SFR_PIOSEL) & !(1 << pio_bit as u32));
                    Some(pio_bit)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn set_drive_strength(&self, port: IoxPort, pin: u8, strength: IoxDriveStrength) {
        assert!(pin < 16, "pin must be in range of 0-15");
        match port {
            IoxPort::PA => self.csr.wo(
                iox::SFR_CFG_DRVSEL_CR_CFG_DRVSEL0,
                self.csr.r(iox::SFR_CFG_DRVSEL_CR_CFG_DRVSEL0) & !(0b11 << (pin * 2))
                    | (strength as u32) << (pin * 2),
            ),
            IoxPort::PB => self.csr.wo(
                iox::SFR_CFG_DRVSEL_CR_CFG_DRVSEL1,
                self.csr.r(iox::SFR_CFG_DRVSEL_CR_CFG_DRVSEL1) & !(0b11 << (pin * 2))
                    | (strength as u32) << (pin * 2),
            ),
            IoxPort::PC => self.csr.wo(
                iox::SFR_CFG_DRVSEL_CR_CFG_DRVSEL2,
                self.csr.r(iox::SFR_CFG_DRVSEL_CR_CFG_DRVSEL2) & !(0b11 << (pin * 2))
                    | (strength as u32) << (pin * 2),
            ),
            IoxPort::PD => self.csr.wo(
                iox::SFR_CFG_DRVSEL_CR_CFG_DRVSEL3,
                self.csr.r(iox::SFR_CFG_DRVSEL_CR_CFG_DRVSEL3) & !(0b11 << (pin * 2))
                    | (strength as u32) << (pin * 2),
            ),
            IoxPort::PE => self.csr.wo(
                iox::SFR_CFG_DRVSEL_CR_CFG_DRVSEL4,
                self.csr.r(iox::SFR_CFG_DRVSEL_CR_CFG_DRVSEL4) & !(0b11 << (pin * 2))
                    | (strength as u32) << (pin * 2),
            ),
            IoxPort::PF => self.csr.wo(
                iox::SFR_CFG_DRVSEL_CR_CFG_DRVSEL5,
                self.csr.r(iox::SFR_CFG_DRVSEL_CR_CFG_DRVSEL5) & !(0b11 << (pin * 2))
                    | (strength as u32) << (pin * 2),
            ),
        }
    }
}

impl IoSetup for Iox {
    fn setup_pin(
        &self,
        port: IoxPort,
        pin: u8,
        direction: Option<IoxDir>,
        function: Option<IoxFunction>,
        schmitt_trigger: Option<IoxEnable>,
        pullup: Option<IoxEnable>,
        slow_slew: Option<IoxEnable>,
        strength: Option<IoxDriveStrength>,
    ) {
        if let Some(f) = function {
            self.set_alternate_function(port, pin, f);
        }
        if let Some(d) = direction {
            self.set_gpio_dir(port, pin, d);
        }
        if let Some(t) = schmitt_trigger {
            self.set_gpio_schmitt_trigger(port, pin, t);
        }
        if let Some(p) = pullup {
            self.set_gpio_pullup(port, pin, p);
        }
        if let Some(s) = slow_slew {
            self.set_slow_slew_rate(port, pin, s);
        }
        if let Some(s) = strength {
            self.set_drive_strength(port, pin, s);
        }
    }
}
