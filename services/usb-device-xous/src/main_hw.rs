use crate::*;

use num_traits::*;
use usb_device_xous::KeyboardLedsReport;
use usb_device_xous::UsbDeviceType;
use usbd_human_interface_device::device::fido::RawFidoMsg;
use usbd_human_interface_device::device::fido::RawFidoInterface;
use xous::{msg_scalar_unpack, msg_blocking_scalar_unpack};
#[cfg(not(feature="minimal"))]
use xous_semver::SemVer;
use core::num::NonZeroU8;
use core::sync::atomic::{AtomicU32, AtomicBool, Ordering};
use std::sync::Arc;
use utralib::generated::*;

use usb_device::prelude::*;
use usb_device::class_prelude::*;
use usbd_human_interface_device::page::Keyboard;
use usbd_human_interface_device::device::keyboard::NKROBootKeyboardInterface;
use usbd_human_interface_device::prelude::*;
use num_enum::FromPrimitive as EnumFromPrimitive;

use embedded_time::Clock;
use std::convert::TryInto;
#[cfg(not(feature="minimal"))]
use keyboard::KeyMap;
use xous_ipc::Buffer;
use std::collections::VecDeque;

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

/// Time allowed for switchover between device core types. It's longer because some hosts
/// get really confused when you have the same VID/PID show up with a different set of endpoints.
const EXTENDED_CORE_RESET_MS: usize = 4000;
#[derive(Eq, PartialEq)]
#[repr(usize)]
enum Views {
    FidoWithKbd = 0,
    FidoOnly = 1,
    #[cfg(feature="mass-storage")]
    MassStorage = 2,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
enum TimeoutOp {
    Pump,
    InvalidCall,
    Quit,
}

pub(crate) fn main_hw() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let usbdev_sid = xns.register_name(api::SERVER_NAME_USB_DEVICE, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", usbdev_sid);
    #[cfg(not(feature="minimal"))]
    let llio = llio::Llio::new(&xns);
    let tt = ticktimer_server::Ticktimer::new().unwrap();
    #[cfg(not(feature="minimal"))]
    let native_kbd = keyboard::Keyboard::new(&xns).unwrap();
    #[cfg(not(feature="minimal"))]
    let native_map = native_kbd.get_keymap().unwrap();

    #[cfg(not(feature="minimal"))]
    let serial_number = format!("{:x}", llio.soc_dna().unwrap());
    #[cfg(not(feature="minimal"))]
    {
        let minimum_ver = SemVer {maj: 0, min: 9, rev: 8, extra: 20, commit: None};
        let soc_ver = llio.soc_gitrev().unwrap();
        if soc_ver < minimum_ver {
            if soc_ver.min != 0 { // don't show during hosted mode, which reports 0.0.0+0
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
                    Some(Opcode::DebugUsbOp) => msg_blocking_scalar_unpack!(msg, _update_req, _new_state, _, _, {
                        xous::return_scalar2(msg.sender, 0, 1).expect("couldn't return status");
                    }),
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
                        }) = msg.body {
                            log::warn!("Returning bogus result");
                            xous::return_scalar(msg.sender, 0).unwrap();
                        }
                    }
                }
            }
            log::info!("consuming listener: {:?}", fido_listener);
        }
    }
    #[cfg(feature="minimal")]
    let serial_number = "minimalbuild";
    #[cfg(feature="minimal")]
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
    let usb = xous::syscall::map_memory(
        xous::MemoryAddress::new(utralib::HW_USBDEV_MEM),
        None,
        utralib::HW_USBDEV_MEM_LEN,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map USB device memory range");
    let csr = xous::syscall::map_memory(
        xous::MemoryAddress::new(utra::usbdev::HW_USBDEV_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map USB CSR range");

    let usb_fidokbd_dev = SpinalUsbDevice::new(usbdev_sid, usb.clone(), csr.clone());
    let mut usbmgmt = usb_fidokbd_dev.get_iface();
    // before doing any allocs, clone a copy of the hardware access structure so we can build a second
    // view into the hardware with only FIDO descriptors
    let usb_fido_dev = SpinalUsbDevice::new(usbdev_sid, usb.clone(), csr.clone());
    // do the same thing for mass storage
    #[cfg(feature="mass-storage")]
    let ums_dev = SpinalUsbDevice::new(usbdev_sid, usb.clone(), csr.clone());

    // track which view is visible on the device core
    #[cfg(not(feature="minimal"))]
    let mut view = Views::FidoWithKbd;

    // register a suspend/resume listener
    let cid = xous::connect(usbdev_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(
        None,
        &xns,
        api::Opcode::SuspendResume as u32,
        cid
    ).expect("couldn't create suspend/resume object");

    // FIDO + keyboard
    let usb_alloc = UsbBusAllocator::new(usb_fidokbd_dev);
    let clock = EmbeddedClock::new();

    let mut composite = UsbHidClassBuilder::new()
        .add_interface(
            NKROBootKeyboardInterface::default_config(&clock),
        )
        .add_interface(
            RawFidoInterface::default_config()
        )
        .build(&usb_alloc);

    let mut usb_dev = UsbDeviceBuilder::new(&usb_alloc, UsbVidPid(0x1209, 0x3613))
        .manufacturer("Kosagi")
        .product("Precursor")
        .serial_number(&serial_number)
        .build();

    // FIDO only
    let fido_alloc = UsbBusAllocator::new(usb_fido_dev);
    let mut fido_class = UsbHidClassBuilder::new()
        .add_interface(
            RawFidoInterface::default_config()
        )
        .build(&fido_alloc);

    let mut fido_dev = UsbDeviceBuilder::new(&fido_alloc, UsbVidPid(0x1209, 0x3613))
    .manufacturer("Kosagi")
    .product("Precursor")
    .serial_number(&serial_number)
    .build();

    // Mass storage
    #[cfg(feature="mass-storage")]
    let ums_alloc = UsbBusAllocator::new(ums_dev);
    #[cfg(feature="mass-storage")]
    let abd = apps_block_device::AppsBlockDevice::new();
    #[cfg(feature="mass-storage")]
    let abdcid = abd.conn();
    #[cfg(feature="mass-storage")]
    let mut ums = usbd_scsi::Scsi::new(
        &ums_alloc,
        64,
        abd,
        "Kosagi".as_bytes(),
        "Kosagi Precursor".as_bytes(),
        "1".as_bytes()
    );

    #[cfg(feature="mass-storage")]
    let mut ums_device = UsbDeviceBuilder::new(&ums_alloc, UsbVidPid(0x1209, 0x3613))
        .manufacturer("Kosagi")
        .product("Precursor")
        .serial_number(&serial_number)
        .self_powered(false)
        .max_power(500)
        .build();

    let mut led_state: KeyboardLedsReport = KeyboardLedsReport::default();
    let mut fido_listener: Option<xous::MessageEnvelope> = None;
    // under the theory that PIDs cannot be forged. TODO: check that PIDs cannot be forged.
    // also if someone commandeers a process, all bets are off within that process (this is a general statement)
    let mut fido_listener_pid: Option<NonZeroU8> = None;
    let mut fido_rx_queue = VecDeque::<[u8; 64]>::new();

    let mut lockstatus_force_update = true; // some state to track if we've been through a suspend/resume, to help out the status thread with its UX update after a restart-from-cold
    let mut was_suspend = true;

    #[cfg(feature="minimal")]
    std::thread::spawn(move || {
        // this keeps the watchdog alive in minimal mode; if there's no event, eventually the watchdog times out
        let tt = ticktimer_server::Ticktimer::new().unwrap();
        loop {
            tt.sleep_ms(1500).ok();
        }
    });
    // switch the core automatically on boot
    #[cfg(feature="minimal")]
    let mut view = Views::MassStorage;
    #[cfg(feature="minimal")]
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
    // we don't have AtomicU64 on this platform, so we suffer from a rollover condition in timeouts once every 46 days
    // this manifests as a timeout that happens to be scheduled on the rollover being rounded to a max limit timeout of
    // 5 seconds, and/or an immediate timeout happening during the 5 seconds before the 46-day limit
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
                xous::reply_and_receive_next_legacy(to_server, &mut msg_opt, &mut return_type)
                .unwrap();
                let msg = msg_opt.as_mut().unwrap();
                // loop only consumes CPU time when a timeout is active. Once it has timed out,
                // it will wait for a new pump call.
                let now = tt.elapsed_ms();
                match num_traits::FromPrimitive::from_usize(msg.body.id())
                .unwrap_or(TimeoutOp::InvalidCall)
                {
                    TimeoutOp::Pump => {
                        if to_run.load(Ordering::SeqCst) {
                            let tt_lsb = target_time_lsb.load(Ordering::SeqCst);
                            if tt_lsb >= (now as u32) ||
                                (now as u32) - tt_lsb > MAX_TIMEOUT_LIMIT_MS // limits rollover case
                            {
                                xous::try_send_message(cid,
                                    xous::Message::new_scalar(Opcode::U2fRxTimeout.to_usize().unwrap(),
                                        0, 0, 0, 0)
                                ).ok();
                                // no need to set to_run to `false` because a Pump message isn't initiated;
                                // the loop de-facto stops from a lack of new Pump messages
                            } else {
                                if next_wake <= now {
                                    next_wake = now + POLL_INTERVAL_MS;
                                    tt.sleep_ms(POLL_INTERVAL_MS as usize).ok();
                                    xous::try_send_message(to_conn,
                                        xous::Message::new_scalar(TimeoutOp::Pump.to_usize().unwrap(),
                                            0, 0, 0, 0)
                                    ).ok();
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
                        log::error!("Unknown opcode received in FIDO Rx timeout handler: {:?}", msg.body.id());
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
            #[cfg(feature="mass-storage")]
            Some(Opcode::SetBlockDevice) => msg_blocking_scalar_unpack!(msg, read_id, write_id, max_lba_id, _, {
                xous::send_message(
                    abdcid, 
                    xous::Message::new_blocking_scalar(
                        apps_block_device::BlockDeviceMgmtOp::SetOps.to_usize().unwrap(), read_id, write_id, max_lba_id, 0,
                    )
                ).unwrap();
                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            #[cfg(feature="mass-storage")]
            Some(Opcode::SetBlockDeviceSID) => msg_blocking_scalar_unpack!(msg, sid1, sid2, sid3, sid4, {
                xous::send_message(abdcid,
                    xous::Message::new_blocking_scalar(
                        apps_block_device::BlockDeviceMgmtOp::SetSID.to_usize().unwrap(), sid1, sid2, sid3, sid4
                    )
                ).unwrap();
                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            #[cfg(feature="mass-storage")]
            Some(Opcode::ResetBlockDevice) => msg_blocking_scalar_unpack!(msg, 0, 0, 0, 0, {
                xous::send_message(abdcid,
                    xous::Message::new_blocking_scalar(
                        apps_block_device::BlockDeviceMgmtOp::Reset.to_usize().unwrap(), 0, 0, 0, 0,
                    )
                ).unwrap();
                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            Some(Opcode::SuspendResume) => msg_scalar_unpack!(msg, token, _, _, _, {
                usbmgmt.xous_suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                // resume1 + reset brings us to an initialized state
                usbmgmt.xous_resume1();
                match view {
                    Views::FidoWithKbd => {
                        match usb_dev.force_reset() {
                            Err(e) => log::warn!("USB reset on resume failed: {:?}", e),
                            _ => ()
                        };
                    }
                    Views::FidoOnly => {
                        match fido_dev.force_reset() {
                            Err(e) => log::warn!("USB reset on resume failed: {:?}", e),
                            _ => ()
                        };
                    }
                    #[cfg(feature="mass-storage")]
                    Views::MassStorage => {
                        // TODO: test this
                        match ums_device.force_reset() {
                            Err(e) => log::warn!("USB reset on resume failed: {:?}", e),
                            _ => ()
                        };
                    }
                }
                // resume2 brings us to our last application state
                usbmgmt.xous_resume2();
                lockstatus_force_update = true; // notify the status bar that yes, it does need to redraw the lock status, even if the value hasn't changed since the last read
            }),
            Some(Opcode::IsSocCompatible) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, 1).expect("couldn't return compatibility status")
            }),
            Some(Opcode::U2fRxDeferred) => {
                if fido_listener_pid.is_none() {
                    fido_listener_pid = msg.sender.pid();
                }
                if fido_listener.is_some() {
                    log::error!("Double-listener request detected. There should only ever by one registered listener at a time.");
                    log::error!("This will cause an upstream server to misbehave, but not panicing so the problem can be debugged.");
                    // the receiver will get a response with the `code` field still in the `RxWait` state to indicate the problem
                }
                if fido_listener_pid == msg.sender.pid() {
                    // preferentially pull from the rx queue if it has elements
                    if let Some(data) = fido_rx_queue.pop_front() {
                        log::debug!("no deferral: ret queued data: {:?} queue len: {}", &data[..8], fido_rx_queue.len() + 1);
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
                        { // not tested
                            let spec = unsafe {
                                Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                            };
                            if let Some(to) = spec.to_original::<U2fMsgIpc, _>().unwrap().timeout_ms {
                                let target = tt.elapsed_ms() + to;
                                target_time_lsb.store(target as u32, Ordering::SeqCst); // this will keep updating the target time later and later
                                // run must always be set *after* target time is updated, because there is always a chance
                                // we timed out and checked target time between these two steps
                                to_run.store(true, Ordering::SeqCst);
                                xous::try_send_message(to_conn,
                                    xous::Message::new_scalar(TimeoutOp::Pump.to_usize().unwrap(), 0, 0, 0, 0)
                                ).ok();
                            }
                        };
                        fido_listener = Some(msg);
                    }
                } else {
                    log::warn!("U2F interface capability is locked on first use; additional servers are ignored: {:?}", msg.sender);
                    let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
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
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut u2f_ipc = buffer.to_original::<U2fMsgIpc, _>().unwrap();
                if fido_listener_pid == msg.sender.pid() {
                    let mut u2f_msg = RawFidoMsg::default();
                    assert_eq!(u2f_ipc.code, U2fCode::Tx, "Expected U2fCode::Tx in wrapper");
                    u2f_msg.packet.copy_from_slice(&u2f_ipc.data);
                    let u2f = match view {
                        Views::FidoWithKbd => composite.interface::<RawFidoInterface<'_, _>, _>(),
                        Views::FidoOnly => fido_class.interface::<RawFidoInterface<'_, _>, _>(),
                        #[cfg(feature="mass-storage")]
                        Views::MassStorage => panic!("expected u2f tx when in mass storage mode!"),
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
                            let keyboard = composite.interface::<NKROBootKeyboardInterface<'_, _, _,>, _>();
                            match keyboard.read_report() {
                                Ok(l) => {
                                    log::info!("keyboard LEDs: {:?}", l);
                                    led_state = l;
                                }
                                Err(e) => log::trace!("KEYB ERR: {:?}", e),
                            }
                            Some(composite.interface::<RawFidoInterface<'_, _>, _>())
                        } else {
                            None
                        }
                    }
                    Views::FidoOnly => {
                        if fido_dev.poll(&mut [&mut fido_class]) {
                            Some(fido_class.interface::<RawFidoInterface<'_, _>, _>())
                        } else {
                            None
                        }
                    },
                    #[cfg(feature="mass-storage")]
                    Views::MassStorage => {
                        if ums_device.poll(&mut [&mut ums]) {
                            log::debug!("ums device had something to do!")
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
                                    Buffer::from_memory_message_mut(listener.body.memory_message_mut().unwrap())
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
                        },
                        Err(e) => log::trace!("U2F ERR: {:?}", e),
                    }
                }

                let is_suspend = match view {
                    Views::FidoWithKbd => usb_dev.state() == UsbDeviceState::Suspend,
                    Views::FidoOnly => fido_dev.state() == UsbDeviceState::Suspend,
                    #[cfg(feature="mass-storage")]
                    Views::MassStorage => ums_device.state() == UsbDeviceState::Suspend,
                };
                if is_suspend {
                    log::info!("suspend detected");
                    if was_suspend == false {
                        // FIDO listener needs to know when USB was unplugged, so that it can reset state per FIDO2 spec
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
            },
            // always triggers a reset when called
            Some(Opcode::SwitchCores) => msg_blocking_scalar_unpack!(msg, core, _, _, _, {
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
                        let keyboard = composite.interface::<NKROBootKeyboardInterface<'_, _, _,>, _>();
                        keyboard.write_report(&[]).ok(); // queues an "all key-up" for the interface
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
                    #[cfg(feature="mass-storage")]
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
                }
                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            // does not trigger a reset if we're already on the core
            Some(Opcode::EnsureCore) => msg_blocking_scalar_unpack!(msg, core, _, _, _, {
                let devtype: UsbDeviceType = core.try_into().unwrap();
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
                        let keyboard = composite.interface::<NKROBootKeyboardInterface<'_, _, _,>, _>();
                        keyboard.write_report(&[]).ok(); // queues an "all key-up" for the interface
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
                    },
                    #[cfg(feature="mass-storage")]
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
                }
                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            Some(Opcode::WhichCore) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if usbmgmt.is_device_connected() {
                    match view {
                        Views::FidoWithKbd => xous::return_scalar(msg.sender, UsbDeviceType::FidoKbd as usize).unwrap(),
                        Views::FidoOnly => xous::return_scalar(msg.sender, UsbDeviceType::Fido as usize).unwrap(),
                        #[cfg(feature="mass-storage")]
                        Views::MassStorage => xous::return_scalar(msg.sender, UsbDeviceType::MassStorage as usize).unwrap(),
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
                match view {
                    Views::FidoWithKbd => xous::return_scalar(msg.sender, usb_dev.state() as usize).unwrap(),
                    Views::FidoOnly => xous::return_scalar(msg.sender, fido_dev.state() as usize).unwrap(),
                    #[cfg(feature="mass-storage")]
                    Views::MassStorage => xous::return_scalar(msg.sender, ums_device.state() as usize).unwrap(),
                }
            }),
            Some(Opcode::SendKeyCode) => msg_blocking_scalar_unpack!(msg, code0, code1, code2, autoup, {
                match view {
                    Views::FidoWithKbd => {
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
                            let keyboard = composite.interface::<NKROBootKeyboardInterface<'_, _, _,>, _>();
                            keyboard.write_report(&codes).ok();
                            keyboard.tick().ok();
                            tt.sleep_ms(30).ok();
                            if auto_up {
                                keyboard.write_report(&[]).ok(); // this is the key-up
                                keyboard.tick().ok();
                                tt.sleep_ms(30).ok();
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
            Some(Opcode::SendString) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut usb_send = buffer.to_original::<api::UsbString, _>().unwrap();
                #[cfg(not(feature="minimal"))]
                let mut sent = 0;
                #[cfg(feature="minimal")]
                let sent = 0;
                match view {
                    #[cfg(not(feature="minimal"))]
                    Views::FidoWithKbd => {
                        for ch in usb_send.s.as_str().unwrap().chars() {
                            // ASSUME: user's keyboard type matches the preference on their Precursor device.
                            let codes = match native_map {
                                KeyMap::Dvorak => mappings::char_to_hid_code_dvorak(ch),
                                _ => mappings::char_to_hid_code_us101(ch),
                            };
                            let keyboard = composite.interface::<NKROBootKeyboardInterface<'_, _, _,>, _>();
                            keyboard.write_report(&codes).ok();
                            keyboard.tick().ok();
                            tt.sleep_ms(30).ok();
                            keyboard.write_report(&[]).ok(); // this is the key-up
                            keyboard.tick().ok();
                            tt.sleep_ms(30).ok();
                            sent += 1;
                        }
                    }
                    _ => {} // do nothing; will report that 0 characters were sent
                }
                usb_send.sent = Some(sent);
                buffer.replace(usb_send).unwrap();
            }
            Some(Opcode::GetLedState) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let mut code = [0u8; 1];
                led_state.pack_to_slice(&mut code).unwrap();
                xous::return_scalar(msg.sender, code[0] as usize).unwrap();
            }),
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