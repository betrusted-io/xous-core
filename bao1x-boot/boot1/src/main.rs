#![cfg_attr(not(test), no_main)]
#![cfg_attr(not(test), no_std)]

extern crate alloc;
// contains runtime setup
mod asm;
mod platform;
mod repl;
mod secboot;
mod uf2;

use alloc::collections::VecDeque;
use core::{cell::RefCell, sync::atomic::Ordering};

use bao1x_api::{BoardTypeCoding, BootWaitCoding};
use bao1x_hal::{board::KeyPress, iox::Iox, usb::driver::UsbDeviceState};
use critical_section::Mutex;
use platform::*;
#[allow(unused_imports)]
use utralib::*;

use crate::delay;
use crate::platform::usb::glue;
use crate::secboot::boot_or_die;

static UART_RX: Mutex<RefCell<VecDeque<u8>>> = Mutex::new(RefCell::new(VecDeque::new()));
#[allow(dead_code)]
static USB_RX: Mutex<RefCell<VecDeque<u8>>> = Mutex::new(RefCell::new(VecDeque::new()));
static USB_TX: Mutex<RefCell<VecDeque<u8>>> = Mutex::new(RefCell::new(VecDeque::new()));
static USB_CONNECTED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

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

    board_type = crate::platform::early_init(board_type);
    crate::println!("\n~~Boot1 up!~~\n");

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
        // this should diverge, rest of code is not run
        boot_or_die();
    }

    if boot_wait == BootWaitCoding::Enable {
        crate::println!("Boot bypassed because autoboot was disabled");
    } else if current_key.is_some() {
        crate::println!("Boot bypassed with keypress: {:?}", current_key);
    }

    let (se0_port, se0_pin) = match board_type {
        BoardTypeCoding::Baosec => bao1x_hal::board::setup_usb_pins(&iox),
        _ => crate::platform::setup_dabao_se0_pin(&iox),
    };
    iox.set_gpio_pin(se0_port, se0_pin, bao1x_api::IoxValue::Low); // put the USB port into SE0
    delay(500);
    // setup the USB port
    let (mut last_usb_state, mut portsc) = glue::setup();
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
            if new_usb_state == UsbDeviceState::Configured {
                crate::println!("USB is connected!");
                last_usb_state = new_usb_state;
                USB_CONNECTED.store(true, core::sync::atomic::Ordering::SeqCst);
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
            glue::flush_tx();
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
            if glue::is_disconnected(portsc) && new_usb_state == UsbDeviceState::Configured {
                USB_CONNECTED.store(false, core::sync::atomic::Ordering::SeqCst);
                // last_usb_state = UsbDeviceState::NotAttached;
                break;
            }
        }
    }

    // when we get to this point, there's only two options...
    crate::secboot::boot_or_die();
}
