#![cfg_attr(not(test), no_main)]
#![cfg_attr(not(test), no_std)]

extern crate alloc;
// contains runtime setup
mod asm;

mod platform;
mod repl;
use alloc::collections::VecDeque;
use core::cell::RefCell;

use critical_section::Mutex;
use platform::*;
#[allow(unused_imports)]
use utralib::*;

#[allow(unused_imports)]
use crate::delay;

static UART_RX: Mutex<RefCell<VecDeque<u8>>> = Mutex::new(RefCell::new(VecDeque::new()));
#[allow(dead_code)]
static USB_RX: Mutex<RefCell<VecDeque<u8>>> = Mutex::new(RefCell::new(VecDeque::new()));
static USB_TX: Mutex<RefCell<VecDeque<u8>>> = Mutex::new(RefCell::new(VecDeque::new()));

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
    crate::platform::early_init();
    crate::println!("\n~~boot0 up!~~\n");

    let mut repl = crate::repl::Repl::new();
    // do the main loop through the serial port
    loop {
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
}
