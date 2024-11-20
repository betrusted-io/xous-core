use core::fmt::Display;

use cramium_hal::iox::{IoGpio, IoSetup, Iox, IoxPort, IoxValue};
use cramium_hal::minigfx::{FrameBuffer, Point};
use cramium_hal::sh1107::Mono;
use cramium_hal::udma;
use simple_fatfs::io::{IOBase, Read, Seek, SeekFrom, Write};
use simple_fatfs::{IOError, IOErrorKind, PathBuf};
use utralib::generated::*;

use crate::platform::cramium::gfx;
use crate::platform::cramium::usb;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum KeyPress {
    Up,
    Down,
    Left,
    Right,
    Select,
    Start,
    A,
    B,
    Invalid,
    None,
}

pub fn scan_keyboard<T: IoSetup + IoGpio>(
    iox: &T,
    rows: &[(IoxPort, u8)],
    cols: &[(IoxPort, u8)],
) -> [KeyPress; 4] {
    let mut key_presses: [KeyPress; 4] = [KeyPress::None; 4];
    let mut key_press_index = 0; // no Vec in no_std, so we have to manually track it

    for (row, (port, pin)) in rows.iter().enumerate() {
        iox.set_gpio_pin_value(*port, *pin, IoxValue::Low);
        for (col, (col_port, col_pin)) in cols.iter().enumerate() {
            if iox.get_gpio_pin_value(*col_port, *col_pin) == IoxValue::Low {
                if key_press_index < key_presses.len() {
                    key_presses[key_press_index] = match (row, col) {
                        (0, 2) => KeyPress::Select,
                        (2, 1) => KeyPress::Start,
                        (1, 2) => KeyPress::Left,
                        (1, 1) => KeyPress::Up,
                        (0, 1) => KeyPress::Down,
                        (2, 0) => KeyPress::Right,
                        (0, 0) => KeyPress::A,
                        (1, 0) => KeyPress::B,
                        _ => KeyPress::Invalid,
                    };
                    key_press_index += 1;
                }
            }
        }
        iox.set_gpio_pin_value(*port, *pin, IoxValue::High);
    }
    key_presses
}

/// Checks to see if the necessary conditions for an update are met
pub fn process_update(perclk: u32) {
    crate::println!("entering process_update");
    // Placeholder:
    // Remember to lock the root keys before processing any updates
    crate::platform::cramium::verifier::lifecycle_lock_root();

    crate::println!("waiting for button press");
    let mut iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
    let mut udma_global = udma::GlobalConfig::new(utra::udma_ctrl::HW_UDMA_CTRL_BASE as *mut u32);

    let mut sh1107 = cramium_hal::sh1107::Oled128x128::new(perclk, &mut iox, &mut udma_global);

    gfx::msg(&mut sh1107, "    START to boot", Point::new(0, 16), Mono::White.into(), Mono::Black.into());
    gfx::msg(&mut sh1107, "   SELECT to update", Point::new(0, 0), Mono::White.into(), Mono::Black.into());

    sh1107.buffer_swap();
    sh1107.draw();

    // setup IO pins to check for update viability
    let (rows, cols) = cramium_hal::board::baosec::setup_kb_pins(&iox);

    let mut key_pressed = false;
    let mut do_update = false;
    while !key_pressed {
        let kps = scan_keyboard(&iox, &rows, &cols);
        for kp in kps {
            if kp != KeyPress::None {
                crate::println!("Got key: {:?}", kp);
                key_pressed = true;
            }
            if kp == KeyPress::Select {
                do_update = true;
            }
        }
    }
    if do_update {
        crate::platform::cramium::usb::init_usb();
        // it's all unsafe because USB is global mutable state
        unsafe {
            if let Some(ref mut usb_ref) = crate::platform::cramium::usb::USB {
                let usb = &mut *core::ptr::addr_of_mut!(*usb_ref);
                usb.reset();
                let mut poweron = 0;
                loop {
                    usb.udc_handle_interrupt();
                    if usb.pp() {
                        poweron += 1; // .pp() is a sham. MPW has no way to tell if power is applied. This needs to be fixed for NTO.
                    }
                    crate::platform::delay(100);
                    if poweron >= 4 {
                        break;
                    }
                }
                usb.reset();
                usb.init();
                usb.start();
                usb.update_current_speed();

                crate::println!("hw started...");

                loop {
                    let kps = scan_keyboard(&iox, &rows, &cols);
                    // only consider the first key returned in case of multi-key hit, for simplicity
                    if kps[0] == KeyPress::Select {
                        break;
                    } else if kps[0] != KeyPress::None {
                        crate::println!("Got key {:?}; ignoring", kps[0]);
                    }
                }

                let disk = usb::conjure_disk();
                let mut cursor = SliceCursor::new(disk);

                // We can either pass by value of by (mutable) reference
                let mut fs = simple_fatfs::FileSystem::from_storage(&mut cursor).unwrap();
                match fs.read_dir(PathBuf::from("/")) {
                    Ok(dir) => {
                        for entry in dir {
                            crate::println!("file: {:?}", entry);
                        }
                    }
                    Err(e) => {
                        crate::println!("Couldn't list dir: {:?}", e);
                    }
                }
            } else {
                crate::println!("USB core not allocated, can't do update!");
            }
        }
    }
}

pub struct SliceCursor<'a> {
    slice: &'a mut [u8],
    pos: u64,
}

impl<'a> SliceCursor<'a> {
    pub fn new(slice: &'a mut [u8]) -> Self { Self { slice, pos: 0 } }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum IOErrorFatfs {
    UnexpectedEof,
    Interrupted,
    InvalidData,
    Description,
    SeekUnderflow,
    SeekOverflow,
    SeekOutOfBounds,
}

impl Display for IOErrorFatfs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            IOErrorFatfs::UnexpectedEof => write!(f, "Unexpected EOF"),
            IOErrorFatfs::Interrupted => write!(f, "Interrupted"),
            IOErrorFatfs::InvalidData => write!(f, "Invalid Data"),
            IOErrorFatfs::Description => write!(f, "Unsupported string description"),
            IOErrorFatfs::SeekOutOfBounds => write!(f, "Seek out of bounds"),
            IOErrorFatfs::SeekOverflow => write!(f, "Seek overflow"),
            IOErrorFatfs::SeekUnderflow => write!(f, "Seek underflow"),
        }
    }
}

impl From<&str> for IOErrorFatfs {
    fn from(_value: &str) -> Self { IOErrorFatfs::Description }
}
impl IOErrorKind for IOErrorFatfs {
    fn new_unexpected_eof() -> Self { Self::UnexpectedEof }

    fn new_invalid_data() -> Self { Self::InvalidData }

    fn new_interrupted() -> Self { Self::Interrupted }
}
impl IOError for IOErrorFatfs {
    type Kind = IOErrorFatfs;

    fn new<M>(kind: Self::Kind, _msg: M) -> Self
    where
        M: core::fmt::Display,
    {
        kind
    }

    fn kind(&self) -> Self::Kind { *self }
}

impl simple_fatfs::Error for IOErrorFatfs {}

impl IOBase for SliceCursor<'_> {
    type Error = IOErrorFatfs;
}

impl Seek for SliceCursor<'_> {
    fn seek(&mut self, seek_from: simple_fatfs::io::SeekFrom) -> Result<u64, Self::Error> {
        let new_pos = match seek_from {
            SeekFrom::Start(offset) => offset,
            SeekFrom::End(offset) => {
                let result = if offset < 0 {
                    self.slice.len().checked_sub((-offset) as usize).ok_or(IOErrorFatfs::SeekUnderflow)?
                } else {
                    self.slice.len().checked_add(offset as usize).ok_or(IOErrorFatfs::SeekOverflow)?
                };
                result as u64
            }
            SeekFrom::Current(offset) => {
                if offset < 0 {
                    self.pos.checked_sub((-offset) as u64).ok_or(IOErrorFatfs::SeekUnderflow)?
                } else {
                    self.pos.checked_add(offset as u64).ok_or(IOErrorFatfs::SeekUnderflow)?
                }
            }
        };

        if new_pos > self.slice.len() as u64 {
            Err(IOErrorFatfs::SeekOutOfBounds)
        } else {
            self.pos = new_pos;
            Ok(self.pos)
        }
    }
}

impl Read for SliceCursor<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let available = &self.slice[self.pos as usize..];
        let to_read = buf.len().min(available.len());
        buf[..to_read].copy_from_slice(&available[..to_read]);
        self.pos += to_read as u64;
        Ok(to_read)
    }
}
impl Write for SliceCursor<'_> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        let available = &mut self.slice[self.pos as usize..];
        let to_write = buf.len().min(available.len());
        available[..to_write].copy_from_slice(&buf[..to_write]);
        self.pos += to_write as u64;
        Ok(to_write)
    }

    fn flush(&mut self) -> Result<(), Self::Error> { Ok(()) }
}
