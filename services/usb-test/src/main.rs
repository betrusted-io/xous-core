#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;

use api::*;
#[cfg(any(feature="precursor", feature="renode"))]
mod kbd;
#[cfg(any(feature="precursor", feature="renode"))]
mod hw;
#[cfg(any(feature="precursor", feature="renode"))]
use hw::*;
#[cfg(any(feature="precursor", feature="renode"))]
mod spinal_udc;
#[cfg(any(feature="precursor", feature="renode"))]
use spinal_udc::*;

#[cfg(not(target_os = "xous"))]
mod hosted;
#[cfg(not(target_os = "xous"))]
use hosted::*;


use num_traits::*;
use xous::{CID, msg_scalar_unpack, Message, send_message};
use std::collections::BTreeMap;

use usb_device::prelude::*;
use usb_device::class_prelude::*;
use usbd_human_interface_device::page::Keyboard;
use usbd_human_interface_device::device::keyboard::NKROBootKeyboardInterface;
use usbd_human_interface_device::prelude::*;
use embedded_time::Clock;
use std::convert::TryInto;

pub struct EmbeddedClock {
    start: std::time::Instant,
}
impl EmbeddedClock {
    pub fn new() -> EmbeddedClock {
        EmbeddedClock { start: std::time::Instant::now() }
    }
}

impl Clock for EmbeddedClock {
    type T = u64;
    const SCALING_FACTOR: embedded_time::fraction::Fraction = <embedded_time::fraction::Fraction>::new(1, 1_000);

    fn try_now(&self) -> Result<embedded_time::Instant<Self>, embedded_time::clock::Error> {
        Ok(embedded_time::Instant::new(self.start.elapsed().as_millis().try_into().unwrap()))
    }
}

fn main() -> ! {
    let _gpio_base = crate::log_init();
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let usbdev_sid = xns.register_name(api::SERVER_NAME_USBTEST, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", usbdev_sid);

    let usbdev = SpinalUsbDevice::new(usbdev_sid);
    usbdev.init();
    let mut usbmgmt = usbdev.get_iface();
    let mut kbd = kbd::Keyboard::new(usbdev_sid);
    let tt = ticktimer_server::Ticktimer::new().unwrap();
    log::info!("connecting device core");
    usbmgmt.connect_device_core(true);
    tt.sleep_ms(500).unwrap();
    log::info!("devcore connected");

    log::trace!("ready to accept requests");

    std::thread::spawn({
        move || {
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            loop {
                // keeps the WDT from firing
                tt.sleep_ms(2500).unwrap();
            }
        }
    });

    // register a suspend/resume listener
    let cid = xous::connect(usbdev_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(
        None,
        &xns,
        api::Opcode::SuspendResume as u32,
        cid
    ).expect("couldn't create suspend/resume object");

    let usb_alloc = UsbBusAllocator::new(usbdev);
    let clock = EmbeddedClock::new();
    let mut keyboard = UsbHidClassBuilder::new()
        .add_interface(
            NKROBootKeyboardInterface::default_config(&clock),
        )
        .build(&usb_alloc);
    let mut usb_dev = UsbDeviceBuilder::new(&usb_alloc, UsbVidPid(0x1209, 0x3613))
        .manufacturer("usbd-human-interface-device")
        .product("NKRO Keyboard")
        .serial_number("PRECURSOR")
        .build();

    let mut cmdline = String::new();
    loop {
        let msg = xous::receive_message(usbdev_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                kbd.suspend();
                usbmgmt.xous_suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                kbd.resume();
                usbmgmt.xous_resume();
            }),
            Some(Opcode::UsbIrqHandler) => {
                if usb_dev.poll(&mut [&mut keyboard]) {
                    match keyboard.interface().read_report() {
                        Ok(l) => {
                            log::info!("got led state {:?}", l);
                        }
                        Err(e) => log::trace!("KEYB ERR: {:?}", e),
                    }
                }
            }
            Some(Opcode::DoCmd) => {
                log::info!("got command line: {}", cmdline);
                if let Some((cmd, args)) = cmdline.split_once(' ') {
                    // command and args
                    match cmd {
                        "test" => {
                            log::info!("got test command with arg {}", args);
                        }
                        "conn" => {
                            match args {
                                "1" => {
                                    usbmgmt.connect_device_core(true);
                                    log::info!("device core connected");
                                },
                                "0" => {
                                    usbmgmt.connect_device_core(false);
                                    log::info!("debug core connected");
                                },
                                _ => log::info!("usage: conn [1,0]; got: 'conn {}'", args),
                            }
                            usbmgmt.print_regs();
                        }
                        _ => {
                            log::info!("unrecognized command {}", cmd);
                        }
                    }
                } else {
                    // just the command
                    match cmdline.as_str() {
                        "help" => {
                            log::info!("wouldn't that be nice...");
                        }
                        "conn" => {
                            usbmgmt.connect_device_core(true);
                            log::info!("device core connected");
                            usbmgmt.print_regs();
                        }
                        "regs" => {
                            usbmgmt.print_regs();
                        }
                        _ => {
                            log::info!("unrecognized command");
                        }
                    }
                }
                cmdline.clear();
            }
            // this is via UART
            Some(Opcode::KeyboardChar) => msg_scalar_unpack!(msg, k, _, _, _, {
                let key = {
                    let bs_del_fix = if k == 0x7f {
                        0x08
                    } else {
                        k
                    };
                    core::char::from_u32(bs_del_fix as u32).unwrap_or('\u{0000}')
                };
                if key != '\u{0000}' {
                    if key != '\u{000d}' {
                        cmdline.push(key);
                    } else {
                        send_message(cid, Message::new_scalar(
                            Opcode::DoCmd.to_usize().unwrap(), 0, 0, 0, 0
                        )).unwrap();
                    }
                }
            }),
            // this is via physical keyboard
            Some(Opcode::HandlerTrigger) => {
                let rawstates = kbd.update();
                // interpret scancodes
                let kc: Vec<char> = kbd.track_keys(&rawstates);
                // handle keys, if any
                for &key in kc.iter() {
                    // send it to the USB interface
                    let code = hid_convert(key);
                    keyboard.interface().write_report(&code).ok();
                    keyboard.interface().tick().unwrap();
                    tt.sleep_ms(20).unwrap();
                    keyboard.interface().write_report(&[]).ok(); // this is the key-up
                    keyboard.interface().tick().unwrap();

                    if key != '\u{000d}' {
                        cmdline.push(key);
                    } else {
                        send_message(cid, Message::new_scalar(
                            Opcode::DoCmd.to_usize().unwrap(), 0, 0, 0, 0
                        )).unwrap();
                    }
                }
            },
            Some(Opcode::Quit) => {
                log::warn!("Quit received, goodbye world!");
                break;
            },
            None => {
                log::error!("couldn't convert opcode: {:?}", msg);
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(usbdev_sid).unwrap();
    xous::destroy_server(usbdev_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}

pub(crate) const START_OFFSET: u32 = 0x0048 + 8 + 16; // align spinal free space to 16-byte boundary + 16 bytes for EP0 read
pub(crate) const END_OFFSET: u32 = 0x1000; // derived from RAMSIZE parameter: this could be a dynamically read out constant, but, in practice, it's part of the hardware
/// USB endpoint allocator. The SpinalHDL USB controller appears as a block of
/// unstructured memory to the host. You can specify pointers into the memory with
/// an offset and length to define where various USB descriptors should be placed.
/// This allocator manages that space.
///
/// Note that all allocations must be aligned to 16-byte boundaries. This is a restriction
/// of the USB core.
///
/// Returns a full memory address as the pointer. Must be shifted left by 4 to get the
/// aligned representation used by the SpinalHDL block.
pub(crate) fn alloc_inner(allocs: &mut BTreeMap<u32, u32>, requested: u32) -> Option<u32> {
    if requested == 0 {
        return None;
    }
    let mut alloc_offset = START_OFFSET;
    for (&offset, &length) in allocs.iter() {
        // round length up to the nearest 16-byte increment
        let length = if length & 0xF == 0 { length } else { (length + 16) & !0xF };
        // println!("aoff: {}, cur: {}+{}", alloc_offset, offset, length);
        assert!(offset >= alloc_offset, "allocated regions overlap");
        if offset > alloc_offset {
            if offset - alloc_offset >= requested {
                // there's a hole in the list, insert the element here
                break;
            }
        }
        alloc_offset = offset + length;
    }
    if alloc_offset + requested <= END_OFFSET {
        allocs.insert(alloc_offset, requested);
        Some(alloc_offset)
    } else {
        None
    }
}
#[allow(dead_code)]
pub(crate) fn dealloc_inner(allocs: &mut BTreeMap<u32, u32>, offset: u32) -> bool {
    allocs.remove(&offset).is_some()
}

// run with `cargo test -- --nocapture --test-threads=1`:
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_alloc() {
        use rand_chacha::ChaCha8Rng;
        use rand_chacha::rand_core::SeedableRng;
        use rand_chacha::rand_core::RngCore;
        let mut rng = ChaCha8Rng::seed_from_u64(0);

        let mut allocs = BTreeMap::<u32, u32>::new();
        assert_eq!(alloc_inner(&mut allocs, 128), Some(START_OFFSET));
        assert_eq!(alloc_inner(&mut allocs, 64), Some(START_OFFSET + 128));
        assert_eq!(alloc_inner(&mut allocs, 256), Some(START_OFFSET + 128 + 64));
        assert_eq!(alloc_inner(&mut allocs, 128), Some(START_OFFSET + 128 + 64 + 256));
        assert_eq!(alloc_inner(&mut allocs, 128), Some(START_OFFSET + 128 + 64 + 256 + 128));
        assert_eq!(alloc_inner(&mut allocs, 128), Some(START_OFFSET + 128 + 64 + 256 + 128 + 128));
        assert_eq!(alloc_inner(&mut allocs, 0xFF00), None);

        // create two holes and fill first hole, interleaved
        assert_eq!(dealloc_inner(&mut allocs, START_OFFSET + 128 + 64), true);
        let mut last_alloc = 0;
        // consistency check and print out
        for (&offset, &len) in allocs.iter() {
            assert!(offset >= last_alloc, "new offset is inside last allocation!");
            println!("{}-{}", offset, offset+len);
            last_alloc = offset + len;
        }

        assert_eq!(alloc_inner(&mut allocs, 128), Some(START_OFFSET + 128 + 64));
        assert_eq!(dealloc_inner(&mut allocs, START_OFFSET + 128 + 64 + 256 + 128), true);
        assert_eq!(alloc_inner(&mut allocs, 128), Some(START_OFFSET + 128 + 64 + 128));

        // alloc something that doesn't fit at all
        assert_eq!(alloc_inner(&mut allocs, 256), Some(START_OFFSET + 128 + 64 + 256 + 128 + 128 + 128));

        // fill second hole
        assert_eq!(alloc_inner(&mut allocs, 128), Some(START_OFFSET + 128 + 64 + 256 + 128));

        // final tail alloc
        assert_eq!(alloc_inner(&mut allocs, 64), Some(START_OFFSET + 128 + 64 + 256 + 128 + 128 + 128 + 256));

        println!("after structured test:");
        let mut last_alloc = 0;
        // consistency check and print out
        for (&offset, &len) in allocs.iter() {
            assert!(offset >= last_alloc, "new offset is inside last allocation!");
            println!("{}-{}({})", offset, offset+len, len);
            last_alloc = offset + len;
        }

        // random alloc/dealloc and check for overlapping regions
        let mut tracker = Vec::<u32>::new();
        for _ in 0..10240 {
            if rng.next_u32() % 2 == 0 {
                if tracker.len() > 0 {
                    //println!("tracker: {:?}", tracker);
                    let index = tracker.remove((rng.next_u32() % tracker.len() as u32) as usize);
                    //println!("removing: {} of {}", index, tracker.len());
                    assert_eq!(dealloc_inner(&mut allocs, index), true);
                }
            } else {
                let req = rng.next_u32() % 256;
                if let Some(offset) = alloc_inner(&mut allocs, req) {
                    //println!("tracker: {:?}", tracker);
                    //println!("alloc: {}+{}", offset, req);
                    tracker.push(offset);
                }
            }
        }

        let mut last_alloc = 0;
        // consistency check and print out
        println!("after random test:");
        for (&offset, &len) in allocs.iter() {
            assert!(offset >= last_alloc, "new offset is inside last allocation!");
            assert!(offset & 0xF == 0, "misaligned allocation detected");
            println!("{}-{}({})", offset, offset+len, len);
            last_alloc = offset + len;
        }
    }
}

fn hid_convert(key: char) -> Vec<Keyboard> {
    let mut code = vec![];
    match key {
        'a' => code.push(Keyboard::A),
        'b' => code.push(Keyboard::B),
        'c' => code.push(Keyboard::C),
        'd' => code.push(Keyboard::D),
        'e' => code.push(Keyboard::E),
        'f' => code.push(Keyboard::F),
        'g' => code.push(Keyboard::G),
        'h' => code.push(Keyboard::H),
        'i' => code.push(Keyboard::I),
        'j' => code.push(Keyboard::J),
        'k' => code.push(Keyboard::K),
        'l' => code.push(Keyboard::L),
        'm' => code.push(Keyboard::M),
        'n' => code.push(Keyboard::N),
        'o' => code.push(Keyboard::O),
        'p' => code.push(Keyboard::P),
        'q' => code.push(Keyboard::Q),
        'r' => code.push(Keyboard::R),
        's' => code.push(Keyboard::S),
        't' => code.push(Keyboard::T),
        'u' => code.push(Keyboard::U),
        'v' => code.push(Keyboard::V),
        'w' => code.push(Keyboard::W),
        'x' => code.push(Keyboard::X),
        'y' => code.push(Keyboard::Y),
        'z' => code.push(Keyboard::Z),

        'A' => {code.push(Keyboard::A); code.push(Keyboard::LeftShift)},
        'B' => {code.push(Keyboard::B); code.push(Keyboard::LeftShift)},
        'C' => {code.push(Keyboard::C); code.push(Keyboard::LeftShift)},
        'D' => {code.push(Keyboard::D); code.push(Keyboard::LeftShift)},
        'E' => {code.push(Keyboard::E); code.push(Keyboard::LeftShift)},
        'F' => {code.push(Keyboard::F); code.push(Keyboard::LeftShift)},
        'G' => {code.push(Keyboard::G); code.push(Keyboard::LeftShift)},
        'H' => {code.push(Keyboard::H); code.push(Keyboard::LeftShift)},
        'I' => {code.push(Keyboard::I); code.push(Keyboard::LeftShift)},
        'J' => {code.push(Keyboard::J); code.push(Keyboard::LeftShift)},
        'K' => {code.push(Keyboard::K); code.push(Keyboard::LeftShift)},
        'L' => {code.push(Keyboard::L); code.push(Keyboard::LeftShift)},
        'M' => {code.push(Keyboard::M); code.push(Keyboard::LeftShift)},
        'N' => {code.push(Keyboard::N); code.push(Keyboard::LeftShift)},
        'O' => {code.push(Keyboard::O); code.push(Keyboard::LeftShift)},
        'P' => {code.push(Keyboard::P); code.push(Keyboard::LeftShift)},
        'Q' => {code.push(Keyboard::Q); code.push(Keyboard::LeftShift)},
        'R' => {code.push(Keyboard::R); code.push(Keyboard::LeftShift)},
        'S' => {code.push(Keyboard::S); code.push(Keyboard::LeftShift)},
        'T' => {code.push(Keyboard::T); code.push(Keyboard::LeftShift)},
        'U' => {code.push(Keyboard::U); code.push(Keyboard::LeftShift)},
        'V' => {code.push(Keyboard::V); code.push(Keyboard::LeftShift)},
        'W' => {code.push(Keyboard::W); code.push(Keyboard::LeftShift)},
        'X' => {code.push(Keyboard::X); code.push(Keyboard::LeftShift)},
        'Y' => {code.push(Keyboard::Y); code.push(Keyboard::LeftShift)},
        'Z' => {code.push(Keyboard::Z); code.push(Keyboard::LeftShift)},

        '0' => code.push(Keyboard::Keyboard0),
        '1' => code.push(Keyboard::Keyboard1),
        '2' => code.push(Keyboard::Keyboard2),
        '3' => code.push(Keyboard::Keyboard3),
        '4' => code.push(Keyboard::Keyboard4),
        '5' => code.push(Keyboard::Keyboard5),
        '6' => code.push(Keyboard::Keyboard6),
        '7' => code.push(Keyboard::Keyboard7),
        '8' => code.push(Keyboard::Keyboard8),
        '9' => code.push(Keyboard::Keyboard9),

        '←' => code.push(Keyboard::LeftArrow),
        '→' => code.push(Keyboard::RightArrow),
        '↑' => code.push(Keyboard::UpArrow),
        '↓' => code.push(Keyboard::DownArrow),

        ',' => code.push(Keyboard::Comma),
        '.' => code.push(Keyboard::Dot),

        '\u{000d}' => code.push(Keyboard::ReturnEnter),
        ' ' => code.push(Keyboard::Space),
        '\u{0008}' => code.push(Keyboard::DeleteBackspace),
        _ => log::info!("Unhandled character: {}", key),
    };
    code
}