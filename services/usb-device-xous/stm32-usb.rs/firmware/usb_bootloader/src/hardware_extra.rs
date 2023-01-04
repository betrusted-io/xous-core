/// The stuff in here should probably reside inside the MCU support crate
/// Something similar exists for some HAL implementations but not the f1

use core::str::from_utf8_unchecked;

pub const ITM_BAUD_RATE: u32 = 8_000_000;
const SERIAL_NUMBER_LEN: usize = 24;

macro_rules! define_ptr_type {
    ($name: ident, $ptr: expr) => (
        impl $name {
            fn ptr() -> *const Self {
                $ptr as *const _
            }

            /// Returns a wrapped reference to the value in flash memory
            pub fn get() -> &'static Self {
                unsafe { &*Self::ptr() }
            }
        }
    )
}

/// Size of integrated flash
#[derive(Debug)]
#[repr(C)]
pub struct FlashSize(u16);
define_ptr_type!(FlashSize, 0x1FFF_F7E0);

impl FlashSize {
    /// Read flash size in kibi bytes
    pub fn kibi_bytes(&self) -> u16 {
        self.0
    }

    /// Read flash size in bytes
    pub fn bytes(&self) -> usize {
        usize::from(self.kibi_bytes()) * 1024
    }
}

#[derive(Hash, Debug)]
#[repr(C)]
pub struct Uid {
    a: u32,
    b: u32,
    c: u32,
}
define_ptr_type!(Uid, 0x1FFF_F7E8);

fn any_as_u8_slice<T: Sized>(p: &T) -> &[u8] {
    unsafe {
    core::slice::from_raw_parts(
            (p as *const T) as *const u8,
            core::mem::size_of::<T>(),
        )
    }
}

impl Uid {
    pub fn update_serial(&self, serial: &mut [u8]) {
        const CHARS: &str = "0123456789ABCDEF";
        let chars = CHARS.as_bytes();
        let bytes = any_as_u8_slice(self);
        
        for (i, b) in bytes.iter().enumerate() {
            let c1 = chars[((b >> 4) & 0xF_u8) as usize];
            let c2 = chars[((b >> 0) & 0xF_u8) as usize];

            let i = i * 2;
            if i < serial.len() {
                serial[i] = c1;
            }
            if i + 1 < serial.len() {
                serial[i+1] = c2;
            }
        }
    }
}



pub fn get_serial_number() -> &'static str {
    static mut SERIAL_NUMBER_BYTES: [u8; SERIAL_NUMBER_LEN] = [0; SERIAL_NUMBER_LEN];
    // Fetch the serial info from the device electronic signature registers 
    Uid::get().update_serial(unsafe { &mut SERIAL_NUMBER_BYTES });
    // And convert it to a utf &str
    let serial_number = unsafe { from_utf8_unchecked(&SERIAL_NUMBER_BYTES) };

    serial_number
}

pub fn get_flash_kibi() -> u16 {
    FlashSize::get().kibi_bytes()
}