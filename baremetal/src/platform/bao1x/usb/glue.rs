use bao1x_api::{IoGpio, IoSetup};
use bao1x_hal::usb::driver::UsbDeviceState;

use crate::{glue, usb};

// Empirically measured PORTSC when the port is unplugged. This might be a brittle way
// to detect if the device is unplugged.
const DISCONNECT_STATE: u32 = 0x40b; //  01_0_0000_0_1_01_1
const DISCONNECT_STATE_HS: u32 = 0xc6b; // 11_0_0011_0_1_01_1

pub fn is_disconnected(state: u32) -> bool { state == DISCONNECT_STATE_HS || state == DISCONNECT_STATE }

pub fn setup() -> (UsbDeviceState, u32) {
    crate::println_d!(
        "RAM disk starts at {:x} and is {}kiB in length",
        usb::RAMDISK_ADDRESS,
        usb::RAMDISK_LEN / 1024
    );

    // safety: this is safe because we're calling this before any access to `USB` static mut
    // state, and we also understand that the .data section doesn't exist in the loader and
    // we've taken countermeasures to initialize everything "from code", i.e. not relying
    // on static compile-time assignments for the static mut state.
    unsafe { crate::platform::bao1x::usb::init_usb() };

    // Below is all unsafe because USB is global mutable state
    unsafe {
        if let Some(ref mut usb_ref) = crate::platform::bao1x::usb::USB {
            let usb = &mut *core::ptr::addr_of_mut!(*usb_ref);
            usb.reset();
            usb.init();
            usb.start();
            usb.update_current_speed();
            // IRQ enable must happen without dependency on the hardware lock
            usb.irq_csr.wo(utralib::utra::irqarray1::EV_PENDING, 0xffff_ffff); // blanket clear
            usb.irq_csr.wfo(utralib::utra::irqarray1::EV_ENABLE_USBC_DUPE, 1);

            let last_usb_state = usb.get_device_state();
            let portsc = usb.portsc_val();
            crate::println_d!("USB state: {:?}, {:x}", last_usb_state, portsc);
            (last_usb_state, portsc)
        } else {
            panic!("USB core not allocated, can't proceed!");
        }
    }
}

pub fn usb_status() -> (UsbDeviceState, u32) {
    unsafe {
        if let Some(ref mut usb_ref) = crate::platform::bao1x::usb::USB {
            let usb = &mut *core::ptr::addr_of_mut!(*usb_ref);
            (usb.get_device_state(), usb.portsc_val())
        } else {
            panic!("USB core not allocated, can't proceed!");
        }
    }
}

pub fn flush_tx() {
    unsafe {
        if let Some(ref mut usb_ref) = crate::platform::bao1x::usb::USB {
            let usb = &mut *core::ptr::addr_of_mut!(*usb_ref);
            crate::usb::handlers::flush_tx(usb);
        } else {
            panic!("USB core not allocated, can't proceed!");
        }
    }
}

pub fn hotplug_usb<T: IoSetup + IoGpio>(iox: &T) -> (UsbDeviceState, u32) {
    let (se0_port, se0_pin) = bao1x_hal::board::setup_usb_pins(iox);
    iox.set_gpio_pin_value(se0_port, se0_pin, bao1x_api::IoxValue::Low); // put the USB port into SE0
    crate::delay(500);
    // setup the USB port
    let (last_usb_state, portsc) = glue::setup();
    crate::delay(500);
    // release SE0
    iox.set_gpio_pin_value(se0_port, se0_pin, bao1x_api::IoxValue::High);
    // USB should have a solid shot of connecting now.
    crate::println!("USB device ready");
    (last_usb_state, portsc)
}
