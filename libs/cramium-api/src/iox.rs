#[cfg(feature = "std")]
use core::sync::atomic::{AtomicU32, Ordering};
#[cfg(feature = "std")]
static REFCOUNT: AtomicU32 = AtomicU32::new(0);

use num_traits::ToPrimitive;

#[cfg(feature = "std")]
use super::*;

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
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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

/// Traits for accessing GPIOs after the port has been set up.
pub trait IoGpio {
    fn set_gpio_pin_value(&self, port: IoxPort, pin: u8, value: IoxValue);
    fn get_gpio_pin_value(&self, port: IoxPort, pin: u8) -> IoxValue;
    fn set_gpio_pin_dir(&self, port: IoxPort, pin: u8, dir: IoxDir);
}

pub trait IoIrq {
    /// This hooks a given port/pin to generate a message to the server specified
    /// with `server` and the opcode number `usize` when an IRQ is detected on the port/pin.
    /// The active state of the IRQ is defined by `active`; the transition edge from inactive
    /// to active is when the event is generated.
    fn set_irq_pin(&self, port: IoxPort, pin: u8, active: IoxValue, server: &str, opcode: usize);
}

#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Debug)]
pub struct IoxConfigMessage {
    pub port: IoxPort,
    pub pin: u8,
    pub direction: Option<IoxDir>,
    pub function: Option<IoxFunction>,
    pub schmitt_trigger: Option<IoxEnable>,
    pub pullup: Option<IoxEnable>,
    pub slow_slew: Option<IoxEnable>,
    pub strength: Option<IoxDriveStrength>,
}

#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Debug)]
#[cfg(feature = "std")]
pub struct IoxIrqRegistration {
    pub server: String,
    pub opcode: usize,
    pub port: IoxPort,
    pub pin: u8,
    pub active: IoxValue,
}

#[cfg(feature = "std")]
pub struct IoxHal {
    conn: xous::CID,
}

#[cfg(feature = "std")]
impl IoxHal {
    pub fn new() -> Self {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let xns = xous_names::XousNames::new().unwrap();
        let conn =
            xns.request_connection(SERVER_NAME_CRAM_HAL).expect("Couldn't connect to Cramium HAL server");
        IoxHal { conn }
    }

    pub fn set_gpio_pin_value(&self, port: IoxPort, pin: u8, value: IoxValue) {
        xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                HalOpcode::SetGpioBank.to_usize().unwrap(),
                port as usize,
                // values to set
                (value as usize) << (pin as usize),
                // which pin, as a bitmask
                1 << pin as usize,
                0,
            ),
        )
        .expect("Couldn't set GPIO pin value");
    }

    pub fn set_gpio_bank_value(&self, port: IoxPort, value: u16, bitmask: u16) {
        xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                HalOpcode::SetGpioBank.to_usize().unwrap(),
                port as usize,
                // values to set
                value as usize,
                // mask of valid bits
                bitmask as usize,
                0,
            ),
        )
        .expect("Couldn't set GPIO pin value");
    }

    pub fn get_gpio_pin_value(&self, port: IoxPort, pin: u8) -> IoxValue {
        match xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                HalOpcode::GetGpioBank.to_usize().unwrap(),
                port as usize,
                0,
                0,
                0,
            ),
        ) {
            Ok(xous::Result::Scalar5(_, value, _, _, _)) => {
                if value & (1 << pin as usize) != 0 {
                    IoxValue::High
                } else {
                    IoxValue::Low
                }
            }
            _ => panic!("Internal Error: Couldn't get GPIO pin value"),
        }
    }

    pub fn get_gpio_bank_value(&self, port: IoxPort) -> u32 {
        match xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                HalOpcode::GetGpioBank.to_usize().unwrap(),
                port as usize,
                0,
                0,
                0,
            ),
        ) {
            Ok(xous::Result::Scalar5(_, value, _, _, _)) => value as u32,
            _ => panic!("Internal Error: Couldn't get GPIO pin value"),
        }
    }

    /// This function takes a 32-bit bitmask, corresponding to PIO 31 through 0, where
    /// a `1` indicates to map that PIO to a GPIO.
    ///
    /// This function will automatically remap the AF and PIO settings for the PIO pins
    /// specified in the bitmask, corresponding to the PIO GPIO pin number. If a `0` is
    /// present in a bit position, it will turn off the PIO mux, but not change the AF setting.
    ///
    /// VERY IMPORTANT: Note that the PIO GPIO number is *not* consistent with the
    /// numbering order of the GPIO ports: in fact, it is reverse-order for PORT B and in-order with skips for
    /// PORT C. Also, bits 22, 27, 30 and 31 are not mappable for the PIO.
    ///
    /// Returns: a 32-entry array which records which GPIO bank and pin number was affected
    /// by the mapping request. The index of the array corresponds to the bit position in
    /// the bitmask. You may use this to pass as arguments to further functions
    /// that do things like control slew rate or apply pull-ups.
    pub fn set_ports_from_pio_bitmask(&self, _enable_bitmask: u32) -> [Option<(IoxPort, u8)>; 32] {
        todo!("Do this when we get around to filling in the PIO drivers")
    }

    /// Returns the PIO bit that was enabled based on the port and pin specifier given;
    /// returns `None` if the proposed mapping is invalid.
    pub fn set_pio_bit_from_port_and_pin(&self, _port: IoxPort, _pin: u8) -> Option<u8> {
        todo!("Do this when we get around to filling in the PIO drivers")
    }

    /// Returns the PIO bit that was disabled based on the port and pin specifier given;
    /// returns `None` if the proposed mapping is invalid. Does not change the AF mapping,
    /// simply disables the bit in the PIO mux register.
    pub fn unset_pio_bit_from_port_and_pin(&self, _port: IoxPort, _pin: u8) -> Option<u8> {
        todo!("Do this when we get around to filling in the PIO drivers")
    }
}

#[cfg(feature = "std")]
impl IoSetup for IoxHal {
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
        let msg =
            IoxConfigMessage { port, pin, direction, function, schmitt_trigger, pullup, slow_slew, strength };
        let buf = xous_ipc::Buffer::into_buf(msg).unwrap();
        buf.lend(self.conn, HalOpcode::ConfigureIox as u32).expect("Couldn't set up IO");
    }
}

#[cfg(feature = "std")]
impl IoGpio for IoxHal {
    fn get_gpio_pin_value(&self, port: IoxPort, pin: u8) -> IoxValue { self.get_gpio_pin_value(port, pin) }

    fn set_gpio_pin_dir(&self, port: IoxPort, pin: u8, dir: IoxDir) {
        let msg = IoxConfigMessage {
            port,
            pin,
            direction: Some(dir),
            function: None,
            schmitt_trigger: None,
            pullup: None,
            slow_slew: None,
            strength: None,
        };
        let buf = xous_ipc::Buffer::into_buf(msg).unwrap();
        buf.lend(self.conn, HalOpcode::ConfigureIox as u32).expect("Couldn't set up IO");
    }

    fn set_gpio_pin_value(&self, port: IoxPort, pin: u8, value: IoxValue) {
        self.set_gpio_pin_value(port, pin, value);
    }
}

#[cfg(feature = "std")]
impl Drop for IoxHal {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the
        // connection.
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}

#[cfg(feature = "std")]
impl IoIrq for IoxHal {
    fn set_irq_pin(&self, port: IoxPort, pin: u8, active: IoxValue, server: &str, opcode: usize) {
        let msg = IoxIrqRegistration { server: server.to_owned(), opcode, port, pin, active };
        let buf = xous_ipc::Buffer::into_buf(msg).unwrap();
        buf.lend(self.conn, HalOpcode::ConfigureIoxIrq as u32).expect("Couldn't set up IRQ");
    }
}
