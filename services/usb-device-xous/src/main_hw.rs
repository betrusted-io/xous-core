use core::num::NonZeroU8;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::collections::VecDeque;
use std::convert::TryInto;
use std::sync::Arc;

#[cfg(all(not(feature = "minimal"), any(feature = "cramium-soc")))]
use cram_hal_service::api::KeyMap;
#[cfg(feature = "cramium-soc")]
use cram_hal_service::keyboard;
#[cfg(feature = "cramium-soc")]
use cramium_hal::usb::driver::CorigineUsb;
#[cfg(all(not(feature = "minimal"), any(feature = "renode", feature = "precursor")))]
use keyboard::KeyMap;
use num_traits::*;
#[cfg(feature = "cramium-soc")]
use packed_struct::PackedStructSlice;
use usb_device::class_prelude::*;
use usb_device::prelude::*;
use usb_device_xous::KeyboardLedsReport;
use usb_device_xous::UsbDeviceType;
use usbd_serial::SerialPort;
#[cfg(any(feature = "precursor", feature = "renode"))]
use utralib::generated::*;
#[cfg(feature = "cramium-soc")]
use utralib::AtomicCsr;
use xous::{msg_blocking_scalar_unpack, msg_scalar_unpack};
use xous_ipc::Buffer;
#[cfg(all(not(feature = "minimal"), any(feature = "precursor", feature = "renode")))]
use xous_semver::SemVer;
use xous_usb_hid::device::fido::RawFido;
use xous_usb_hid::device::fido::RawFidoConfig;
use xous_usb_hid::device::fido::RawFidoReport;
use xous_usb_hid::device::keyboard::{NKROBootKeyboard, NKROBootKeyboardConfig};
use xous_usb_hid::device::DeviceClass;
use xous_usb_hid::page::Keyboard;
use xous_usb_hid::prelude::*;

use crate::hid::AppHIDConfig;
use crate::*;

/// Time allowed for switchover between device core types. It's longer because some hosts
/// get really confused when you have the same VID/PID show up with a different set of endpoints.
const EXTENDED_CORE_RESET_MS: usize = 4000;
#[derive(Eq, PartialEq, Debug)]
#[repr(usize)]
enum Views {
    FidoWithKbd = 0,
    FidoOnly = 1,
    #[cfg(feature = "mass-storage")]
    MassStorage = 2,
    Serial = 3,
    HIDv2 = 4,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
enum TimeoutOp {
    Pump,
    InvalidCall,
    Quit,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
enum TrngOp {
    Pump,
    InvalidCall,
    Quit,
}

#[derive(Debug)]
enum SerialListenMode {
    // this just causes data incoming to be printed to the debug log; it is the default
    NoListener,
    // this assumes there will be a CR/LF character to delimit lines (the `char` arg), and
    // will buffer data until two conditions are met: 1) a listener is hooked and 2) a CR/LF is received.
    // This will "infinitely" buffer incoming characters if no listener is hooked.
    AsciiListener(Option<char>),
    // this will simply buffer the data until the `usize` argument is met and passes it back to
    // hooked listener. If this mode is set and there is no listener, it will buffer data "indefinitely"
    // (e.g. until local heap is exhausted and the system panics)
    BinaryListener,
    // this will take any serial input and pass it on as if one was typing at the console
    ConsoleListener,
}

pub(crate) fn main_hw() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let usbdev_sid = xns.register_name(api::SERVER_NAME_USB_DEVICE, None).expect("can't register server");
    let cid = xous::connect(usbdev_sid).expect("couldn't create suspend callback connection");
    log::trace!("registered with NS -- {:?}", usbdev_sid);
    #[cfg(not(feature = "minimal"))]
    #[cfg(any(feature = "precursor", feature = "renode"))]
    let llio = llio::Llio::new(&xns);
    let tt = ticktimer_server::Ticktimer::new().unwrap();
    #[cfg(not(feature = "minimal"))]
    let native_kbd = keyboard::Keyboard::new(&xns).unwrap();

    #[cfg(all(not(feature = "minimal"), any(feature = "precursor", feature = "renode")))]
    let serial_number = format!("{:x}", llio.soc_dna().unwrap());
    #[cfg(all(not(feature = "minimal"), any(feature = "cramium-soc")))]
    let serial_number = format!("TODO!!"); // implement in cramium-hal once we have a serial number API

    #[cfg(all(not(feature = "minimal"), any(feature = "precursor", feature = "renode")))]
    {
        let minimum_ver = SemVer { maj: 0, min: 9, rev: 8, extra: 20, commit: None };
        let soc_ver = llio.soc_gitrev().unwrap();
        if soc_ver < minimum_ver {
            if soc_ver.min != 0 {
                // don't show during hosted mode, which reports 0.0.0+0
                tt.sleep_ms(1500).ok(); // wait for some system boot to happen before popping up the modal
                let modals = modals::Modals::new(&xns).unwrap();
                modals.show_notification(
                    &format!("SoC version >= 0.9.8+20 required for USB HID. Detected rev: {}. Refusing to start USB driver.",
                    soc_ver.to_string()
                ),
                    None
                ).unwrap();
            }
            let mut fido_listener: Option<xous::MessageEnvelope> = None;
            loop {
                let msg = xous::receive_message(usbdev_sid).unwrap();
                match FromPrimitive::from_usize(msg.body.id()) {
                    Some(Opcode::DebugUsbOp) => {
                        msg_blocking_scalar_unpack!(msg, _update_req, _new_state, _, _, {
                            xous::return_scalar2(msg.sender, 0, 1).expect("couldn't return status");
                        })
                    }
                    Some(Opcode::U2fRxDeferred) => {
                        // block any rx requests forever
                        fido_listener = Some(msg);
                    }
                    Some(Opcode::IsSocCompatible) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        xous::return_scalar(msg.sender, 0).expect("couldn't return compatibility status")
                    }),
                    Some(Opcode::Quit) => {
                        break;
                    }
                    _ => {
                        log::warn!("SoC not compatible with HID, ignoring USB message: {:?}", msg);
                        // make it so blocking scalars don't block
                        if let xous::Message::BlockingScalar(xous::ScalarMessage {
                            id: _,
                            arg1: _,
                            arg2: _,
                            arg3: _,
                            arg4: _,
                        }) = msg.body
                        {
                            log::warn!("Returning bogus result");
                            xous::return_scalar(msg.sender, 0).unwrap();
                        }
                    }
                }
            }
            log::info!("consuming listener: {:?}", fido_listener);
        }
    }
    #[cfg(feature = "minimal")]
    let serial_number = "minimalbuild";
    #[cfg(feature = "minimal")]
    {
        use utralib::generated::*;
        let gpio_base = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::gpio::HW_GPIO_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map GPIO CSR range");
        let mut gpio_csr = CSR::new(gpio_base.as_mut_ptr() as *mut u32);
        // setup the initial logging output
        gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, 1); // 0 is kernel, 1 is console
    }

    // Allocate memory range and CSR for sharing between all the views.
    #[cfg(any(feature = "precursor", feature = "renode"))]
    let usb = xous::syscall::map_memory(
        xous::MemoryAddress::new(utralib::HW_USBDEV_MEM),
        None,
        utralib::HW_USBDEV_MEM_LEN,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map USB device memory range");
    #[cfg(any(feature = "precursor", feature = "renode"))]
    let csr = xous::syscall::map_memory(
        xous::MemoryAddress::new(utra::usbdev::HW_USBDEV_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map USB CSR range");

    #[cfg(feature = "cramium-soc")]
    let usb_mapping = xous::syscall::map_memory(
        xous::MemoryAddress::new(cramium_hal::usb::utra::CORIGINE_USB_BASE),
        None,
        cramium_hal::usb::utra::CORIGINE_USB_LEN,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't reserve register pages");
    #[cfg(feature = "cramium-soc")]
    let ifram_range = xous::syscall::map_memory(
        xous::MemoryAddress::new(cramium_hal::usb::driver::CRG_UDC_MEMBASE),
        xous::MemoryAddress::new(cramium_hal::usb::driver::CRG_UDC_MEMBASE), // make P & V addresses line up
        cramium_hal::usb::driver::CRG_IFRAM_PAGES * 0x1000,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't allocate IFRAM pages");
    #[cfg(feature = "cramium-soc")]
    let usb = AtomicCsr::new(usb_mapping.as_ptr() as *mut u32);

    // Notes:
    //  - Most drivers would `Box()` the hardware management structure to make sure the compiler doesn't move
    //    its location. However, we can't do this here because we are trying to maintain compatibility with
    //    another crate that implements the USB stack which can't handle Box'd structures.
    //  - It is safe to call `.init()` repeatedly because within `init()` we have an atomic bool that tracks
    //    if the interrupt handler has been hooked, and ignores further requests to hook it.
    #[cfg(any(feature = "renode", feature = "precursor"))]
    let usb_fidokbd_dev = SpinalUsbDevice::new(usbdev_sid, usb.clone(), csr.clone());
    #[cfg(feature = "cramium-soc")]
    // safety: this is safe because we allocated ifram_range to have the same physical and virtual addresses
    let usb_fidokbd_dev = unsafe { CorigineUsb::new(cid, ifram_range.as_ptr() as usize, usb.clone()) };
    #[cfg(any(feature = "renode", feature = "precursor"))]
    usb_fidokbd_dev.init();
    #[cfg(any(feature = "renode", feature = "precursor"))]
    let mut usbmgmt = usb_fidokbd_dev.get_iface();
    #[cfg(feature = "cramium-soc")]
    // safety: this is safe because we allocated ifram_range to have the same physical and virtual addresses
    let mut usbmgmt = unsafe { CorigineUsb::new(cid, ifram_range.as_ptr() as usize, usb.clone()) };
    // before doing any allocs, clone a copy of the hardware access structure so we can build a second
    // view into the hardware with only FIDO descriptors
    #[cfg(any(feature = "renode", feature = "precursor"))]
    let usb_fido_dev: SpinalUsbDevice = SpinalUsbDevice::new(usbdev_sid, usb.clone(), csr.clone());
    #[cfg(feature = "cramium-soc")]
    // safety: this is safe because we allocated ifram_range to have the same physical and virtual addresses
    let usb_fido_dev = unsafe { CorigineUsb::new(cid, ifram_range.as_ptr() as usize, usb.clone()) };
    #[cfg(any(feature = "renode", feature = "precursor"))]
    usb_fido_dev.init();
    // do the same thing for mass storage
    #[cfg(all(feature = "mass-storage", any(feature = "precursor", feature = "renode")))]
    let ums_dev = SpinalUsbDevice::new(usbdev_sid, usb.clone(), csr.clone());
    #[cfg(all(feature = "mass-storage", any(feature = "cramium-soc")))]
    // safety: this is safe because we allocated ifram_range to have the same physical and virtual addresses
    let ums_dev = unsafe { CorigineUsb::new(cid, ifram_range.as_ptr() as usize, usb.clone()) };
    #[cfg(feature = "mass-storage")]
    #[cfg(any(feature = "renode", feature = "precursor"))]
    ums_dev.init();
    #[cfg(any(feature = "renode", feature = "precursor"))]
    let serial_dev = SpinalUsbDevice::new(usbdev_sid, usb.clone(), csr.clone());
    #[cfg(feature = "cramium-soc")]
    // safety: this is safe because we allocated ifram_range to have the same physical and virtual addresses
    let serial_dev = unsafe { CorigineUsb::new(cid, ifram_range.as_ptr() as usize, usb.clone()) };
    #[cfg(any(feature = "renode", feature = "precursor"))]
    serial_dev.init();

    #[cfg(feature = "cramium-soc")]
    usbmgmt.init(); // for the cramium target, call init only once via management structure

    // track which view is visible on the device core
    #[cfg(not(feature = "minimal"))]
    let mut view = Views::FidoWithKbd;

    // register a suspend/resume listener
    #[cfg(any(feature = "renode", feature = "precursor", feature = "hosted"))]
    let mut susres = susres::Susres::new(None, &xns, api::Opcode::SuspendResume as u32, cid)
        .expect("couldn't create suspend/resume object");

    // FIDO + keyboard
    let usb_alloc = UsbBusAllocator::new(usb_fidokbd_dev);

    let mut composite = UsbHidClassBuilder::new()
        .add_device(NKROBootKeyboardConfig::default())
        .add_device(RawFidoConfig::default())
        .build(&usb_alloc);

    let mut usb_dev = UsbDeviceBuilder::new(&usb_alloc, UsbVidPid(0x1209, 0x3613))
        .manufacturer("Kosagi")
        .product("Precursor")
        .serial_number(&serial_number)
        .build();

    // FIDO only
    let fido_alloc = UsbBusAllocator::new(usb_fido_dev);
    let mut fido_class = UsbHidClassBuilder::new().add_device(RawFidoConfig::default()).build(&fido_alloc);

    let mut fido_dev = UsbDeviceBuilder::new(&fido_alloc, UsbVidPid(0x1209, 0x3613))
        .manufacturer("Kosagi")
        .product("Precursor")
        .serial_number(&serial_number)
        .build();

    // Mass storage
    #[cfg(feature = "mass-storage")]
    let ums_alloc = UsbBusAllocator::new(ums_dev);
    #[cfg(feature = "mass-storage")]
    let abd = apps_block_device::AppsBlockDevice::new();
    #[cfg(feature = "mass-storage")]
    let abdcid = abd.conn();
    #[cfg(feature = "mass-storage")]
    let mut ums = usbd_scsi::Scsi::new(
        &ums_alloc,
        64,
        abd,
        "Kosagi".as_bytes(),
        "Kosagi Precursor".as_bytes(),
        "1".as_bytes(),
    );

    #[cfg(feature = "mass-storage")]
    let mut ums_device = UsbDeviceBuilder::new(&ums_alloc, UsbVidPid(0x1209, 0x3613))
        .manufacturer("Kosagi")
        .product("Precursor")
        .serial_number(&serial_number)
        .self_powered(false)
        .max_power(500)
        .build();

    // Serial
    const SERIAL_BUF_LEN: usize = 1024; // length of the internal character buffer. This is not the *hardware* buffer; this is a buffer we maintain in the driver to improve performance
    let serial_alloc = UsbBusAllocator::new(serial_dev);
    // this will create a default port with 128 bytes of backing store
    let mut serial_port = SerialPort::new(&serial_alloc);
    let mut serial_device = UsbDeviceBuilder::new(&serial_alloc, UsbVidPid(0x1209, 0x3613))
        .manufacturer("Kosagi")
        .product("Precursor")
        .serial_number(&serial_number)
        .self_powered(false)
        .max_power(500)
        .build();
    let mut serial_listener: Option<xous::MessageEnvelope> = None;
    let mut serial_listen_mode: SerialListenMode = SerialListenMode::NoListener;
    let mut serial_buf = Vec::<u8>::new();
    let mut serial_rx_trigger = false; // when true, the condition was met to pass data to the listener (but the listener was not yet installed)
    let trng = trng::Trng::new(&xns).unwrap();
    let mut serial_trng_buf = Vec::<u8>::new();
    let serial_trng_interval = Arc::new(AtomicU32::new(0));
    let mut serial_trng_cid: Option<xous::CID> = None;
    const TRNG_PKT_SIZE: usize = 64; // size of a TRNG packet being sent. This is inferred from the spec.
    const TRNG_INITIAL_DELAY_MS: u32 = 200; // the very first poll takes longer, because we have to fill the TRNG back-end
    const TRNG_REFILL_DELAY_MS: u32 = 1; // we re-poll very fast once we see the host taking data
    const TRNG_BACKOFF_MS: u32 = 1;
    const TRNG_BACKOFF_MAX_MS: u32 = 1000; // cap on how far we backoff the polling rate

    #[cfg(any(feature = "renode", feature = "precursor"))]
    let usb_hidv2_dev = SpinalUsbDevice::new(usbdev_sid, usb.clone(), csr.clone());
    #[cfg(feature = "cramium-soc")]
    // safety: this is safe because we allocated ifram_range to have the same physical and virtual addresses
    let usb_hidv2_dev = unsafe { CorigineUsb::new(cid, ifram_range.as_ptr() as usize, usb.clone()) };

    let hidv2_alloc = UsbBusAllocator::new(usb_hidv2_dev);

    let mut hidv2 = hid::AppHID::new(
        UsbVidPid(0x1209, 0x3613),
        &serial_number,
        &hidv2_alloc,
        AppHIDConfig::default(),
        100, // 100 * 64 bytes = 6.4kb, quite the backlog
    );

    let mut led_state: KeyboardLedsReport = KeyboardLedsReport::default();
    let mut fido_listener: Option<xous::MessageEnvelope> = None;
    // under the theory that PIDs cannot be forged. TODO: check that PIDs cannot be forged.
    // also if someone commandeers a process, all bets are off within that process (this is a general
    // statement)
    let mut fido_listener_pid: Option<NonZeroU8> = None;
    let mut fido_rx_queue = VecDeque::<[u8; 64]>::new();

    let mut lockstatus_force_update = true; // some state to track if we've been through a suspend/resume, to help out the status thread with its UX update after a restart-from-cold
    let mut was_suspend = true;
    let mut autotype_delay_ms = 30;

    // event observer connection
    let mut observer_conn: Option<xous::CID> = None;
    let mut observer_op: Option<usize> = None;

    #[cfg(feature = "minimal")]
    std::thread::spawn(move || {
        // this keeps the watchdog alive in minimal mode; if there's no event, eventually the watchdog times
        // out
        let tt = ticktimer_server::Ticktimer::new().unwrap();
        loop {
            tt.sleep_ms(1500).ok();
        }
    });
    // switch the core automatically on boot
    #[cfg(feature = "minimal")]
    let mut view = Views::MassStorage;
    #[cfg(feature = "minimal")]
    {
        usbmgmt.ll_reset(true);
        tt.sleep_ms(1000).ok();
        usbmgmt.ll_connect_device_core(true);
        tt.sleep_ms(EXTENDED_CORE_RESET_MS).ok();
        usbmgmt.ll_reset(false);
    }
    // manage FIDO Rx timeouts -- not tested yet
    let to_server = xous::create_server().unwrap();
    let to_conn = xous::connect(to_server).unwrap();
    // we don't have AtomicU64 on this platform, so we suffer from a rollover condition in timeouts once every
    // 46 days this manifests as a timeout that happens to be scheduled on the rollover being rounded to a
    // max limit timeout of 5 seconds, and/or an immediate timeout happening during the 5 seconds before
    // the 46-day limit
    let target_time_lsb = Arc::new(AtomicU32::new(0));
    let to_run = Arc::new(AtomicBool::new(false));
    const MAX_TIMEOUT_LIMIT_MS: u32 = 5000;
    const POLL_INTERVAL_MS: u64 = 50;
    std::thread::spawn({
        let cid = cid;
        let to_conn = to_conn;
        let target_time_lsb = target_time_lsb.clone();
        let to_run = to_run.clone();
        move || {
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            let mut msg_opt = None;
            let mut return_type = 0;
            let mut next_wake = tt.elapsed_ms();
            loop {
                xous::reply_and_receive_next_legacy(to_server, &mut msg_opt, &mut return_type).unwrap();
                let msg = msg_opt.as_mut().unwrap();
                // loop only consumes CPU time when a timeout is active. Once it has timed out,
                // it will wait for a new pump call.
                let now = tt.elapsed_ms();
                match num_traits::FromPrimitive::from_usize(msg.body.id()).unwrap_or(TimeoutOp::InvalidCall) {
                    TimeoutOp::Pump => {
                        if to_run.load(Ordering::SeqCst) {
                            let tt_lsb = target_time_lsb.load(Ordering::SeqCst);
                            if tt_lsb >= (now as u32) || (now as u32) - tt_lsb > MAX_TIMEOUT_LIMIT_MS
                            // limits rollover case
                            {
                                xous::try_send_message(
                                    cid,
                                    xous::Message::new_scalar(
                                        Opcode::U2fRxTimeout.to_usize().unwrap(),
                                        0,
                                        0,
                                        0,
                                        0,
                                    ),
                                )
                                .ok();
                                // no need to set to_run to `false` because a Pump message isn't initiated;
                                // the loop de-facto stops from a lack of new Pump messages
                            } else {
                                if next_wake <= now {
                                    next_wake = now + POLL_INTERVAL_MS;
                                    tt.sleep_ms(POLL_INTERVAL_MS as usize).ok();
                                    xous::try_send_message(
                                        to_conn,
                                        xous::Message::new_scalar(
                                            TimeoutOp::Pump.to_usize().unwrap(),
                                            0,
                                            0,
                                            0,
                                            0,
                                        ),
                                    )
                                    .ok();
                                } else {
                                    // don't issue more wakeups if we already have a wakeup scheduled
                                }
                            }
                        }
                    }
                    TimeoutOp::Quit => {
                        if let Some(scalar) = msg.body.scalar_message_mut() {
                            scalar.id = 0;
                            scalar.arg1 = 1;
                            break;
                        }
                    }
                    TimeoutOp::InvalidCall => {
                        log::error!(
                            "Unknown opcode received in FIDO Rx timeout handler: {:?}",
                            msg.body.id()
                        );
                    }
                }
            }
            xous::destroy_server(to_server).unwrap();
        }
    });

    loop {
        let mut msg = xous::receive_message(usbdev_sid).unwrap();
        let opcode: Option<Opcode> = FromPrimitive::from_usize(msg.body.id());
        log::debug!("{:?}", opcode);
        match opcode {
            #[cfg(feature = "mass-storage")]
            Some(Opcode::SetBlockDevice) => {
                msg_blocking_scalar_unpack!(msg, read_id, write_id, max_lba_id, _, {
                    xous::send_message(
                        abdcid,
                        xous::Message::new_blocking_scalar(
                            apps_block_device::BlockDeviceMgmtOp::SetOps.to_usize().unwrap(),
                            read_id,
                            write_id,
                            max_lba_id,
                            0,
                        ),
                    )
                    .unwrap();
                    xous::return_scalar(msg.sender, 0).unwrap();
                })
            }
            #[cfg(feature = "mass-storage")]
            Some(Opcode::SetBlockDeviceSID) => msg_blocking_scalar_unpack!(msg, sid1, sid2, sid3, sid4, {
                xous::send_message(
                    abdcid,
                    xous::Message::new_blocking_scalar(
                        apps_block_device::BlockDeviceMgmtOp::SetSID.to_usize().unwrap(),
                        sid1,
                        sid2,
                        sid3,
                        sid4,
                    ),
                )
                .unwrap();
                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            #[cfg(feature = "mass-storage")]
            Some(Opcode::ResetBlockDevice) => msg_blocking_scalar_unpack!(msg, 0, 0, 0, 0, {
                xous::send_message(
                    abdcid,
                    xous::Message::new_blocking_scalar(
                        apps_block_device::BlockDeviceMgmtOp::Reset.to_usize().unwrap(),
                        0,
                        0,
                        0,
                        0,
                    ),
                )
                .unwrap();
                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            #[cfg(any(feature = "renode", feature = "precursor", feature = "hosted"))]
            Some(Opcode::SuspendResume) => msg_scalar_unpack!(msg, token, _, _, _, {
                usbmgmt.xous_suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                // resume1 + reset brings us to an initialized state
                usbmgmt.xous_resume1();
                match view {
                    Views::FidoWithKbd => {
                        match usb_dev.force_reset() {
                            Err(e) => log::warn!("USB reset on resume failed: {:?}", e),
                            _ => (),
                        };
                    }
                    Views::FidoOnly => {
                        match fido_dev.force_reset() {
                            Err(e) => log::warn!("USB reset on resume failed: {:?}", e),
                            _ => (),
                        };
                    }
                    #[cfg(feature = "mass-storage")]
                    Views::MassStorage => {
                        // TODO: test this
                        match ums_device.force_reset() {
                            Err(e) => log::warn!("USB reset on resume failed: {:?}", e),
                            _ => (),
                        };
                    }
                    Views::HIDv2 => {
                        match hidv2.force_reset() {
                            Err(e) => log::warn!("USB reset on resume failed: {:?}", e),
                            _ => (),
                        };
                    }
                    Views::Serial => match serial_device.force_reset() {
                        Err(e) => log::warn!("USB reset on resume failed: {:?}", e),
                        _ => (),
                    },
                }
                // resume2 brings us to our last application state
                usbmgmt.xous_resume2();
                lockstatus_force_update = true; // notify the status bar that yes, it does need to redraw the lock status, even if the value hasn't changed since the last read
            }),
            Some(Opcode::IsSocCompatible) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, 1).expect("couldn't return compatibility status")
            }),
            Some(Opcode::U2fRxDeferred) => {
                // notify the event listener, if any
                if observer_conn.is_some() && observer_op.is_some() {
                    xous::try_send_message(
                        observer_conn.unwrap(),
                        xous::Message::new_scalar(observer_op.unwrap(), 0, 0, 0, 0),
                    )
                    .ok();
                }

                if fido_listener_pid.is_none() {
                    fido_listener_pid = msg.sender.pid();
                }
                if fido_listener.is_some() {
                    log::error!(
                        "Double-listener request detected. There should only ever by one registered listener at a time."
                    );
                    log::error!(
                        "This will cause an upstream server to misbehave, but not panicing so the problem can be debugged."
                    );
                    // the receiver will get a response with the `code` field still in the `RxWait` state to
                    // indicate the problem
                }
                if fido_listener_pid == msg.sender.pid() {
                    // preferentially pull from the rx queue if it has elements
                    if let Some(data) = fido_rx_queue.pop_front() {
                        log::debug!(
                            "no deferral: ret queued data: {:?} queue len: {}",
                            &data[..8],
                            fido_rx_queue.len() + 1
                        );
                        let mut response = unsafe {
                            Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                        };
                        let mut buf = response.to_original::<U2fMsgIpc, _>().unwrap();
                        assert_eq!(buf.code, U2fCode::RxWait, "Expected U2fcode::RxWait in wrapper");
                        buf.data.copy_from_slice(&data);
                        buf.code = U2fCode::RxAck;
                        response.replace(buf).unwrap();
                    } else {
                        log::trace!("registering deferred listener");
                        {
                            // not tested
                            let spec = unsafe {
                                Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                            };
                            if let Some(to) = spec.to_original::<U2fMsgIpc, _>().unwrap().timeout_ms {
                                let target = tt.elapsed_ms() + to;
                                target_time_lsb.store(target as u32, Ordering::SeqCst); // this will keep updating the target time later and later
                                // run must always be set *after* target time is updated, because there is
                                // always a chance we timed out and checked
                                // target time between these two steps
                                to_run.store(true, Ordering::SeqCst);
                                xous::try_send_message(
                                    to_conn,
                                    xous::Message::new_scalar(
                                        TimeoutOp::Pump.to_usize().unwrap(),
                                        0,
                                        0,
                                        0,
                                        0,
                                    ),
                                )
                                .ok();
                            }
                        };
                        fido_listener = Some(msg);
                    }
                } else {
                    log::warn!(
                        "U2F interface capability is locked on first use; additional servers are ignored: {:?}",
                        msg.sender
                    );
                    let mut buffer =
                        unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                    let mut u2f_ipc = buffer.to_original::<U2fMsgIpc, _>().unwrap();
                    u2f_ipc.code = U2fCode::Denied;
                    buffer.replace(u2f_ipc).unwrap();
                }
            }
            // Note: not tested
            Some(Opcode::U2fRxTimeout) => {
                if let Some(mut listener) = fido_listener.take() {
                    let mut response = unsafe {
                        Buffer::from_memory_message_mut(listener.body.memory_message_mut().unwrap())
                    };
                    let mut buf = response.to_original::<U2fMsgIpc, _>().unwrap();
                    assert_eq!(buf.code, U2fCode::RxWait, "Expected U2fcode::RxWait in wrapper");
                    buf.code = U2fCode::RxTimeout;
                    response.replace(buf).unwrap();
                }
            }
            Some(Opcode::U2fTx) => {
                if fido_listener_pid.is_none() {
                    fido_listener_pid = msg.sender.pid();
                }
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut u2f_ipc = buffer.to_original::<U2fMsgIpc, _>().unwrap();
                if fido_listener_pid == msg.sender.pid() {
                    let mut u2f_msg = RawFidoReport::default();
                    assert_eq!(u2f_ipc.code, U2fCode::Tx, "Expected U2fCode::Tx in wrapper");
                    u2f_msg.packet.copy_from_slice(&u2f_ipc.data);
                    let u2f = match view {
                        Views::FidoWithKbd => composite.device::<RawFido<'_, _>, _>(),
                        Views::FidoOnly => fido_class.device::<RawFido<'_, _>, _>(),
                        #[cfg(feature = "mass-storage")]
                        Views::MassStorage => panic!("did not expect u2f tx when in mass storage mode!"),
                        Views::Serial => panic!("did not expect u2f tx while in serial mode!"),
                        Views::HIDv2 => panic!("did not expect u2f tx while in hidv2 mode!"),
                    };
                    u2f.write_report(&u2f_msg).ok();
                    log::debug!("sent U2F packet {:x?}", u2f_ipc.data);
                    u2f_ipc.code = U2fCode::TxAck;
                } else {
                    u2f_ipc.code = U2fCode::Denied;
                }
                buffer.replace(u2f_ipc).unwrap();
            }
            Some(Opcode::UsbIrqHandler) => {
                let maybe_u2f = match view {
                    Views::FidoWithKbd => {
                        if usb_dev.poll(&mut [&mut composite]) {
                            match composite.device::<NKROBootKeyboard<_>, _>().read_report() {
                                Ok(l) => {
                                    log::info!("keyboard LEDs: {:?}", l);
                                    led_state = l;
                                }
                                Err(e) => log::trace!("KEYB ERR: {:?}", e),
                            }
                            Some(composite.device::<RawFido<'_, _>, _>())
                        } else {
                            None
                        }
                    }
                    Views::FidoOnly => {
                        if fido_dev.poll(&mut [&mut fido_class]) {
                            Some(fido_class.device::<RawFido<'_, _>, _>())
                        } else {
                            None
                        }
                    }
                    #[cfg(feature = "mass-storage")]
                    Views::MassStorage => {
                        if ums_device.poll(&mut [&mut ums]) {
                            log::debug!("ums device had something to do!")
                        }
                        None
                    }
                    Views::Serial => {
                        if serial_device.poll(&mut [&mut serial_port]) {
                            let mut data: [u8; 1024] = [0u8; SERIAL_BUF_LEN];
                            match serial_listen_mode {
                                SerialListenMode::NoListener => match serial_port.read(&mut data) {
                                    Ok(len) => match std::str::from_utf8(&data[..len]) {
                                        Ok(s) => log::debug!("No listener ascii: {}", s),
                                        Err(_) => {
                                            log::debug!("No listener binary: {:x?}", &data[..len]);
                                        }
                                    },
                                    Err(e) => {
                                        log::debug!("No listener: {:?}", e);
                                    }
                                },
                                SerialListenMode::ConsoleListener => match serial_port.read(&mut data) {
                                    Ok(len) => match std::str::from_utf8(&data[..len]) {
                                        Ok(s) => {
                                            for c in s.chars() {
                                                native_kbd.inject_key(c);
                                            }
                                        }
                                        Err(_) => {
                                            log::info!("Non UTF-8 received on console: {:x?}", &data[..len]);
                                        }
                                    },
                                    Err(e) => {
                                        log::info!("Serial read error: {:?}", e);
                                    }
                                },
                                SerialListenMode::AsciiListener(maybe_delimiter) => {
                                    let readlen = serial_port.read(&mut data).unwrap_or(0);
                                    if readlen == 0 {
                                        continue;
                                    }
                                    if let Some(delimiter) = maybe_delimiter {
                                        if !delimiter.is_ascii() {
                                            log::warn!(
                                                "Chosen ASCII delimiter {} is not ASCII. Serial receive will not function properly.",
                                                delimiter
                                            );
                                        }
                                        if !serial_rx_trigger {
                                            // once true, sticks as true
                                            serial_rx_trigger = data[..readlen]
                                                .iter()
                                                .find(|&&c| c == (delimiter as u8))
                                                .is_some();
                                        }
                                    } else {
                                        serial_rx_trigger = true;
                                    }
                                    // append the incoming data to the main buffer
                                    for &d in &data[..readlen] {
                                        serial_buf.push(d);
                                    }
                                    // now see if we should pass it back to the listener (if it is hooked)
                                    if serial_rx_trigger && serial_listener.is_some() {
                                        let mut rx_msg = serial_listener.take().unwrap();
                                        let mut response = unsafe {
                                            Buffer::from_memory_message_mut(
                                                rx_msg.body.memory_message_mut().unwrap(),
                                            )
                                        };
                                        let mut buf = response.to_original::<UsbSerialAscii, _>().unwrap();
                                        use std::fmt::Write; // is this really the best way to do it? probably not.
                                        write!(
                                            buf.s,
                                            "{}",
                                            std::string::String::from_utf8_lossy(&serial_buf)
                                        )
                                        .ok();

                                        response.replace(buf).unwrap();
                                        // the rx_msg will drop and respond to the listener
                                        serial_rx_trigger = false;
                                    }
                                }
                                SerialListenMode::BinaryListener => {
                                    let readlen = serial_port.read(&mut data).unwrap_or(0);
                                    if readlen == 0 {
                                        continue;
                                    }
                                    // append the incoming data to the main buffer
                                    for &d in &data[..readlen] {
                                        serial_buf.push(d);
                                    }
                                    if serial_buf.len() >= SERIAL_BINARY_BUFLEN {
                                        match serial_listener.take() {
                                            Some(mut rx_msg) => {
                                                let mut response = unsafe {
                                                    Buffer::from_memory_message_mut(
                                                        rx_msg.body.memory_message_mut().unwrap(),
                                                    )
                                                };
                                                let mut buf =
                                                    response.to_original::<UsbSerialBinary, _>().unwrap();
                                                buf.d.copy_from_slice(
                                                    serial_buf.drain(..SERIAL_BINARY_BUFLEN).as_slice(),
                                                );
                                                buf.len = SERIAL_BINARY_BUFLEN;
                                                response.replace(buf).unwrap();
                                                // the rx_msg will drop and respond to the listener
                                            }
                                            None => {
                                                // do nothing, keep queuing data...
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        None
                    }
                    Views::HIDv2 => {
                        match hidv2.poll() {
                            Ok(_) => (),
                            Err(error) => log::error!("hidv2 poll error: {:?}", error),
                        }

                        None
                    }
                };
                if let Some(u2f) = maybe_u2f {
                    match u2f.read_report() {
                        Ok(u2f_report) => {
                            if let Some(mut listener) = fido_listener.take() {
                                to_run.store(false, Ordering::SeqCst); // stop the timeout process from running
                                let mut response = unsafe {
                                    Buffer::from_memory_message_mut(
                                        listener.body.memory_message_mut().unwrap(),
                                    )
                                };
                                let mut buf = response.to_original::<U2fMsgIpc, _>().unwrap();
                                assert_eq!(buf.code, U2fCode::RxWait, "Expected U2fcode::RxWait in wrapper");
                                buf.data.copy_from_slice(&u2f_report.packet);
                                log::trace!("ret deferred data {:x?}", &u2f_report.packet[..8]);
                                buf.code = U2fCode::RxAck;
                                response.replace(buf).unwrap();
                            } else {
                                log::debug!("Got U2F packet, but no server to respond...queuing.");
                                fido_rx_queue.push_back(u2f_report.packet);
                            }
                        }
                        Err(e) => log::trace!("U2F ERR: {:?}", e),
                    }
                }

                let is_suspend = match view {
                    Views::FidoWithKbd => usb_dev.state() == UsbDeviceState::Suspend,
                    Views::FidoOnly => fido_dev.state() == UsbDeviceState::Suspend,
                    #[cfg(feature = "mass-storage")]
                    Views::MassStorage => ums_device.state() == UsbDeviceState::Suspend,
                    Views::Serial => serial_device.state() == UsbDeviceState::Suspend,
                    Views::HIDv2 => hidv2.state() == UsbDeviceState::Suspend,
                };
                if is_suspend {
                    log::info!("suspend detected");
                    if was_suspend == false {
                        // FIDO listener needs to know when USB was unplugged, so that it can reset state per
                        // FIDO2 spec
                        if let Some(mut listener) = fido_listener.take() {
                            to_run.store(false, Ordering::SeqCst);
                            let mut response = unsafe {
                                Buffer::from_memory_message_mut(listener.body.memory_message_mut().unwrap())
                            };
                            let mut buf = response.to_original::<U2fMsgIpc, _>().unwrap();
                            assert_eq!(buf.code, U2fCode::RxWait, "Expected U2fcode::RxWait in wrapper");
                            buf.code = U2fCode::Hangup;
                            response.replace(buf).unwrap();
                        }
                    }
                    was_suspend = true;
                } else {
                    was_suspend = false;
                }
            }
            // always triggers a reset when called
            Some(Opcode::SwitchCores) => msg_blocking_scalar_unpack!(msg, core, _, _, _, {
                // ensure unhook the logger if it's connected to serial
                let log_conn = xous::connect(xous::SID::from_bytes(b"xous-log-server ").unwrap()).unwrap();
                // it is never harmful to double-unhook this
                xous::send_message(
                    log_conn,
                    xous::Message::new_blocking_scalar(
                        log_server::api::Opcode::UnhookUsbMirror.to_usize().unwrap(),
                        0,
                        0,
                        0,
                        0,
                    ),
                )
                .ok();
                // reset any serial listeners that may have been set
                serial_listen_mode = SerialListenMode::NoListener;
                serial_listener.take();
                // shut down the TRNG sender if it's set
                if let Some(trng_cid) = serial_trng_cid.take() {
                    serial_trng_interval.store(0, Ordering::SeqCst);
                    xous::send_message(
                        trng_cid,
                        xous::Message::new_blocking_scalar(TrngOp::Quit.to_usize().unwrap(), 0, 0, 0, 0),
                    )
                    .ok();
                    trng.set_test_mode(trng::api::TrngTestMode::None);
                }

                let devtype: UsbDeviceType = core.try_into().unwrap();
                match devtype {
                    UsbDeviceType::Debug => {
                        log::info!("Connecting debug core; disconnecting USB device core");
                        usbmgmt.connect_device_core(false);
                    }
                    UsbDeviceType::FidoKbd => {
                        log::info!("Connecting device core FIDO + kbd; disconnecting debug USB core");
                        match view {
                            Views::FidoWithKbd => usbmgmt.connect_device_core(true),
                            _ => {
                                view = Views::FidoWithKbd;
                                usbmgmt.ll_reset(true);
                                tt.sleep_ms(1000).ok();
                                usbmgmt.ll_connect_device_core(true);
                                tt.sleep_ms(EXTENDED_CORE_RESET_MS).ok();
                                usbmgmt.ll_reset(false);
                            }
                        }
                        let keyboard = composite.device::<NKROBootKeyboard<'_, _>, _>();
                        keyboard.write_report([Keyboard::NoEventIndicated]).ok(); // queues an "all key-up" for the interface
                        keyboard.tick().ok();
                    }
                    UsbDeviceType::Fido => {
                        log::info!("Connecting device core FIDO only; disconnecting debug USB core");
                        match view {
                            Views::FidoOnly => usbmgmt.connect_device_core(true),
                            _ => {
                                view = Views::FidoOnly;
                                usbmgmt.ll_reset(true);
                                tt.sleep_ms(1000).ok();
                                usbmgmt.ll_connect_device_core(true);
                                tt.sleep_ms(EXTENDED_CORE_RESET_MS).ok();
                                usbmgmt.ll_reset(false);
                            }
                        }
                    }
                    #[cfg(feature = "mass-storage")]
                    UsbDeviceType::MassStorage => {
                        log::info!("Connecting device mass storage; disconnecting debug USB core");
                        match view {
                            Views::MassStorage => usbmgmt.connect_device_core(true),
                            _ => {
                                view = Views::MassStorage;
                                usbmgmt.ll_reset(true);
                                tt.sleep_ms(1000).ok();
                                usbmgmt.ll_connect_device_core(true);
                                tt.sleep_ms(EXTENDED_CORE_RESET_MS).ok();
                                usbmgmt.ll_reset(false);
                            }
                        }
                    }
                    UsbDeviceType::Serial => {
                        log::info!("Connecting device serial");
                        match view {
                            Views::Serial => usbmgmt.connect_device_core(true),
                            _ => {
                                view = Views::Serial;
                                usbmgmt.ll_reset(true);
                                tt.sleep_ms(1000).ok();
                                usbmgmt.ll_connect_device_core(true);
                                tt.sleep_ms(EXTENDED_CORE_RESET_MS).ok();
                                usbmgmt.ll_reset(false);
                            }
                        }
                    }
                    UsbDeviceType::HIDv2 => {
                        log::info!("Connectiing HIDv2 device");
                        match view {
                            Views::HIDv2 => usbmgmt.connect_device_core(true),
                            _ => {
                                view = Views::HIDv2;
                                usbmgmt.ll_reset(true);
                                tt.sleep_ms(1000).ok();
                                usbmgmt.ll_connect_device_core(true);
                                tt.sleep_ms(EXTENDED_CORE_RESET_MS).ok();
                                usbmgmt.ll_reset(false);
                            }
                        }
                    }
                }
                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            // does not trigger a reset if we're already on the core
            Some(Opcode::EnsureCore) => msg_blocking_scalar_unpack!(msg, core, _, _, _, {
                let devtype: UsbDeviceType = core.try_into().unwrap();
                // if we are switching away from serial, unhook any possible listeners, and the logger
                if view == Views::Serial && devtype != UsbDeviceType::Serial {
                    let log_conn =
                        xous::connect(xous::SID::from_bytes(b"xous-log-server ").unwrap()).unwrap();
                    // it is never harmful to double-unhook this
                    xous::send_message(
                        log_conn,
                        xous::Message::new_blocking_scalar(
                            log_server::api::Opcode::UnhookUsbMirror.to_usize().unwrap(),
                            0,
                            0,
                            0,
                            0,
                        ),
                    )
                    .ok();
                    // reset any serial listeners that may have been set
                    serial_listen_mode = SerialListenMode::NoListener;
                    serial_listener.take();
                    // shut down the TRNG sender if it's set
                    if let Some(trng_cid) = serial_trng_cid.take() {
                        serial_trng_interval.store(0, Ordering::SeqCst);
                        xous::send_message(
                            trng_cid,
                            xous::Message::new_blocking_scalar(TrngOp::Quit.to_usize().unwrap(), 0, 0, 0, 0),
                        )
                        .ok();
                        trng.set_test_mode(trng::api::TrngTestMode::None);
                    }
                }

                match devtype {
                    UsbDeviceType::Debug => {
                        if usbmgmt.is_device_connected() {
                            log::info!("Connecting debug core; disconnecting USB device core");
                            usbmgmt.connect_device_core(false);
                        }
                    }
                    UsbDeviceType::FidoKbd => {
                        if !usbmgmt.is_device_connected() {
                            log::info!("Ensuring FIDO + kbd device");
                            view = Views::FidoWithKbd;
                            usbmgmt.connect_device_core(true);
                        } else {
                            if view != Views::FidoWithKbd {
                                view = Views::FidoWithKbd;
                                usbmgmt.ll_reset(true);
                                tt.sleep_ms(1000).ok();
                                usbmgmt.ll_connect_device_core(true);
                                tt.sleep_ms(EXTENDED_CORE_RESET_MS).ok();
                                usbmgmt.ll_reset(false);
                            } else {
                                // type matches, do nothing
                            }
                        }
                        let keyboard = composite.device::<NKROBootKeyboard<'_, _>, _>();
                        keyboard.write_report([Keyboard::NoEventIndicated]).ok(); // queues an "all key-up" for the interface
                        keyboard.tick().ok();
                    }
                    UsbDeviceType::Fido => {
                        if !usbmgmt.is_device_connected() {
                            log::info!("Ensuring FIDO only device");
                            view = Views::FidoOnly;
                            usbmgmt.connect_device_core(true);
                        } else {
                            if view != Views::FidoOnly {
                                view = Views::FidoOnly;
                                usbmgmt.ll_reset(true);
                                tt.sleep_ms(1000).ok();
                                usbmgmt.ll_connect_device_core(true);
                                tt.sleep_ms(EXTENDED_CORE_RESET_MS).ok();
                                usbmgmt.ll_reset(false);
                            } else {
                                // type matches, do nothing
                            }
                        }
                    }
                    #[cfg(feature = "mass-storage")]
                    UsbDeviceType::MassStorage => {
                        log::info!("Ensuring mass storage device");
                        if !usbmgmt.is_device_connected() {
                            view = Views::MassStorage;
                            usbmgmt.connect_device_core(true);
                        } else {
                            if view != Views::MassStorage {
                                view = Views::MassStorage;
                                usbmgmt.ll_reset(true);
                                tt.sleep_ms(1000).ok();
                                usbmgmt.ll_connect_device_core(true);
                                tt.sleep_ms(EXTENDED_CORE_RESET_MS).ok();
                                usbmgmt.ll_reset(false);
                            } else {
                                // type matches, do nothing
                            }
                        }
                    }
                    UsbDeviceType::Serial => {
                        log::info!("Ensuring serial device");
                        if !usbmgmt.is_device_connected() {
                            view = Views::Serial;
                            usbmgmt.connect_device_core(true);
                        } else {
                            if view != Views::Serial {
                                view = Views::Serial;
                                usbmgmt.ll_reset(true);
                                tt.sleep_ms(1000).ok();
                                usbmgmt.ll_connect_device_core(true);
                                tt.sleep_ms(EXTENDED_CORE_RESET_MS).ok();
                                usbmgmt.ll_reset(false);
                            }
                        }
                    }
                    UsbDeviceType::HIDv2 => {
                        log::info!("Ensuring HIDv2 device");
                        if !usbmgmt.is_device_connected() {
                            view = Views::HIDv2;
                            usbmgmt.connect_device_core(true);
                        } else {
                            if view != Views::HIDv2 {
                                view = Views::HIDv2;
                                usbmgmt.ll_reset(true);
                                tt.sleep_ms(1000).ok();
                                usbmgmt.ll_connect_device_core(true);
                                tt.sleep_ms(EXTENDED_CORE_RESET_MS).ok();
                                usbmgmt.ll_reset(false);
                            }
                        }
                    }
                }
                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            Some(Opcode::WhichCore) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if usbmgmt.is_device_connected() {
                    match view {
                        Views::FidoWithKbd => {
                            xous::return_scalar(msg.sender, UsbDeviceType::FidoKbd as usize).unwrap()
                        }
                        Views::FidoOnly => {
                            xous::return_scalar(msg.sender, UsbDeviceType::Fido as usize).unwrap()
                        }
                        #[cfg(feature = "mass-storage")]
                        Views::MassStorage => {
                            xous::return_scalar(msg.sender, UsbDeviceType::MassStorage as usize).unwrap()
                        }
                        Views::Serial => {
                            xous::return_scalar(msg.sender, UsbDeviceType::Serial as usize).unwrap()
                        }
                        Views::HIDv2 => {
                            xous::return_scalar(msg.sender, UsbDeviceType::HIDv2 as usize).unwrap()
                        }
                    }
                } else {
                    xous::return_scalar(msg.sender, UsbDeviceType::Debug as usize).unwrap();
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
                // at this point, *read back* the new state -- don't assume it "took". The readback is always
                // based on a real hardware value and not the requested value. for now, always
                // false.
                let is_locked = if usbmgmt.get_disable_debug() { 1 } else { 0 };

                // this is a performance optimization. we could always redraw the status, but, instead we only
                // redraw when the status has changed. However, there is an edge case: on a
                // resume from suspend, the status needs a redraw, even if nothing has
                // changed. Thus, we have this separate boolean we send back to force an update in the
                // case that we have just come out of a suspend.
                let force_update = if lockstatus_force_update { 1 } else { 0 };
                xous::return_scalar2(msg.sender, is_locked, force_update).expect("couldn't return status");
                lockstatus_force_update = false;
            }),
            Some(Opcode::LinkStatus) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                match view {
                    Views::FidoWithKbd => xous::return_scalar(msg.sender, usb_dev.state() as usize).unwrap(),
                    Views::FidoOnly => xous::return_scalar(msg.sender, fido_dev.state() as usize).unwrap(),
                    #[cfg(feature = "mass-storage")]
                    Views::MassStorage => {
                        xous::return_scalar(msg.sender, ums_device.state() as usize).unwrap()
                    }
                    Views::Serial => xous::return_scalar(msg.sender, serial_device.state() as usize).unwrap(),
                    Views::HIDv2 => xous::return_scalar(msg.sender, hidv2.state() as usize).unwrap(),
                }
            }),
            Some(Opcode::SendKeyCode) => msg_blocking_scalar_unpack!(msg, code0, code1, code2, autoup, {
                match view {
                    Views::FidoWithKbd => {
                        if usb_dev.state() == UsbDeviceState::Configured {
                            let native_map = native_kbd.get_keymap().unwrap();
                            let mut codes = Vec::<Keyboard>::new();
                            if code0 != 0 {
                                codes.push(match native_map {
                                    KeyMap::Dvorak => {
                                        mappings::char_to_hid_code_dvorak(code0 as u8 as char)[0]
                                    }
                                    _ => mappings::char_to_hid_code_us101(code0 as u8 as char)[0],
                                });
                            }
                            if code1 != 0 {
                                codes.push(match native_map {
                                    KeyMap::Dvorak => {
                                        mappings::char_to_hid_code_dvorak(code1 as u8 as char)[0]
                                    }
                                    _ => mappings::char_to_hid_code_us101(code1 as u8 as char)[0],
                                });
                            }
                            if code2 != 0 {
                                codes.push(match native_map {
                                    KeyMap::Dvorak => {
                                        mappings::char_to_hid_code_dvorak(code2 as u8 as char)[0]
                                    }
                                    _ => mappings::char_to_hid_code_us101(code2 as u8 as char)[0],
                                });
                            }
                            let auto_up = if autoup == 1 { true } else { false };
                            let keyboard = composite.device::<NKROBootKeyboard<'_, _>, _>();
                            keyboard.write_report(codes).ok();
                            keyboard.tick().ok();
                            tt.sleep_ms(autotype_delay_ms).ok();
                            if auto_up {
                                keyboard.write_report([Keyboard::NoEventIndicated]).ok(); // this is the key-up
                                keyboard.tick().ok();
                                tt.sleep_ms(autotype_delay_ms).ok();
                            }
                            xous::return_scalar(msg.sender, 0).unwrap();
                        } else {
                            xous::return_scalar(msg.sender, 1).unwrap();
                        }
                    }
                    _ => {
                        xous::return_scalar(msg.sender, 1).unwrap();
                    }
                }
            }),
            Some(Opcode::LogString) => {
                // the logger API is "best effort" only. Because retries and response codes can cause problems
                // in the logger API, if anything goes wrong, we prefer to discard characters rather than get
                // the whole subsystem stuck in some awful recursive error handling hell.
                match view {
                    Views::Serial => {
                        let buffer =
                            unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let usb_send = buffer.to_original::<api::UsbString, _>().unwrap();
                        // this is implemented as a "blocking write": the routine will block until the data
                        // has all been written.
                        let send_data = usb_send.s.as_bytes();
                        let to_send = usb_send.s.len();
                        let mut sent = 0;
                        while sent < to_send {
                            match serial_port.write(&send_data[sent..to_send]) {
                                Ok(written) => {
                                    sent += written;
                                }
                                Err(_) => {
                                    // just drop characters
                                }
                            }
                            match serial_port.flush() {
                                Ok(_) => {}
                                Err(_) => {
                                    // just drop characters
                                }
                            }
                        }
                    }
                    _ => {} // do nothing; don't fail, don't report any error.
                }
            }
            Some(Opcode::SetAutotypeRate) => msg_scalar_unpack!(msg, rate, _, _, _, {
                // limit rate to 0.5s delay. Even then, this will probably cause repeated characters because
                // it also adjusts keydown delays
                let checked_rate = if rate > 500 { 500 } else { rate };
                // there is no limit on the minimum rate. good luck if you set it to 0!
                autotype_delay_ms = checked_rate;
            }),
            Some(Opcode::SendString) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut usb_send = buffer.to_original::<api::UsbString, _>().unwrap();
                #[cfg(not(feature = "minimal"))]
                let mut sent = 0;
                #[cfg(feature = "minimal")]
                let sent = 0;
                match view {
                    #[cfg(not(feature = "minimal"))]
                    Views::FidoWithKbd => {
                        // check keymap on every call because we may need to toggle this for e.g. plugging
                        // into a new host with a different map
                        let native_map = native_kbd.get_keymap().unwrap();
                        for ch in usb_send.s.as_str().unwrap().chars() {
                            // ASSUME: user's keyboard type matches the preference on their Precursor device.
                            let codes = match native_map {
                                KeyMap::Dvorak => mappings::char_to_hid_code_dvorak(ch),
                                _ => mappings::char_to_hid_code_us101(ch),
                            };
                            let keyboard = composite.device::<NKROBootKeyboard<'_, _>, _>();
                            keyboard.write_report(codes).ok();
                            keyboard.tick().ok();
                            tt.sleep_ms(autotype_delay_ms).ok();
                            keyboard.write_report([Keyboard::NoEventIndicated]).ok(); // this is the key-up
                            keyboard.tick().ok();
                            tt.sleep_ms(autotype_delay_ms).ok();
                            sent += 1;
                        }
                    }
                    Views::Serial => {
                        // this is implemented as a "blocking write": the routine will block until the data
                        // has all been written.
                        let send_data = usb_send.s.as_bytes();
                        let to_send = usb_send.s.len();
                        // log::debug!("serial RTS: {:?}", serial_port.rts());
                        // log::debug!("serial DTR: {:?}", serial_port.dtr());
                        while sent < to_send {
                            match serial_port.write(&send_data[sent..to_send]) {
                                Ok(written) => {
                                    sent += written;
                                }
                                Err(_) => {
                                    log::warn!("Serial send is blocking. Delaying and trying again.");
                                    tt.sleep_ms(100).ok();
                                }
                            }
                            match serial_port.flush() {
                                Ok(_) => {}
                                Err(_) => {
                                    log::warn!("Serial port reported WouldBlock on flush");
                                    tt.sleep_ms(100).ok();
                                }
                            }
                        }
                    }
                    _ => {} // do nothing; will report that 0 characters were sent
                }
                usb_send.sent = Some(sent as _);
                buffer.replace(usb_send).unwrap();
            }
            Some(Opcode::GetLedState) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let mut code = [0u8; 1];
                led_state.pack_to_slice(&mut code).unwrap();
                xous::return_scalar(msg.sender, code[0] as usize).unwrap();
            }),
            Some(Opcode::SerialHookAscii) => {
                let maybe_delimiter = {
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let data = buffer.to_original::<UsbSerialAscii, _>().unwrap();
                    data.delimiter
                };
                serial_listen_mode = SerialListenMode::AsciiListener(maybe_delimiter);
                serial_listener = Some(msg);
            }
            Some(Opcode::SerialHookBinary) => {
                serial_listen_mode = SerialListenMode::BinaryListener;
                serial_listener = Some(msg);
            }
            Some(Opcode::SerialHookConsole) => msg_scalar_unpack!(msg, _, _, _, _, {
                let log_conn = xous::connect(xous::SID::from_bytes(b"xous-log-server ").unwrap()).unwrap();
                match xous::send_message(
                    log_conn,
                    xous::Message::new_blocking_scalar(
                        log_server::api::Opcode::TryHookUsbMirror.to_usize().unwrap(),
                        0,
                        0,
                        0,
                        0,
                    ),
                ) {
                    Ok(xous::Result::Scalar1(result)) => {
                        if result == 1 {
                            serial_listen_mode = SerialListenMode::ConsoleListener;
                            // unhook any previous pending listener
                            serial_listener.take();
                        } else {
                            log::error!("Error trying to connect USB console.");
                        }
                    }
                    _ => {
                        log::error!("Could not connect USB console");
                    }
                }
            }),
            Some(Opcode::SerialClearHooks) => {
                let log_conn = xous::connect(xous::SID::from_bytes(b"xous-log-server ").unwrap()).unwrap();
                // it is never harmful to double-unhook this
                xous::send_message(
                    log_conn,
                    xous::Message::new_blocking_scalar(
                        log_server::api::Opcode::UnhookUsbMirror.to_usize().unwrap(),
                        0,
                        0,
                        0,
                        0,
                    ),
                )
                .ok();

                serial_listen_mode = SerialListenMode::NoListener;
                serial_listener.take();
                // shut down the TRNG sender if it's set
                if let Some(trng_cid) = serial_trng_cid.take() {
                    serial_trng_interval.store(0, Ordering::SeqCst);
                    xous::send_message(
                        trng_cid,
                        xous::Message::new_blocking_scalar(TrngOp::Quit.to_usize().unwrap(), 0, 0, 0, 0),
                    )
                    .ok();
                    trng.set_test_mode(trng::api::TrngTestMode::None);
                }
            }
            Some(Opcode::SerialFlush) => msg_scalar_unpack!(msg, _, _, _, _, {
                // this will hardware flush any pending items in usb_serial driver
                serial_port.flush().ok();
                // this tries to return any data that's pending within the main loop's buffers
                match serial_listen_mode {
                    SerialListenMode::BinaryListener => {
                        match serial_listener.take() {
                            Some(mut rx_msg) => {
                                let mut response = unsafe {
                                    Buffer::from_memory_message_mut(rx_msg.body.memory_message_mut().unwrap())
                                };
                                let mut buf = response.to_original::<UsbSerialBinary, _>().unwrap();
                                let chars_avail = serial_buf.len().min(SERIAL_BINARY_BUFLEN);
                                buf.len = chars_avail;
                                buf.d.copy_from_slice(serial_buf.drain(..chars_avail).as_slice());
                                response.replace(buf).unwrap();
                                // the rx_msg will drop and respond to the listener
                            }
                            None => {
                                // do nothing, keep queuing data...
                            }
                        }
                    }
                    SerialListenMode::AsciiListener(_) => {
                        match serial_listener.take() {
                            Some(mut rx_msg) => {
                                let mut response = unsafe {
                                    Buffer::from_memory_message_mut(rx_msg.body.memory_message_mut().unwrap())
                                };
                                let mut buf = response.to_original::<UsbSerialAscii, _>().unwrap();
                                use std::fmt::Write; // is this really the best way to do it? probably not.
                                write!(buf.s, "{}", std::string::String::from_utf8_lossy(&serial_buf)).ok();

                                response.replace(buf).unwrap();
                                // the rx_msg will drop and respond to the listener
                                serial_rx_trigger = false;
                            }
                            None => {} // do nothing
                        }
                    }
                    _ => {}
                }
            }),
            Some(Opcode::SerialHookTrngSender) => msg_scalar_unpack!(msg, trng_mode_code, _, _, _, {
                if view != Views::Serial {
                    log::error!("USB is not in serial mode. Ignoring request to hook TRNG sender");
                    continue;
                }
                match serial_listen_mode {
                    SerialListenMode::ConsoleListener => {
                        log::error!(
                            "Serial is already hooked as a console. Refusing to turn on TRNG source mode"
                        );
                        continue;
                    }
                    SerialListenMode::NoListener => {}
                    _ => {
                        log::warn!(
                            "Serial already has a listener {:?} attached, hooking TRNG at your risk!",
                            serial_listen_mode
                        );
                    }
                }

                let trng_mode: trng::api::TrngTestMode =
                    num_traits::FromPrimitive::from_usize(trng_mode_code)
                        .unwrap_or(trng::api::TrngTestMode::None);
                if trng_mode == trng::api::TrngTestMode::None {
                    // ignore the call in case of a bad parameter
                    continue;
                } else {
                    trng.set_test_mode(trng_mode);
                }
                log::info!("TRNG set to mode {:?}", trng_mode);

                // The strategy here is when this is called, we start a thread that polls at some
                // interval in milliseconds to see if the Tx buffer is empty and
                // if rts() is true; and if so, jams more binary data into the pipe. If rts() is not true
                // or the buffer is not available to write, the interval backs off.
                //
                // It also takes a trng_mode argument which is used to select either CPRNG whitened output
                // (which is the default mode), or direct raw data from the RO or the Avalanche generator;
                // this is used to help characterize the raw entropy sources.
                if serial_trng_cid.is_none() {
                    let trng_sid = xous::create_server().unwrap();
                    let trng_cid = xous::connect(trng_sid).unwrap();
                    std::thread::spawn({
                        let serial_trng_interval = serial_trng_interval.clone();
                        let main_conn = cid.clone();
                        let trng_cid = trng_cid.clone();
                        move || {
                            let tt = ticktimer_server::Ticktimer::new().unwrap();
                            let mut msg_opt = None;
                            let mut return_type = 0;
                            serial_trng_interval.store(TRNG_INITIAL_DELAY_MS, Ordering::SeqCst);
                            log::info!("TRNG polling loop started");
                            let mut debug_count = 0;
                            loop {
                                xous::reply_and_receive_next_legacy(trng_sid, &mut msg_opt, &mut return_type)
                                    .unwrap();
                                let msg = msg_opt.as_mut().unwrap();
                                match num_traits::FromPrimitive::from_usize(msg.body.id())
                                    .unwrap_or(TrngOp::InvalidCall)
                                {
                                    TrngOp::Pump => {
                                        let next_interval = serial_trng_interval.load(Ordering::SeqCst);
                                        if debug_count < 20 && next_interval < 100 {
                                            log::debug!("TRNG serial poller, delay = {}", next_interval);
                                            debug_count += 1;
                                        }
                                        if next_interval > 0 {
                                            xous::try_send_message(
                                                main_conn,
                                                xous::Message::new_scalar(
                                                    Opcode::SerialTrngPoll.to_usize().unwrap(),
                                                    0,
                                                    0,
                                                    0,
                                                    0,
                                                ),
                                            )
                                            .ok();
                                            tt.sleep_ms(next_interval as _).ok();
                                            xous::try_send_message(
                                                trng_cid,
                                                xous::Message::new_scalar(
                                                    TrngOp::Pump.to_usize().unwrap(),
                                                    0,
                                                    0,
                                                    0,
                                                    0,
                                                ),
                                            )
                                            .ok();
                                            // reset debug_count so when the next trigger comes we can see the
                                            // output
                                            if next_interval > 100 {
                                                debug_count = 0;
                                            }
                                        } else {
                                            debug_count = 0;
                                        }
                                    }
                                    TrngOp::Quit => {
                                        if let Some(scalar) = msg.body.scalar_message_mut() {
                                            scalar.id = 0;
                                            scalar.arg1 = 1;
                                            log::info!("Quit called to Trng helper thread");
                                            // I think there might be a bug in the kernel where
                                            // quitting/disconnecting
                                            // a reply_and_receive_next_legacy() loop is broken?
                                            // This results in an inexplicable kernel hang...
                                            // for now we can work around this by keeping the thread around
                                            // once the server is started.
                                            // break;
                                        }
                                    }
                                    TrngOp::InvalidCall => {
                                        log::error!(
                                            "Unknown opcode received in TRNG source handler: {:?}",
                                            msg.body.id()
                                        );
                                    }
                                }
                            }
                            /*
                            unsafe{xous::disconnect(trng_cid).ok()};
                            xous::destroy_server(trng_sid).ok();
                            log::info!("TRNG polling loop exited");
                            */
                        }
                    });
                    serial_trng_cid = Some(trng_cid);
                }
                // kick off the polling thread
                if let Some(trng_cid) = serial_trng_cid.as_ref() {
                    xous::try_send_message(
                        *trng_cid,
                        xous::Message::new_scalar(TrngOp::Pump.to_usize().unwrap(), 0, 0, 0, 0),
                    )
                    .ok();
                }
            }),
            Some(Opcode::SerialTrngPoll) => {
                if serial_trng_cid.is_none() {
                    // stale request from previously configured TRNG system
                    continue;
                }
                let mut sent = false;
                if serial_port.rts() {
                    if serial_trng_buf.len() < TRNG_PKT_SIZE {
                        match trng.get_test_data() {
                            Ok(data) => {
                                serial_trng_buf.extend_from_slice(&data);
                            }
                            Err(e) => {
                                log::warn!("TRNG returned error while polling to refill: {:?}", e);
                                continue;
                            }
                        }
                    }
                    // at this point, we should have data we can copy to the buffer. Pull it from the end of
                    // the buffer so the Vec can efficiently de-allocate data.
                    match serial_port.flush() {
                        Ok(_) => {
                            let available = serial_trng_buf.len();
                            match serial_port.write(&serial_trng_buf[available - TRNG_PKT_SIZE..available]) {
                                Ok(_) => {
                                    serial_trng_buf.drain(available - TRNG_PKT_SIZE..available);
                                    sent = true;
                                }
                                Err(_) => {
                                    // do nothing, the host port is too full and can't take any more data
                                }
                            }
                        }
                        Err(_) => {
                            // do nothing, we're still sending older data
                        }
                    }
                }
                if !sent {
                    let prev_interval = serial_trng_interval.fetch_add(TRNG_BACKOFF_MS, Ordering::SeqCst);
                    // cap the backoff rate
                    if prev_interval > TRNG_BACKOFF_MAX_MS {
                        log::debug!("Max backoff delay encountered");
                        serial_trng_interval.store(TRNG_BACKOFF_MAX_MS, Ordering::SeqCst);
                    }
                } else {
                    serial_trng_interval.store(TRNG_REFILL_DELAY_MS, Ordering::SeqCst);
                }
            }
            Some(Opcode::HIDSetDescriptor) => {
                let buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let data = buffer.to_original::<HIDReportDescriptorMessage, _>().unwrap();

                // This branch can only error if data.descriptor is longer than the
                // expected maximum.
                // The userland library checks this for us already.
                match hidv2.set_device_report(Vec::from(&data.descriptor[..data.len])) {
                    Ok(_) => (),
                    Err(error) => {
                        log::error!("cannot set hidv2 device report: {:?}", error);
                    }
                }
            }
            Some(Opcode::HIDUnsetDescriptor) => match hidv2.reset_device_report() {
                Ok(_) => (),
                Err(error) => log::error!("cannot reset hidv2 device report: {:?}", error),
            },
            Some(Opcode::HIDReadReport) => {
                if !hidv2.descriptor_set() {
                    log::warn!("trying to read a HID report with no descriptor set!");
                    continue;
                }

                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };

                let mut data = buffer.to_original::<HIDReportMessage, _>().unwrap();

                data.data = hidv2.read_report();

                buffer.replace(data).expect("couldn't serialize return");
            }
            Some(Opcode::HIDWriteReport) => {
                if !hidv2.descriptor_set() {
                    log::warn!("trying to write a HID report with no descriptor set!");
                    continue;
                }

                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let data = buffer.to_original::<HIDReport, _>().unwrap();

                log::info!("report to be written to USB: {:?}", data);

                hidv2.write_report(data);
            }
            Some(Opcode::RegisterUsbObserver) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let ur = buffer.as_flat::<UsbListenerRegistration, _>().unwrap();
                if observer_conn.is_none() {
                    match xns.request_connection_blocking(ur.server_name.as_str()) {
                        Ok(cid) => {
                            observer_conn = Some(cid);
                            observer_op = Some(ur.listener_op_id as usize);
                        }
                        Err(e) => {
                            log::error!("couldn't connect to observer: {:?}", e);
                            observer_conn = None;
                            observer_op = None;
                        }
                    }
                }
            }
            Some(Opcode::Quit) => {
                log::warn!("Quit received, goodbye world!");
                break;
            }
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
