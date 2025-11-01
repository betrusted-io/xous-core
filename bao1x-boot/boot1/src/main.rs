#![cfg_attr(not(test), no_main)]
#![cfg_attr(not(test), no_std)]

extern crate alloc;
// contains runtime setup
mod asm;
mod platform;
mod repl;
mod secboot;
mod uf2;
mod version;

use alloc::collections::VecDeque;
use core::{
    cell::RefCell,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

use bao1x_api::{BoardTypeCoding, BootWaitCoding};
use bao1x_hal::{board::KeyPress, iox::Iox, usb::driver::UsbDeviceState};
use bao1x_hal::{sh1107::Oled128x128, udma::GlobalConfig};
use critical_section::Mutex;
use platform::*;
#[allow(unused_imports)]
use utralib::*;
use ux_api::minigfx::{DrawStyle, FrameBuffer, Point, Rectangle};

use crate::delay;
use crate::platform::usb::glue;
use crate::secboot::try_boot;

// Notes:
// - "Towards" - not a release yet, but working towards the stated milestone
// - Eliminating "Towards" is done at the tag-out point.
const RELEASE_DESCRIPTION: &'static str = "Towards Alpha-1";

static UART_RX: Mutex<RefCell<VecDeque<u8>>> = Mutex::new(RefCell::new(VecDeque::new()));
#[allow(dead_code)]
static USB_RX: Mutex<RefCell<VecDeque<u8>>> = Mutex::new(RefCell::new(VecDeque::new()));
static USB_TX: Mutex<RefCell<VecDeque<u8>>> = Mutex::new(RefCell::new(VecDeque::new()));
static USB_CONNECTED: AtomicBool = AtomicBool::new(false);
static DISK_BUSY: AtomicBool = AtomicBool::new(false);

// telemetry for updates. Has to be accessible in an interrupt context, hence the Atomics
static BAREMETAL_BYTES: AtomicU32 = AtomicU32::new(0);
static KERNEL_BYTES: AtomicU32 = AtomicU32::new(0);
static SWAP_BYTES: AtomicU32 = AtomicU32::new(0);
static APP_BYTES: AtomicU32 = AtomicU32::new(0);
static IS_BAOSEC: AtomicBool = AtomicBool::new(false);

pub fn uart_irq_handler() {
    use crate::debug::SerialRead;
    let mut uart = crate::debug::Uart {};

    loop {
        match uart.getc() {
            Some(c) => {
                critical_section::with(|cs| {
                    UART_RX.borrow(cs).borrow_mut().push_back(c);
                });
            }
            _ => break,
        }
    }
}

/// Entrypoint
///
/// # Safety
///
/// This function is safe to call exactly once.
#[export_name = "rust_entry"]
pub unsafe extern "C" fn rust_entry() -> ! {
    let one_way = bao1x_hal::acram::OneWayCounter::new();
    let mut board_type =
        one_way.get_decoded::<bao1x_api::BoardTypeCoding>().expect("Board type coding error");

    crate::println_d!("TX_IDLE: {:?}", crate::platform::usb::TX_IDLE.load(Ordering::SeqCst));
    let perclk: u32;
    (board_type, perclk) = crate::platform::early_init(board_type);
    crate::println!("\n~~Boot1 up! ({}: {})~~\n", crate::version::SEMVER, RELEASE_DESCRIPTION);
    crate::println!("Configured board type: {:?}", board_type);
    if board_type == BoardTypeCoding::Baosec {
        IS_BAOSEC.store(true, Ordering::SeqCst);
        #[cfg(all(feature = "force-dabao", feature = "alt-boot1"))]
        {
            while one_way.get_decoded::<bao1x_api::BoardTypeCoding>().expect("owc coding error")
                != bao1x_api::BoardTypeCoding::Dabao
            {
                one_way.inc_coded::<bao1x_api::BoardTypeCoding>().expect("increment error");
            }
            board_type = one_way.get_decoded::<bao1x_api::BoardTypeCoding>().expect("owc coding error");
            crate::println!("Re-configured board type: {:?}", board_type);
        }
    }

    let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);

    let mut current_key = if let Some(key) = crate::platform::get_key(&board_type, &iox) {
        // TODO: on baosec v2, we should not get Invalid keys. However, as we wait for the new
        // boards to come in this will be a thing.
        if key != KeyPress::Invalid {
            // skip boot if a key is pressed; record what key it is so we know to check that it has
            // become *unpressed* before looking for a new press
            Some(key)
        } else {
            None
        }
    } else {
        None
    };

    let one_way = bao1x_hal::acram::OneWayCounter::new();
    let boot_wait = one_way.get_decoded::<BootWaitCoding>().expect("internal error");

    if boot_wait == BootWaitCoding::Disable && current_key.is_none() {
        // diverges if there is code to run
        try_boot(false);
        // or_die == false means the rest of this gets run if there is no valid image
    }

    if boot_wait == BootWaitCoding::Enable {
        crate::println!("Boot bypassed because bootwait was enabled");
    } else if current_key.is_some() {
        crate::println!("Boot bypassed with keypress: {:?}", current_key);
    }

    // grab a handle to the OLED if it exists on the board.
    let mut udma_global = GlobalConfig::new();
    let mut oled_iox = iox.clone();
    let mut oled = if board_type == BoardTypeCoding::Baosec {
        Some(bao1x_hal::sh1107::Oled128x128::new(
            bao1x_hal::sh1107::MainThreadToken::new(),
            perclk,
            &mut oled_iox,
            &mut udma_global,
        ))
    } else {
        None
    };

    let (se0_port, se0_pin) = match board_type {
        BoardTypeCoding::Baosec => bao1x_hal::board::setup_usb_pins(&iox),
        _ => crate::platform::setup_dabao_se0_pin(&iox),
    };
    iox.set_gpio_pin(se0_port, se0_pin, bao1x_api::IoxValue::Low); // put the USB port into SE0
    delay(100);
    // use the USB disconnect time to initialize the display - at least 100ms is
    // needed after reset for the display to initialize
    if let Some(ref mut sh1107) = oled {
        // show the boot logo
        sh1107.init();
        sh1107.buffer_mut().fill(0xFFFF_FFFF);
        sh1107.blit_screen(&ux_api::bitmaps::baochip128x128::BITMAP);
        sh1107.draw();
    }
    delay(400);

    // setup the USB port
    let (mut last_usb_state, mut portsc) = glue::setup();

    // remainder of the 1-second total wait target time for USB disconnect
    delay(500);
    // release SE0
    iox.set_gpio_pin(se0_port, se0_pin, bao1x_api::IoxValue::High);
    // return the pin to an input
    match board_type {
        BoardTypeCoding::Dabao | BoardTypeCoding::Oem => {
            crate::platform::setup_dabao_boot_pin(&iox);
        }
        _ => {
            // no need to switch back
        }
    }
    // USB should have a solid shot of connecting now.
    crate::println!("USB device ready");
    if let Some(ref mut sh1107) = oled {
        marquee(sh1107, "Update mode");
    }

    // if Baosec, initialize the QPI SPI flash interface (for receiving updates)
    if board_type == BoardTypeCoding::Baosec {
        crate::glue::setup_spim(perclk);
    }

    // provide some feedback on the run state of the BIO by peeking at the program counter
    // value, and provide feedback on the CPU operation by flashing the RGB LEDs.
    let mut repl = crate::repl::Repl::new();
    let mut new_key: Option<KeyPress>;
    loop {
        let (new_usb_state, new_portsc) = glue::usb_status();

        // update key state
        new_key = crate::platform::get_key(&board_type, &iox);
        if current_key.is_some() && new_key.is_none() {
            delay(10);
            // debounce the release
            new_key = crate::platform::get_key(&board_type, &iox);
        }
        // break if a key is pressed, but only after we have detected the original key being released
        if new_key.is_some() && current_key.is_none() {
            break;
        }
        current_key = new_key;

        // provide feedback when connection is established
        if new_usb_state != last_usb_state {
            crate::println_d!("new state {:?}", new_usb_state);
            if new_usb_state == UsbDeviceState::Configured {
                crate::println!("USB is connected!");
                last_usb_state = new_usb_state;
                USB_CONNECTED.store(true, core::sync::atomic::Ordering::SeqCst);
                if let Some(ref mut sh1107) = oled {
                    marquee(sh1107, "Connected");
                }
            }
        }

        // repl handling; USB is entirely interrupt driven, so there is no loop to handle it
        if USB_CONNECTED.load(Ordering::SeqCst) {
            // fetch characters from the Rx buffer
            critical_section::with(|cs| {
                let mut queue = USB_RX.borrow(cs).borrow_mut();
                while let Some(byte) = queue.pop_front() {
                    repl.rx_char(byte);
                }
            });

            // Process any command line requests
            match repl.process() {
                Err(e) => {
                    if let Some(m) = e.message {
                        crate::println!("{}", m);
                        repl.abort_cmd();
                    }
                }
                _ => (),
            };
            if !DISK_BUSY.load(Ordering::SeqCst) {
                glue::flush_tx();
            }
        } else {
            // Handle keyboard events.
            critical_section::with(|cs| {
                let mut queue = UART_RX.borrow(cs).borrow_mut();
                while let Some(byte) = queue.pop_front() {
                    repl.rx_char(byte);
                }
            });

            // Process any command line requests
            match repl.process() {
                Err(e) => {
                    if let Some(m) = e.message {
                        crate::println!("{}", m);
                        repl.abort_cmd();
                    }
                }
                _ => (),
            };
        }

        // break out of the loop when USB is disconnected
        if new_portsc != portsc {
            portsc = new_portsc;
            crate::println_d!("new portsc {:x}", portsc);
            if glue::is_disconnected(portsc) && new_usb_state == UsbDeviceState::Configured {
                crate::println_d!("USB disconnected!");
                USB_CONNECTED.store(false, core::sync::atomic::Ordering::SeqCst);
                // last_usb_state = UsbDeviceState::NotAttached;
                break;
            }
        }
    }

    boot(&iox, oled, se0_port, se0_pin)
}

pub fn boot(iox: &Iox, mut oled: Option<Oled128x128>, se0_port: bao1x_api::IoxPort, se0_pin: u8) -> ! {
    if let Some(ref mut sh1107) = oled {
        marquee(sh1107, "Booting...");
    }

    // stop the USB subsystem so it can be re-init'd by the next stage.
    // without this, USB init will hang later on.
    glue::shutdown();
    iox.set_gpio_dir(se0_port, se0_pin, bao1x_api::IoxDir::Output);
    iox.set_gpio_pin(se0_port, se0_pin, bao1x_api::IoxValue::Low); // put the USB port into SE0, so we re-enumerate with the OS stack

    // check that all pages in the SPI memory page cache have been written out
    critical_section::with(|cs| {
        if let Some(assembler) = &mut *crate::glue::SECTOR_TRACKER.borrow(cs).borrow_mut() {
            if assembler.active_pages() > 0 {
                loop {
                    if let Some((addr, data)) = assembler.take_next_incomplete() {
                        // the "holes" will just have 0 in them, which is fine for these purposes
                        // the primary case that triggers this is when the last sector written doesn't fill
                        // up a whole page.
                        crate::print_d!("Flushing final swap page at {:x}", addr);
                        crate::glue::write_spim_page(addr, data);
                    } else {
                        break;
                    }
                }
            }
        }
    });

    // when we get to this point, there's only two options...
    try_boot(true);
    unreachable!("`or_die = true` means this should be unreachable");
}

pub fn marquee(sh1107: &mut Oled128x128, msg: &str) {
    use bao1x_hal::sh1107::{COLUMN, ROW};
    use ux_api::bitmaps::baochip128x128::MARQUEE_BELOW;

    // blank out the marquee
    ux_api::minigfx::op::rectangle(
        sh1107,
        Rectangle::new_with_style(
            Point::new(0, MARQUEE_BELOW as isize),
            Point::new(COLUMN, ROW),
            DrawStyle::new(ux_api::minigfx::PixelColor::Dark, ux_api::minigfx::PixelColor::Dark, 1),
        ),
        None,
        false,
    );

    // now try best-effort to fit the message. No word-wrapping here.
    let msg_width = msg.len() as isize * crate::gfx::CHAR_WIDTH;
    let x_pos = (COLUMN - msg_width) / 2;
    let y_midline = MARQUEE_BELOW as isize + (ROW - MARQUEE_BELOW as isize) / 2;
    let y_pos = y_midline - crate::gfx::CHAR_HEIGHT / 2;
    gfx::msg(
        sh1107,
        msg,
        Point::new(x_pos, y_pos),
        bao1x_hal::sh1107::Mono::White.into(),
        bao1x_hal::sh1107::Mono::Black.into(),
    );
    sh1107.draw();
}
