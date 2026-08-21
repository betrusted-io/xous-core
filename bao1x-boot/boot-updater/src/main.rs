#![cfg_attr(not(test), no_main)]
#![cfg_attr(not(test), no_std)]

extern crate alloc;
// contains runtime setup
mod asm;

mod platform;
mod repl;
mod rram;
mod uf2;
mod update;

use alloc::collections::VecDeque;
use alloc::format;
use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use bao1x_api::*;
use bao1x_hal::{
    board::KeyPress,
    hardening::*,
    iox::Iox,
    sh1107::Oled128x128,
    udma::GlobalConfig,
    usb::driver::{PortSpeed, UsbDeviceState},
};
use critical_section::Mutex;
use platform::*;
#[allow(unused_imports)]
use utralib::*;
use ux_api::minigfx::{DrawStyle, FrameBuffer, Point, Rectangle};

#[allow(unused_imports)]
use crate::delay;
use crate::usb::glue;

static WRITTEN_BYTES: AtomicU32 = AtomicU32::new(0);
static IS_BAOSEC: AtomicBool = AtomicBool::new(false);
static DISK_BUSY: AtomicBool = AtomicBool::new(false);

static UART_RX: Mutex<RefCell<VecDeque<u8>>> = Mutex::new(RefCell::new(VecDeque::new()));

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
#[unsafe(export_name = "rust_entry")]
pub unsafe extern "C" fn rust_entry() -> ! {
    let mut csprng = Csprng::new();
    csprng.random_delay();

    let one_way = bao1x_hal::acram::OneWayCounter::new();
    let board_type = one_way.get_decoded::<bao1x_api::BoardTypeCoding>().expect("Board type coding error");

    let perclk = crate::platform::early_init();
    crate::println!("\n~~Bootloader updater up!~~\n");
    csprng.random_delay();

    if board_type == BoardTypeCoding::Baosec {
        IS_BAOSEC.store(true, Ordering::SeqCst);
    }
    #[cfg(feature = "oem-baosec-lite")]
    {
        IS_BAOSEC.store(true, Ordering::SeqCst);
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

    // force bootwait - otherwise, we end up in a boot-loop because we'd go through a successful
    // boot1 update and then fall through into the updater again!
    while one_way.get_decoded::<bao1x_api::BootWaitCoding>().expect("couldn't fetch flag")
        != bao1x_api::BootWaitCoding::Enable
    {
        one_way.inc_coded::<bao1x_api::BootWaitCoding>().unwrap();
    }

    // grab a handle to the OLED if it exists on the board.
    let mut udma_global = GlobalConfig::new();
    let mut oled_iox = iox.clone();
    #[cfg(feature = "oem-baosec-lite")]
    let mut oled = Some(bao1x_hal::sh1107::Oled128x128::new(
        bao1x_hal::sh1107::MainThreadToken::new(),
        perclk,
        &mut oled_iox,
        &mut udma_global,
    ));
    #[cfg(not(feature = "oem-baosec-lite"))]
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

    // put both options for USB port switch into SE0 - at this point we may not know what board
    // type we have. This works for both dabao and for baosec.
    let se0_baosec = bao1x_hal::board::setup_usb_pins(&iox);
    let se0_dabao = crate::platform::setup_dabao_se0_pin(&iox);
    iox.set_gpio_pin(se0_baosec.0, se0_baosec.1, bao1x_api::IoxValue::Low);
    iox.set_gpio_pin(se0_dabao.0, se0_dabao.1, bao1x_api::IoxValue::Low);
    // use the USB disconnect time to initialize the display - at least 100ms is
    // needed after reset for the display to initialize
    if let Some(ref mut sh1107) = oled {
        // show the boot logo
        sh1107.init().ok();
        delay(100);
        sh1107.blit_screen(&ux_api::bitmaps::baochip128x128::BITMAP);
        sh1107.draw().ok();
        delay(150);
    } else {
        delay(250);
    }

    // run the update while SE0 is asserted - this ensures no simultaneous write to boot1
    let update_good = if one_way.get_decoded::<bao1x_api::Boot1DeveloperState>().unwrap()
        == bao1x_api::Boot1DeveloperState::Good
    {
        // this forces the staged setting to be applied. Useful primarily if you're doing risky
        // boot1 development and you want to force a staging test cycle.
        #[cfg(feature = "force-stage")]
        one_way.inc_coded::<bao1x_api::Boot1DeveloperState>().unwrap();

        // only run if the boot1 state was previously Good. If we had some boot1 failure, fall
        // back to mass storage and hope the developer is smart enough to restore boot1 from a
        // known good state.
        crate::update::run(&mut csprng)
    } else {
        crate::println!(
            "Boot1 update previously failed: {:?}",
            one_way.get_decoded::<bao1x_api::Boot1DeveloperState>().unwrap()
        );
        crate::println!(
            "Skipping auto-update using internal payload. Immediately restore boot1 from a known good image or risk bricking your board!"
        );
        false
    };

    delay(250);
    // setup the USB port
    let (mut last_usb_state, mut portsc) = match one_way.get_decoded::<UsbDefaultSpeed>() {
        Ok(UsbDefaultSpeed::Full) => glue::setup(Some(PortSpeed::Fs)),
        _ => glue::setup(Some(PortSpeed::Hs)),
    };
    delay(150);

    // release SE0
    iox.set_gpio_pin(se0_baosec.0, se0_baosec.1, bao1x_api::IoxValue::High);
    iox.set_gpio_pin(se0_dabao.0, se0_dabao.1, bao1x_api::IoxValue::High);
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
        if !update_good {
            marquee(sh1107, "FAILSAFE");
        } else {
            marquee(sh1107, "hello");
        }
        // update_good == true case handled in auto-reboot UI below
    }

    let mut repl = crate::repl::Repl::new();

    // do the main loop through only the serial port
    let mut delay_count: u32 = 500;
    let mut delay_s;
    let mut last_delay_s = 0;
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
            crate::println!("new state {:?}", new_usb_state);
            if new_usb_state == UsbDeviceState::Configured {
                crate::println!("USB is connected!");
                last_usb_state = new_usb_state;
                if let Some(ref mut sh1107) = oled {
                    marquee(sh1107, "Connected");
                }
            }
        }

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

        delay(10);
        if update_good {
            delay_count = delay_count.saturating_sub(1);
            delay_s = delay_count / 100;
            if delay_count == 0 {
                break;
            }
            if delay_s != last_delay_s {
                let msg = format!("Update OK! Reboot: {}", delay_s + 1);
                if let Some(ref mut sh1107) = oled {
                    marquee(sh1107, &msg);
                }
                crate::println!("{}", msg);
                last_delay_s = delay_s;
            }
        }

        // break out of the loop when USB is disconnected
        if new_portsc != portsc {
            portsc = new_portsc;
            crate::println!("new portsc {:x}", portsc);
            if glue::is_disconnected(portsc) && new_usb_state == UsbDeviceState::Configured {
                crate::println!("USB disconnected!");
                break;
            }
        }
    }

    // self-reboot
    let mut rcurst = CSR::new(utra::sysctrl::HW_SYSCTRL_BASE as *mut u32);
    rcurst.wo(utra::sysctrl::SFR_RCURST0, 0x55AA);
    loop {
        // unreachable loop due to the reboot
    }
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
    sh1107.draw().ok();
}
