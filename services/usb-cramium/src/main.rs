mod api;
mod debug;
mod hw;
#[cfg(not(target_os = "xous"))]
mod main_hosted;
mod mappings;

use core::convert::TryFrom;
use core::num::NonZeroU8;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::collections::VecDeque;
use std::sync::Arc;

use api::{HIDReport, LogLevel, Opcode, U2fCode, U2fMsgIpc, UsbListenerRegistration};
use cram_hal_service::api::KeyMap;
use cramium_hal::usb::driver::{CorigineUsb, CorigineWrapper};
use hw::CramiumUsb;
use hw::UsbIrqReq;
use num_traits::*;
use usb_device::class_prelude::*;
use utralib::AtomicCsr;
use xous::{msg_blocking_scalar_unpack, msg_scalar_unpack};
use xous_ipc::Buffer;
use xous_usb_hid::device::fido::RawFidoReport;
use xous_usb_hid::page::Keyboard;

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
enum TimeoutOp {
    Pump,
    InvalidCall,
    Quit,
}

fn main() -> ! {
    #[cfg(target_os = "xous")]
    main_hw();
    #[cfg(not(target_os = "xous"))]
    main_hosted::main_hosted();
}

pub(crate) fn main_hw() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let usbdev_sid = xns.register_name(api::SERVER_NAME_USB_DEVICE, None).expect("can't register server");
    let cid = xous::connect(usbdev_sid).expect("couldn't create suspend callback connection");
    log::trace!("registered with NS -- {:?}", usbdev_sid);
    let tt = ticktimer::Ticktimer::new().unwrap();

    let serial_number = format!("TODO!!"); // implement in cramium-hal once we have a serial number API

    let native_kbd =
        cram_hal_service::keyboard::Keyboard::new(&xns).expect("couldn't connect to keyboard service");

    let usb_mapping = xous::syscall::map_memory(
        xous::MemoryAddress::new(cramium_hal::usb::utra::CORIGINE_USB_BASE),
        None,
        cramium_hal::usb::utra::CORIGINE_USB_LEN,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't reserve register pages");
    let ifram_range = xous::syscall::map_memory(
        xous::MemoryAddress::new(cramium_hal::usb::driver::CRG_UDC_MEMBASE),
        xous::MemoryAddress::new(cramium_hal::usb::driver::CRG_UDC_MEMBASE), /* make P & V addresses
                                                                              * line up */
        cramium_hal::usb::driver::CRG_IFRAM_PAGES * 0x1000,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't allocate IFRAM pages");
    assert!(
        cramium_hal::usb::driver::CRG_UDC_TOTAL_MEM_LEN <= cramium_hal::usb::driver::CRG_IFRAM_PAGES * 0x1000
    );
    log::info!(
        "total memory len: {:x}, allocated: {:x}",
        cramium_hal::usb::driver::CRG_UDC_TOTAL_MEM_LEN,
        cramium_hal::usb::driver::CRG_IFRAM_PAGES * 0x1000
    );
    let irq_range = xous::syscall::map_memory(
        xous::MemoryAddress::new(utralib::utra::irqarray1::HW_IRQARRAY1_BASE),
        None,
        0x1000,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't allocate IFRAM pages");
    let usb = AtomicCsr::new(usb_mapping.as_ptr() as *mut u32);
    let irq_csr = AtomicCsr::new(irq_range.as_ptr() as *mut u32);
    let h_op: usize = Opcode::UsbIrqHandler.to_usize().unwrap();

    let corigine_usb =
        unsafe { CorigineUsb::new(cid, h_op, ifram_range.as_ptr() as usize, usb.clone(), irq_csr.clone()) };
    // safety: this is only safe because we will actually claim the IRQ after all the initializations are
    // done, and we promise not to enable interrupts until that time, either.
    unsafe {
        corigine_usb.irq_claimed();
    }
    let cw = CorigineWrapper::new(corigine_usb);
    let usb_alloc = UsbBusAllocator::new(cw.clone());

    let mut cu = Box::new(CramiumUsb::new(cid, cw, &usb_alloc, usb.clone(), irq_csr.clone(), &serial_number));

    // this has to be done in `main` because we're passing a pointer to the Box'd structure, which
    // the IRQ handler can freely and safely manipulate
    xous::claim_interrupt(
        utralib::utra::irqarray1::IRQARRAY1_IRQ,
        hw::composite_handler,
        &mut cu as *mut Box<CramiumUsb> as *mut usize,
    )
    .expect("couldn't claim irq");

    // enable both the corigine core IRQ and the software IRQ bit
    // software IRQ is used to initiate send/receive from software to the interrupt context
    irq_csr.wo(utralib::utra::irqarray1::EV_ENABLE, hw::CORIGINE_IRQ_MASK | hw::SW_IRQ_MASK);

    // under the theory that PIDs cannot be forged.
    // also if someone commandeers a process, all bets are off within that process (this is a general
    // statement)
    let mut fido_listener_pid: Option<NonZeroU8> = None;
    let mut fido_listener: Option<xous::MessageEnvelope> = None;
    let mut fido_rx_queue: VecDeque<[u8; 64]> = VecDeque::new();

    let mut autotype_delay_ms = 30;

    // event observer connection
    let mut observer_conn: Option<xous::CID> = None;
    let mut observer_op: Option<usize> = None;

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
            let tt = ticktimer::Ticktimer::new().unwrap();
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

    let mut msg_opt = None;
    loop {
        xous::reply_and_receive_next(usbdev_sid, &mut msg_opt).unwrap();
        let msg = msg_opt.as_mut().unwrap();
        let opcode = num_traits::FromPrimitive::from_usize(msg.body.id()).unwrap_or(Opcode::InvalidCall);
        log::debug!("{:?}", opcode);
        match opcode {
            Opcode::U2fRxDeferred => {
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
                        fido_listener = msg_opt.take();
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
            Opcode::U2fRxTimeout => {
                // put the lock in its own scope so we release it as soon as we've taken the message
                let maybe_listener = { fido_listener.take() };
                if let Some(mut listener) = maybe_listener {
                    let mut response = unsafe {
                        Buffer::from_memory_message_mut(listener.body.memory_message_mut().unwrap())
                    };
                    let mut buf = response.to_original::<U2fMsgIpc, _>().unwrap();
                    assert_eq!(buf.code, U2fCode::RxWait, "Expected U2fcode::RxWait in wrapper");
                    buf.code = U2fCode::RxTimeout;
                    response.replace(buf).unwrap();
                }
            }
            Opcode::IrqFidoRx => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let u2f_report = buffer.to_original::<HIDReport, _>().unwrap();
                if let Some(mut listener) = fido_listener.take() {
                    let mut response = unsafe {
                        Buffer::from_memory_message_mut(listener.body.memory_message_mut().unwrap())
                    };
                    let mut deferred_buf = response.to_original::<U2fMsgIpc, _>().unwrap();

                    deferred_buf.data.copy_from_slice(&u2f_report.0);
                    log::trace!("ret deferred data {:x?}", &u2f_report.0[..8]);
                    deferred_buf.code = U2fCode::RxAck;
                    response.replace(deferred_buf).unwrap();
                } else {
                    crate::println!("Got U2F packet, but no server to respond...queuing.");
                    fido_rx_queue.push_back(u2f_report.0);
                }
            }
            Opcode::U2fTx => {
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
                    {
                        // scope this so the borrow is released before the IRQ is tripped
                        cu.fido_tx_queue.borrow_mut().push_back(u2f_msg);
                    }
                    cu.sw_irq(UsbIrqReq::FidoTx);
                    log::debug!("enqueued U2F packet {:x?}", u2f_ipc.data);
                    u2f_ipc.code = U2fCode::TxAck;
                } else {
                    u2f_ipc.code = U2fCode::Denied;
                }
                buffer.replace(u2f_ipc).unwrap();
            }
            Opcode::SendKeyCode => msg_blocking_scalar_unpack!(msg, code0, code1, code2, autoup, {
                let native_map = native_kbd.get_keymap().unwrap();
                if code0 != 0 {
                    cu.kbd_tx_queue.borrow_mut().push_back(match native_map {
                        KeyMap::Dvorak => mappings::char_to_hid_code_dvorak(code0 as u8 as char)[0],
                        _ => mappings::char_to_hid_code_us101(code0 as u8 as char)[0],
                    });
                }
                if code1 != 0 {
                    cu.kbd_tx_queue.borrow_mut().push_back(match native_map {
                        KeyMap::Dvorak => mappings::char_to_hid_code_dvorak(code1 as u8 as char)[0],
                        _ => mappings::char_to_hid_code_us101(code1 as u8 as char)[0],
                    });
                }
                if code2 != 0 {
                    cu.kbd_tx_queue.borrow_mut().push_back(match native_map {
                        KeyMap::Dvorak => mappings::char_to_hid_code_dvorak(code2 as u8 as char)[0],
                        _ => mappings::char_to_hid_code_us101(code2 as u8 as char)[0],
                    });
                }
                let auto_up = if autoup == 1 { true } else { false };
                // kbd_tx_queue borrow_mut() should be out of scope before the IRQ is fired
                cu.sw_irq(UsbIrqReq::KbdTx);
                tt.sleep_ms(autotype_delay_ms).ok();
                if auto_up {
                    {
                        // ensure borrow_mut() is scoped out before IRQ is fired
                        cu.kbd_tx_queue.borrow_mut().push_back(Keyboard::NoEventIndicated);
                    }
                    cu.sw_irq(UsbIrqReq::KbdTx);
                    tt.sleep_ms(autotype_delay_ms).ok();
                }
                xous::return_scalar(msg.sender, 0).unwrap();
            }),

            Opcode::SendString => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut usb_send = buffer.to_original::<api::UsbString, _>().unwrap();
                let mut sent = 0;

                // check keymap on every call because we may need to toggle this for e.g. plugging
                // into a new host with a different map
                let native_map = native_kbd.get_keymap().unwrap();
                for ch in usb_send.s.as_str().chars() {
                    // ASSUME: user's keyboard type matches the preference on their Precursor device.
                    let codes = match native_map {
                        KeyMap::Dvorak => mappings::char_to_hid_code_dvorak(ch),
                        _ => mappings::char_to_hid_code_us101(ch),
                    };
                    for code in codes {
                        cu.kbd_tx_queue.borrow_mut().push_back(code);
                    }
                    // key down
                    cu.sw_irq(UsbIrqReq::KbdTx);
                    tt.sleep_ms(autotype_delay_ms).ok();
                    // key up
                    {
                        // ensure that borrow_mut() is scoped out before IRQ is fired
                        cu.kbd_tx_queue.borrow_mut().push_back(Keyboard::NoEventIndicated);
                    }
                    cu.sw_irq(UsbIrqReq::KbdTx);
                    tt.sleep_ms(autotype_delay_ms).ok();

                    sent += 1;
                }
                usb_send.sent = Some(sent as _);
                buffer.replace(usb_send).unwrap();
            }
            Opcode::RegisterUsbObserver => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let ur = buffer.as_flat::<UsbListenerRegistration, _>().unwrap();
                if observer_conn.is_none() {
                    match xns.request_connection_blocking(ur.server_name.as_str()) {
                        Ok(cid) => {
                            observer_conn = Some(cid);
                            observer_op = Some(<u32 as From<u32>>::from(ur.listener_op_id.into()) as usize);
                        }
                        Err(e) => {
                            log::error!("couldn't connect to observer: {:?}", e);
                            observer_conn = None;
                            observer_op = None;
                        }
                    }
                }
            }
            Opcode::SetAutotypeRate => msg_scalar_unpack!(msg, rate, _, _, _, {
                // limit rate to 0.5s delay. Even then, this will probably cause repeated characters because
                // it also adjusts keydown delays
                let checked_rate = if rate > 500 { 500 } else { rate };
                // there is no limit on the minimum rate. good luck if you set it to 0!
                autotype_delay_ms = checked_rate;
            }),
            Opcode::SetLogLevel => msg_scalar_unpack!(msg, level_code, _, _, _, {
                let level = LogLevel::try_from(level_code).unwrap_or(LogLevel::Info);
                match level {
                    LogLevel::Trace => log::set_max_level(log::LevelFilter::Trace),
                    LogLevel::Info => log::set_max_level(log::LevelFilter::Info),
                    LogLevel::Debug => log::set_max_level(log::LevelFilter::Debug),
                    LogLevel::Warn => log::set_max_level(log::LevelFilter::Warn),
                    LogLevel::Err => log::set_max_level(log::LevelFilter::Error),
                }
            }),
            Opcode::InvalidCall => {
                log::warn!("Illegal opcode received {:?}", msg);
            }
            Opcode::Quit => {
                log::warn!("Quit received, goodbye world!");
                break;
            }
            _ => {
                unimplemented!(
                    "Opcode {:?} not implemented for this version of the stack: {:?}",
                    opcode,
                    msg
                );
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
