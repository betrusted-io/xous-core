#![cfg_attr(not(test), no_main)]
#![cfg_attr(not(test), no_std)]

extern crate alloc;
// contains runtime setup
mod asm;

mod erase;
mod platform;
mod repl;
mod serial;

use alloc::collections::VecDeque;
use core::cell::RefCell;
#[cfg(feature = "bao1x-usb")]
use core::sync::atomic::Ordering;

#[cfg(feature = "bao1x-usb")]
use bao1x_hal::{iox::Iox, usb::driver::UsbDeviceState};
use critical_section::Mutex;
use platform::*;
#[allow(unused_imports)]
use utralib::*;

#[cfg(feature = "bao1x-usb")]
use crate::usb::glue;

use crate::serial::SerialInteract;

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
    crate::platform::early_init();
    #[cfg(feature = "repl")]
    crate::println!("\n~~Baremetal up!~~\n");

    #[cfg(feature = "repl")]
    let mut handler = crate::repl::Repl::new();

    #[cfg(not(feature = "repl"))]
    let mut handler = crate::erase::OneShotErasure::new();

    #[cfg(feature = "bao1x-usb")]
    let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
    #[cfg(feature = "bao1x-usb")]
    let (mut last_usb_state, mut portsc) = crate::platform::usb::glue::hotplug_usb(&iox);

    #[cfg(feature = "bao1x-usb")]
    loop {
        let (new_usb_state, new_portsc) = glue::usb_status();

        // check if the USB status has changed
        if new_usb_state != last_usb_state {
            if new_usb_state == UsbDeviceState::Configured {
                last_usb_state = new_usb_state;
                USB_CONNECTED.store(true, core::sync::atomic::Ordering::SeqCst);
            }
        }

        let use_usb = USB_CONNECTED.load(Ordering::SeqCst);

        // fetch characters from the rx buffer
        critical_section::with(|cs| {
            let mut queue =
                if use_usb { USB_RX.borrow(cs).borrow_mut() } else { UART_RX.borrow(cs).borrow_mut() };
            while let Some(byte) = queue.pop_front() {
                handler.rx_char(byte);
            }
        });

        // process received data
        handler.process();

        if use_usb {
            glue::flush_tx();
        }

        // return control to hard-wired serial port when USB is disconnected
        if new_portsc != portsc {
            portsc = new_portsc;
            if glue::is_disconnected(portsc) && new_usb_state == UsbDeviceState::Configured {
                USB_CONNECTED.store(false, core::sync::atomic::Ordering::SeqCst);
            }
        }
    }

    #[cfg(not(feature = "bao1x-usb"))]
    // do the main loop through only the serial port
    loop {
        // fetch characters from the rx buffer
        critical_section::with(|cs| {
            let mut queue = UART_RX.borrow(cs).borrow_mut();
            while let Some(byte) = queue.pop_front() {
                handler.rx_char(byte);
            }
        });

        // process received data
        handler.process();
    }
}
