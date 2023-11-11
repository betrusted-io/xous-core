use crate::*;

use num_traits::*;
use usbd_human_interface_device::device::fido::RawFidoReport;
use xous::{msg_scalar_unpack, msg_blocking_scalar_unpack};
use xous_semver::SemVer;
use core::num::NonZeroU8;
use core::sync::atomic::AtomicUsize;
use std::sync::Arc;

use xous_ipc::Buffer;
use std::collections::VecDeque;

pub(crate) fn main_hosted() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let usbdev_sid = xns.register_name(api::SERVER_NAME_USB_DEVICE, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", usbdev_sid);
    let llio = llio::Llio::new(&xns);
    let tt = ticktimer_server::Ticktimer::new().unwrap();

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

    let view = Arc::new(AtomicUsize::new(0));
    let usbdev = SpinalUsbDevice::new(usbdev_sid, view.clone());
    let mut usbmgmt = usbdev.get_iface();

    // register a suspend/resume listener
    let cid = xous::connect(usbdev_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(
        None,
        &xns,
        api::Opcode::SuspendResume as u32,
        cid
    ).expect("couldn't create suspend/resume object");

    let mut fido_listener: Option<xous::MessageEnvelope> = None;
    // under the theory that PIDs are unforgeable. TODO: check that PIDs are unforgeable.
    // also if someone commandeers a process, all bets are off within that process (this is a general statement)
    let mut fido_listener_pid: Option<NonZeroU8> = None;
    let mut fido_rx_queue = VecDeque::<[u8; 64]>::new();

    let mut lockstatus_force_update = true; // some state to track if we've been through a susupend/resume, to help out the status thread with its UX update after a restart-from-cold

    loop {
        let mut msg = xous::receive_message(usbdev_sid).unwrap();
        let opcode: Option<Opcode> = FromPrimitive::from_usize(msg.body.id());
        log::debug!("{:?}", opcode);
        match opcode {
            #[cfg(feature="mass-storage")]
            Some(Opcode::SetBlockDevice) => {
                log::info!("ignoring SetBlockDevice in hosted mode");
            },
            #[cfg(feature="mass-storage")]
            Some(Opcode::SetBlockDeviceSID) => {
                log::info!("ignoring SetBlockDeviceSID in hosted mode");
            },
            #[cfg(feature="mass-storage")]
            Some(Opcode::ResetBlockDevice) => {
                log::info!("ignoring ResetBlockDevice in hosted mode");
            },
            Some(Opcode::SuspendResume) => msg_scalar_unpack!(msg, token, _, _, _, {
                usbmgmt.xous_suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                // resume1 + reset brings us to an initialized state
                usbmgmt.xous_resume1();
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
                    let mut u2f_msg = RawFidoReport::default();
                    assert_eq!(u2f_ipc.code, U2fCode::Tx, "Expected U2fCode::Tx in wrapper");
                    u2f_msg.packet.copy_from_slice(&u2f_ipc.data);
                    log::debug!("sent U2F packet {:x?}", u2f_ipc.data);
                    u2f_ipc.code = U2fCode::TxAck;
                } else {
                    u2f_ipc.code = U2fCode::Denied;
                }
                buffer.replace(u2f_ipc).unwrap();
            }
            Some(Opcode::UsbIrqHandler) => {

            },
            Some(Opcode::SwitchCores) => msg_blocking_scalar_unpack!(msg, core, _, _, _, {
                if core == 1 {
                    log::info!("Connecting USB device core; disconnecting debug USB core");
                    usbmgmt.connect_device_core(true);
                } else {
                    log::info!("Connecting debug core; disconnecting USB device core");
                    usbmgmt.connect_device_core(false);
                }
                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            Some(Opcode::EnsureCore) => msg_blocking_scalar_unpack!(msg, core, _, _, _, {
                if core == 1 {
                    if !usbmgmt.is_device_connected() {
                        log::info!("Connecting USB device core; disconnecting debug USB core");
                        usbmgmt.connect_device_core(true);
                    }
                } else {
                    if usbmgmt.is_device_connected() {
                        log::info!("Connecting debug core; disconnecting USB device core");
                        usbmgmt.connect_device_core(false);
                    }
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
                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            Some(Opcode::SendKeyCode) => {
                xous::return_scalar(msg.sender, 1).unwrap();
            }
            Some(Opcode::SendString) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let usb_send = buffer.to_original::<api::UsbString, _>().unwrap(); // suppress mut warning on hosted mode
                buffer.replace(usb_send).unwrap();
            }
            Some(Opcode::GetLedState) => {
                xous::return_scalar(msg.sender, 0).unwrap();
            }
            Some(Opcode::Quit) => {
                log::warn!("Quit received, goodbye world!");
                break;
            },
            _  => log::warn!("Opcode not supported: {:?}", msg),
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(usbdev_sid).unwrap();
    xous::destroy_server(usbdev_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}