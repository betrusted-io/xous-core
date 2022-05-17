#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
mod mappings;

use api::*;
#[cfg(any(target_os = "none", target_os = "xous"))]
mod hw;
#[cfg(any(target_os = "none", target_os = "xous"))]
use hw::*;
#[cfg(any(target_os = "none", target_os = "xous"))]
mod spinal_udc;
#[cfg(any(target_os = "none", target_os = "xous"))]
use packed_struct::PackedStructSlice;
#[cfg(any(target_os = "none", target_os = "xous"))]
use spinal_udc::*;

#[cfg(not(any(target_os = "none", target_os = "xous")))]
mod hosted;
#[cfg(not(any(target_os = "none", target_os = "xous")))]
use hosted::*;


use num_traits::*;
#[cfg(any(target_os = "none", target_os = "xous"))]
use usb_device_xous::KeyboardLedsReport;
use xous::{msg_scalar_unpack, msg_blocking_scalar_unpack};
use std::collections::BTreeMap;

#[cfg(any(target_os = "none", target_os = "xous"))]
use usb_device::prelude::*;
#[cfg(any(target_os = "none", target_os = "xous"))]
use usb_device::class_prelude::*;
#[cfg(any(target_os = "none", target_os = "xous"))]
use usbd_human_interface_device::page::Keyboard;
#[cfg(any(target_os = "none", target_os = "xous"))]
use usbd_human_interface_device::device::keyboard::NKROBootKeyboardInterface;
#[cfg(any(target_os = "none", target_os = "xous"))]
use usbd_human_interface_device::prelude::*;
#[cfg(any(target_os = "none", target_os = "xous"))]
use num_enum::FromPrimitive as EnumFromPrimitive;

use embedded_time::Clock;
use std::convert::TryInto;
use keyboard::KeyMap;
use xous_ipc::Buffer;

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

#[xous::xous_main]
fn xmain() -> ! {
    use crate::SpinalUsbDevice;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let usbdev_sid = xns.register_name(api::SERVER_NAME_USB_DEVICE, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", usbdev_sid);
    let llio = llio::Llio::new(&xns);
    let tt = ticktimer_server::Ticktimer::new().unwrap();
    let native_kbd = keyboard::Keyboard::new(&xns).unwrap();
    let native_map = native_kbd.get_keymap().unwrap();

    #[cfg(any(target_os = "none", target_os = "xous"))]
    let serial_number = format!("{:x}", llio.soc_dna().unwrap());
    let (maj, min, rev, extra, _gitrev) = llio.soc_gitrev().unwrap();
    if maj == 0 && min <= 9 && rev <= 8 {
        if (min == 9 && rev == 8 && extra < 20) ||
        (min < 9) || (min == 9 && rev < 8) {
            if min != 0 { // don't show during hosted mode, which reports 0.0.0+0
                tt.sleep_ms(1500).ok(); // wait for some system boot to happen before popping up the modal
                let modals = modals::Modals::new(&xns).unwrap();
                modals.show_notification(
                    &format!("SoC version >= 0.9.8+20 required for USB HID. Detected rev: {}.{}.{}+{}. Refusing to start USB driver.",
                    maj, min, rev, extra
                ),
                    None
                ).unwrap();
            }
            loop {
                let msg = xous::receive_message(usbdev_sid).unwrap();
                match FromPrimitive::from_usize(msg.body.id()) {
                    Some(Opcode::DebugUsbOp) => msg_blocking_scalar_unpack!(msg, _update_req, _new_state, _, _, {
                        xous::return_scalar2(msg.sender, 0, 1).expect("couldn't return status");
                    }),
                    _ => {
                        log::warn!("SoC not compatible with HID, ignoring USB message: {:?}", msg);
                        // make it so blocking scalars don't block
                        if let xous::Message::BlockingScalar(xous::ScalarMessage {
                            id: _,
                            arg1: _,
                            arg2: _,
                            arg3: _,
                            arg4: _,
                        }) = msg.body {
                            log::warn!("Returning bogus result");
                            xous::return_scalar(msg.sender, 0).unwrap();
                        }
                    }
                }
            }
        }
    }

    let usbdev = SpinalUsbDevice::new(usbdev_sid);
    let mut usbmgmt = usbdev.get_iface();

    // register a suspend/resume listener
    let cid = xous::connect(usbdev_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(
        None,
        &xns,
        api::Opcode::SuspendResume as u32,
        cid
    ).expect("couldn't create suspend/resume object");

    #[cfg(any(target_os = "none", target_os = "xous"))]
    let usb_alloc = UsbBusAllocator::new(usbdev);
    #[cfg(any(target_os = "none", target_os = "xous"))]
    let clock = EmbeddedClock::new();
    #[cfg(any(target_os = "none", target_os = "xous"))]
    let mut keyboard = UsbHidClassBuilder::new()
        .add_interface(
            NKROBootKeyboardInterface::default_config(&clock),
        )
        .build(&usb_alloc);
    #[cfg(any(target_os = "none", target_os = "xous"))]
    let mut usb_dev = UsbDeviceBuilder::new(&usb_alloc, UsbVidPid(0x1209, 0x0001))
        .manufacturer("Kosagi")
        .product("Precursor")
        .serial_number(&serial_number)
        .build();

    #[cfg(any(target_os = "none", target_os = "xous"))]
    let mut led_state: KeyboardLedsReport = KeyboardLedsReport::default();
    let mut lockstatus_force_update = true; // some state to track if we've been through a susupend/resume, to help out the status thread with its UX update after a restart-from-cold
    loop {
        let mut msg = xous::receive_message(usbdev_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::SuspendResume) => msg_scalar_unpack!(msg, token, _, _, _, {
                usbmgmt.xous_suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                usbmgmt.xous_resume();
                lockstatus_force_update = true; // notify the status bar that yes, it does need to redraw the lock status, even if the value hasn't changed since the last read
            }),
            Some(Opcode::UsbIrqHandler) => {
                #[cfg(any(target_os = "none", target_os = "xous"))]
                if usb_dev.poll(&mut [&mut keyboard]) {
                    match keyboard.interface().read_report() {
                        Ok(l) => {
                            led_state = l;
                        }
                        Err(e) => log::trace!("KEYB ERR: {:?}", e),
                    }
                }
            },
            Some(Opcode::SwitchCores) => msg_blocking_scalar_unpack!(msg, core, _, _, _, {
                if core == 1 {
                    log::info!("Connecting USB device core; disconnecting debug USB core");
                    usbmgmt.connect_device_core(true);
                    tt.sleep_ms(500).unwrap();
                } else {
                    log::info!("Connecting debug core; disconnecting USB device core");
                    usbmgmt.connect_device_core(false);
                    tt.sleep_ms(500).unwrap();
                }
                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            Some(Opcode::WhichCore) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if usbmgmt.is_device_connected() {
                    xous::return_scalar(msg.sender, 1).unwrap();
                } else {
                    xous::return_scalar(msg.sender, 0).unwrap();
                }
            }),
            Some(Opcode::RestrictDebugAccess) => msg_scalar_unpack!(msg, restrict, _, _, _, {
                if restrict == 0 {
                    usbmgmt.disable_debug(false);
                } else {
                    usbmgmt.disable_debug(true);
                }
            }),
            Some(Opcode::IsRestricted) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if usbmgmt.get_disable_debug() {
                    xous::return_scalar(msg.sender, 1).unwrap();
                } else {
                    xous::return_scalar(msg.sender, 0).unwrap();
                }
            }),
            Some(Opcode::DebugUsbOp) => msg_blocking_scalar_unpack!(msg, update_req, new_state, _, _, {
                if update_req != 0 {
                    // if new_state is true (not 0), then try to lock the USB port
                    // if false, try to unlock the USB port
                    if new_state != 0 {
                        usbmgmt.disable_debug(true);
                    } else {
                        usbmgmt.disable_debug(false);
                    }
                }
                // at this point, *read back* the new state -- don't assume it "took". The readback is always based on
                // a real hardware value and not the requested value. for now, always false.
                let is_locked = if usbmgmt.get_disable_debug() {
                    1
                } else {
                    0
                };

                // this is a performance optimization. we could always redraw the status, but, instead we only redraw when
                // the status has changed. However, there is an edge case: on a resume from suspend, the status needs a redraw,
                // even if nothing has changed. Thus, we have this separate boolean we send back to force an update in the
                // case that we have just come out of a suspend.
                let force_update = if lockstatus_force_update {
                    1
                } else {
                    0
                };
                xous::return_scalar2(msg.sender, is_locked, force_update).expect("couldn't return status");
                lockstatus_force_update = false;
            }),
            Some(Opcode::LinkStatus) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                #[cfg(any(target_os = "none", target_os = "xous"))]
                xous::return_scalar(msg.sender, usb_dev.state() as usize).unwrap();
                #[cfg(not(any(target_os = "none", target_os = "xous")))]
                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            #[cfg(any(target_os = "none", target_os = "xous"))]
            Some(Opcode::SendKeyCode) => msg_blocking_scalar_unpack!(msg, code0, code1, code2, autoup, {
                if usb_dev.state() == UsbDeviceState::Configured {
                    let mut codes = Vec::<Keyboard>::new();
                    if code0 != 0 {
                        codes.push(Keyboard::from_primitive(code0 as u8));
                    }
                    if code1 != 0 {
                        codes.push(Keyboard::from_primitive(code1 as u8));
                    }
                    if code2 != 0 {
                        codes.push(Keyboard::from_primitive(code2 as u8));
                    }
                    let auto_up = if autoup == 1 {true} else {false};
                    keyboard.interface().write_report(&codes).ok();
                    keyboard.interface().tick().unwrap();
                    tt.sleep_ms(30).ok();
                    if auto_up {
                        keyboard.interface().write_report(&[]).ok(); // this is the key-up
                        keyboard.interface().tick().unwrap();
                        tt.sleep_ms(30).ok();
                    }
                    xous::return_scalar(msg.sender, 0).unwrap();
                } else {
                    xous::return_scalar(msg.sender, 1).unwrap();
                }
            }),
            #[cfg(not(any(target_os = "none", target_os = "xous")))]
            Some(Opcode::SendKeyCode) => {
                xous::return_scalar(msg.sender, 1).unwrap();
            }
            Some(Opcode::SendString) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut usb_send = buffer.to_original::<api::UsbString, _>().unwrap();
                #[cfg(any(target_os = "none", target_os = "xous"))]
                {
                    let mut sent = 0;
                    for ch in usb_send.s.as_str().unwrap().chars() {
                        // ASSUME: user's keyboard type matches the preference on their Precursor device.
                        let codes = match native_map {
                            KeyMap::Dvorak => mappings::char_to_hid_code_dvorak(ch),
                            _ => mappings::char_to_hid_code_us101(ch),
                        };
                        keyboard.interface().write_report(&codes).ok();
                        keyboard.interface().tick().unwrap();
                        tt.sleep_ms(30).ok();
                        keyboard.interface().write_report(&[]).ok(); // this is the key-up
                        keyboard.interface().tick().unwrap();
                        tt.sleep_ms(30).ok();
                        sent += 1;
                    }
                    usb_send.sent = Some(sent);
                }
                buffer.replace(usb_send).unwrap();
            }
            #[cfg(any(target_os = "none", target_os = "xous"))]
            Some(Opcode::GetLedState) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let mut code = [0u8; 1];
                led_state.pack_to_slice(&mut code).unwrap();
                xous::return_scalar(msg.sender, code[0] as usize).unwrap();
            }),
            #[cfg(not(any(target_os = "none", target_os = "xous")))]
            Some(Opcode::GetLedState) => {
                xous::return_scalar(msg.sender, 0).unwrap();
            }
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

#[cfg(any(target_os = "none", target_os = "xous"))]
pub(crate) const START_OFFSET: u32 = 0x0048 + 8 + 16; // align spinal free space to 16-byte boundary + 16 bytes for EP0 read
#[cfg(any(target_os = "none", target_os = "xous"))]
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
#[cfg(any(target_os = "none", target_os = "xous"))]
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
