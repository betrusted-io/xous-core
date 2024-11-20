use core::convert::TryInto;

use cramium_hal::iox::{IoGpio, IoSetup, Iox, IoxPort, IoxValue};
use cramium_hal::minigfx::{FrameBuffer, Point};
use cramium_hal::sh1107::Mono;
use cramium_hal::udma;
use cramium_hal::usb::driver::UsbDeviceState;
use ed25519_dalek::{Digest, Signature, VerifyingKey};
use sha2::Sha512;
use simple_fatfs::PathBuf;
use utralib::generated::*;

use crate::SIGBLOCK_SIZE;
use crate::platform::cramium::gfx;
use crate::platform::cramium::sha512_digest::Sha512Prehash;
use crate::platform::cramium::usb;
use crate::platform::cramium::usb::SliceCursor;

// TODO:
//   - Port unicode font drawing into loader
//   - Support localization

// Empirically measured PORTSC when the port is unplugged. This might be a brittle way
// to detect if the device is unplugged.
const DISCONNECT_STATE: u32 = 0x40b;

// loader is not updateable here because we're XIP. But we can update these other images:
const SWAP_NAME: &'static str = "SWAP.IMG";
const KERNEL_NAME: &'static str = "XOUS.IMG";
const DEV_PUBKEY: [u8; 32] = [
    0x1c, 0x9b, 0xea, 0xe3, 0x2a, 0xea, 0xc8, 0x75, 0x07, 0xc1, 0x80, 0x94, 0x38, 0x7e, 0xff, 0x1c, 0x74,
    0x61, 0x42, 0x82, 0xaf, 0xfd, 0x81, 0x52, 0xd8, 0x71, 0x35, 0x2e, 0xdf, 0x3f, 0x58, 0xbb,
];
#[repr(C)]
struct SignatureInFlash {
    pub version: u32,
    pub signed_len: u32,
    pub signature: [u8; 64],
}

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

    let iox_kbd = iox.clone();
    let mut sh1107 = cramium_hal::sh1107::Oled128x128::new(perclk, &mut iox, &mut udma_global);

    gfx::msg(&mut sh1107, "    START to boot", Point::new(0, 16), Mono::White.into(), Mono::Black.into());
    gfx::msg(&mut sh1107, "   SELECT to update", Point::new(0, 0), Mono::White.into(), Mono::Black.into());

    sh1107.buffer_swap();
    sh1107.draw();

    // setup IO pins to check for update viability
    let (rows, cols) = cramium_hal::board::baosec::setup_kb_pins(&iox_kbd);

    let mut key_pressed = false;
    let mut do_update = false;
    while !key_pressed {
        let kps = scan_keyboard(&iox_kbd, &rows, &cols);
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

    sh1107.clear();

    if do_update {
        gfx::msg(&mut sh1107, "Connect to USB", Point::new(16, 64), Mono::White.into(), Mono::Black.into());
        sh1107.buffer_swap();
        sh1107.draw();

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

                let mut last_usb_state = usb.get_device_state();
                let mut portsc = usb.portsc_val();
                crate::println!("USB state: {:?}, {:x}", last_usb_state, portsc);
                loop {
                    let kps = scan_keyboard(&iox_kbd, &rows, &cols);
                    // only consider the first key returned in case of multi-key hit, for simplicity
                    if kps[0] == KeyPress::Select {
                        break;
                    } else if kps[0] != KeyPress::None {
                        crate::println!("Got key {:?}; ignoring", kps[0]);
                    }
                    let new_usb_state = usb.get_device_state();
                    let new_portsc = usb.portsc_val();
                    // alternately, break out of the loop when USB is disconnected
                    if new_portsc != portsc {
                        crate::println!("PP: {:x}", portsc);
                        portsc = new_portsc;
                        if portsc == DISCONNECT_STATE && new_usb_state == UsbDeviceState::Configured {
                            break;
                        }
                    }
                    if new_usb_state != last_usb_state {
                        crate::println!("USB state: {:?}", new_usb_state);
                        if new_usb_state == UsbDeviceState::Configured {
                            sh1107.clear();
                            gfx::msg(
                                &mut sh1107,
                                "Copy files to device",
                                Point::new(6, 64),
                                Mono::White.into(),
                                Mono::Black.into(),
                            );
                            gfx::msg(
                                &mut sh1107,
                                "Press SELECT",
                                Point::new(22, 46),
                                Mono::Black.into(),
                                Mono::White.into(),
                            );
                            gfx::msg(
                                &mut sh1107,
                                "when finished!",
                                Point::new(19, 32),
                                Mono::Black.into(),
                                Mono::White.into(),
                            );
                            sh1107.buffer_swap();
                            sh1107.draw();
                            last_usb_state = new_usb_state;
                        }
                    }
                }

                let disk = usb::conjure_disk();
                let mut cursor = SliceCursor::new(disk);

                // We can either pass by value of by (mutable) reference
                let mut fs = simple_fatfs::FileSystem::from_storage(&mut cursor).unwrap();
                match fs.read_dir(PathBuf::from("/")) {
                    Ok(dir) => {
                        for entry in dir {
                            crate::println!("{:?}", entry);
                            if let Some(file_name) = entry.path().file_name() {
                                if file_name.to_ascii_uppercase() == KERNEL_NAME {
                                    match fs.get_file(entry.path().clone()) {
                                        Ok(f) => {
                                            let sector_offset = f.sector_offset();
                                            crate::println!("sector offset: {}", sector_offset);
                                            let disk_access = usb::conjure_disk();
                                            crate::println!(
                                                "{:x?}",
                                                &disk_access[sector_offset as usize * 512
                                                    ..sector_offset as usize * 512 + 32]
                                            );
                                            let pubkey = VerifyingKey::from_bytes(&DEV_PUBKEY)
                                                .expect("public key was not valid");
                                            crate::println!("pubkey as reconstituted: {:x?}", pubkey);

                                            let k_start = sector_offset as usize * 512;
                                            let sig_region = &disk_access
                                                [k_start..k_start + core::mem::size_of::<SignatureInFlash>()];
                                            let sig_rec: &SignatureInFlash = (sig_region.as_ptr()
                                                as *const SignatureInFlash)
                                                .as_ref()
                                                .unwrap(); // this pointer better not be null, we just created it!
                                            let sig = Signature::from_bytes(&sig_rec.signature);

                                            let kern_len = sig_rec.signed_len as usize;
                                            crate::println!("recorded kernel len: {} bytes", kern_len);
                                            crate::println!(
                                                "verifying with signature {:x?}",
                                                sig_rec.signature
                                            );
                                            crate::println!("verifying with pubkey {:x?}", pubkey.to_bytes());

                                            let mut h: Sha512 = Sha512::new();
                                            let image = &disk_access[k_start + SIGBLOCK_SIZE
                                                ..k_start + SIGBLOCK_SIZE + sig_rec.signed_len as usize];
                                            h.update(&image);
                                            let hash = h.finalize();
                                            let mut ph = Sha512Prehash::new();
                                            ph.set_prehash(hash.as_slice().try_into().unwrap());
                                            match pubkey.verify_prehashed(ph, None, &sig) {
                                                Ok(()) => crate::println!("Kernel image is good!"),
                                                Err(e) => {
                                                    crate::println!("error verifying signature: {:?}", e);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            crate::println!("Couldn't access image: {:?}", e);
                                        }
                                    }
                                } else if file_name.to_ascii_lowercase() == SWAP_NAME {
                                    crate::println!("Found swap image");
                                }
                            }
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

    gfx::msg(&mut sh1107, "   Booting Xous...", Point::new(0, 64), Mono::White.into(), Mono::Black.into());
    sh1107.buffer_swap();
    sh1107.draw();
    sh1107.clear();
}
