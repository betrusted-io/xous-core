use core::sync::atomic::Ordering;

use cramium_hal::iox::{
    IoGpio, IoSetup, IoxDir, IoxDriveStrength, IoxEnable, IoxFunction, IoxPort, IoxValue,
};
use num_traits::*;

use crate::{Opcode, SERVER_NAME_CRAM_HAL, api::IoxConfigMessage};

pub struct IoxHal {
    conn: xous::CID,
}

impl IoxHal {
    pub fn new() -> Self {
        crate::REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let xns = xous_names::XousNames::new().unwrap();
        let conn =
            xns.request_connection(SERVER_NAME_CRAM_HAL).expect("Couldn't connect to Cramium HAL server");
        IoxHal { conn }
    }

    pub fn set_gpio_pin_value(&self, port: IoxPort, pin: u8, value: IoxValue) {
        xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                Opcode::SetGpioBank.to_usize().unwrap(),
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
                Opcode::SetGpioBank.to_usize().unwrap(),
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
                Opcode::GetGpioBank.to_usize().unwrap(),
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
                Opcode::GetGpioBank.to_usize().unwrap(),
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
        buf.lend(self.conn, Opcode::ConfigureIox.to_u32().unwrap()).expect("Couldn't set up IO");
    }
}

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
        buf.lend(self.conn, Opcode::ConfigureIox.to_u32().unwrap()).expect("Couldn't set up IO");
    }

    fn set_gpio_pin_value(&self, port: IoxPort, pin: u8, value: IoxValue) {
        self.set_gpio_pin_value(port, pin, value);
    }
}

impl Drop for IoxHal {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the
        // connection.
        if crate::REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}
