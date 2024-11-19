use cramium_hal::iox::{IoGpio, IoSetup, Iox, IoxPort, IoxValue};
use cramium_hal::minigfx::{FrameBuffer, Point};
use cramium_hal::sh1107::Mono;
use cramium_hal::udma;
use utralib::generated::*;

use crate::platform::cramium::gfx;

pub fn scan_keyboard<T: IoSetup + IoGpio>(
    iox: &T,
    rows: &[(IoxPort, u8)],
    cols: &[(IoxPort, u8)],
) -> [Option<(u8, u8)>; 4] {
    let mut key_presses: [Option<(u8, u8)>; 4] = [None; 4];
    let mut key_press_index = 0; // no Vec in no_std, so we have to manually track it

    for (row, (port, pin)) in rows.iter().enumerate() {
        iox.set_gpio_pin_value(*port, *pin, IoxValue::Low);
        for (col, (col_port, col_pin)) in cols.iter().enumerate() {
            if iox.get_gpio_pin_value(*col_port, *col_pin) == IoxValue::Low {
                if key_press_index < key_presses.len() {
                    key_presses[key_press_index] = Some((row as u8, col as u8));
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
            match kp {
                // SELECT
                Some((0, 2)) => {
                    crate::println!("SELECT detected");
                    do_update = true;
                    key_pressed = true;
                }
                // START
                Some((2, 1)) => {
                    crate::println!("START detected");
                    key_pressed = true;
                }
                Some((r, c)) => {
                    crate::println!("{},{} pressed", r, c);
                    // this causes the system to boot if *any* key is pressed.
                    key_pressed = true;
                }
                None => (),
            }
        }
    }
    if do_update {
        update();
    }
}

fn update() {
    crate::platform::cramium::usb::init_usb();
    unsafe {
        crate::platform::cramium::usb::test_usb();
    }
}
