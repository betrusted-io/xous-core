use core::num::NonZeroU8;
use std::collections::VecDeque;

use api::*;
use num_traits::*;
use xous::{msg_blocking_scalar_unpack, msg_scalar_unpack};
use xous_ipc::Buffer;
use xous_usb_hid::device::fido::RawFidoReport;

use crate::*;

pub(crate) fn main_hosted() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let usbdev_sid = xns.register_name(api::SERVER_NAME_USB_DEVICE, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", usbdev_sid);

    let mut fido_listener: Option<xous::MessageEnvelope> = None;
    // under the theory that PIDs are unforgeable. TODO: check that PIDs are unforgeable.
    // also if someone commandeers a process, all bets are off within that process (this is a general
    // statement)
    let mut fido_listener_pid: Option<NonZeroU8> = None;
    let mut fido_rx_queue = VecDeque::<[u8; 64]>::new();

    loop {
        let mut msg = xous::receive_message(usbdev_sid).unwrap();
        let opcode: Option<Opcode> = FromPrimitive::from_usize(msg.body.id());
        log::debug!("{:?}", opcode);
        match opcode {
            #[cfg(feature = "mass-storage")]
            Some(Opcode::SetBlockDevice) => {
                log::info!("ignoring SetBlockDevice in hosted mode");
            }
            #[cfg(feature = "mass-storage")]
            Some(Opcode::SetBlockDeviceSID) => {
                log::info!("ignoring SetBlockDeviceSID in hosted mode");
            }
            #[cfg(feature = "mass-storage")]
            Some(Opcode::ResetBlockDevice) => {
                log::info!("ignoring ResetBlockDevice in hosted mode");
            }
            Some(Opcode::IsSocCompatible) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, 1).expect("couldn't return compatibility status")
            }),
            Some(Opcode::U2fRxDeferred) => {
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
                    log::debug!("sent U2F packet {:x?}", u2f_ipc.data);
                    u2f_ipc.code = U2fCode::TxAck;
                } else {
                    u2f_ipc.code = U2fCode::Denied;
                }
                buffer.replace(u2f_ipc).unwrap();
            }
            Some(Opcode::UsbIrqHandler) => {}
            Some(Opcode::SwitchCores) => msg_blocking_scalar_unpack!(msg, _core, _, _, _, {
                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            Some(Opcode::EnsureCore) => msg_blocking_scalar_unpack!(msg, _core, _, _, _, {
                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            Some(Opcode::WhichCore) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            Some(Opcode::RestrictDebugAccess) => msg_scalar_unpack!(msg, _restrict, _, _, _, {}),
            Some(Opcode::IsRestricted) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, 1).unwrap();
            }),
            Some(Opcode::DebugUsbOp) => msg_blocking_scalar_unpack!(msg, _update_req, _new_state, _, _, {
                xous::return_scalar2(msg.sender, 1, 0).expect("couldn't return status");
            }),
            Some(Opcode::LinkStatus) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            Some(Opcode::SendKeyCode) => {
                xous::return_scalar(msg.sender, 1).unwrap();
            }
            Some(Opcode::SendString) => {}
            Some(Opcode::GetLedState) => {
                xous::return_scalar(msg.sender, 0).unwrap();
            }
            Some(Opcode::Quit) => {
                log::warn!("Quit received, goodbye world!");
                break;
            }
            _ => log::warn!("Opcode not supported: {:?}", msg),
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(usbdev_sid).unwrap();
    xous::destroy_server(usbdev_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
