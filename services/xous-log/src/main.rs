#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use xous_api_log::api;

#[macro_use]
mod platform;

use core::fmt::Write;

use num_traits::FromPrimitive;
use platform::implementation;

/// A page-aligned stack allocation for connection requests (used by USB resolver)
#[cfg(feature = "usb")]
#[repr(C, align(4096))]
struct ConnectRequest {
    name: [u8; 64],
    len: u32,
    _padding: [u8; 4096 - 4 - 64],
}
#[cfg(feature = "usb")]
impl Default for ConnectRequest {
    fn default() -> Self { ConnectRequest { name: [0u8; 64], len: 0, _padding: [0u8; 4096 - 4 - 64] } }
}

#[cfg(feature = "usb")]
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct UsbString {
    pub s: xous_ipc::String<4000>,
    pub sent: Option<u32>,
}

#[cfg(feature = "usb")]
fn usb_send_str(conn: xous::CID, s: &str) {
    let serializer = UsbString { s: xous_ipc::String::<4000>::from_str(s), sent: None };
    let buf = xous_ipc::Buffer::into_buf(serializer).expect("usb error");
    // failures to send are silent & ignored; also, this API doesn't block.
    buf.send(conn, 8192 /* LogString */).expect("usb error");
}

fn reader_thread(arg: usize) {
    let output = unsafe { &mut *(arg as *mut implementation::OutputWriter) };
    writeln!(output, "LOG: Xous Logging Server starting up...").ok();
    let server_addr = xous::create_server_with_address(b"xous-log-server ").expect("create server");
    writeln!(output, "LOG: Server listening on address {:?}", server_addr).ok();
    #[cfg(feature = "usb")]
    let mut usb_serial: Option<xous::CID> = None;
    // use a stack-allocated string to ensure no heap thrashing results from String manipulations
    #[cfg(feature = "usb")]
    let mut usb_str = xous_ipc::String::<4000>::new();

    println!("LOG: my PID is {}", xous::process::id());
    let mut counter: usize = 0;
    loop {
        if counter.trailing_zeros() >= 12 {
            writeln!(output, "LOG: Counter tick: {}", counter).unwrap();
        }
        counter += 1;
        // writeln!(output, "LOG: Waiting for an event...").unwrap();
        let envelope = xous::syscall::receive_message(server_addr).expect("couldn't get address");
        let sender = envelope.sender;
        if let Some(opcode) = FromPrimitive::from_usize(envelope.body.id()) {
            if let Some(mem) = envelope.body.memory_message() {
                match opcode {
                    api::Opcode::LogRecord => {
                        // This transmute is safe because even if the resulting buffer is garbage,
                        // there are no invalid values in the resulting struct.
                        let lr = unsafe { &*(mem.buf.as_ptr() as *const api::LogRecord) };
                        let level = if log::Level::Error as u32 == lr.level {
                            "ERR "
                        } else if log::Level::Warn as u32 == lr.level {
                            "WARN"
                        } else if log::Level::Info as u32 == lr.level {
                            "INFO"
                        } else if log::Level::Debug as u32 == lr.level {
                            "DBG "
                        } else if log::Level::Trace as u32 == lr.level {
                            "TRCE"
                        } else {
                            "UNKNOWN"
                        };
                        if lr.file_length as usize > lr.file.len() {
                            return;
                        }
                        if lr.args_length as usize > lr.args.len() {
                            return;
                        }
                        if lr.module_length as usize > lr.module.len() {
                            return;
                        }

                        let file_slice = &lr.file[0..lr.file_length as usize];

                        let args_slice = &lr.args[0..lr.args_length as usize];

                        let module_slice = &lr.module[0..lr.module_length as usize];

                        write!(output, "{}:", level).ok();
                        for c in module_slice {
                            output.putc(*c);
                        }
                        write!(output, ": ").ok();
                        for c in args_slice {
                            output.putc(*c);
                        }

                        write!(output, " (").ok();
                        for c in file_slice {
                            output.putc(*c);
                        }
                        if let Some(line) = lr.line {
                            write!(output, ":{}", line.get()).ok();
                        }
                        writeln!(output, ")").ok();
                        #[cfg(feature = "usb")]
                        if let Some(conn) = usb_serial {
                            if usb_str.len() > 0 {
                                // test length so we aren't constantly copying 4096 bytes of 0's clearing an
                                // already cleared structure.
                                usb_str.clear();
                            }
                            // duplicate the above code because doing repeated calls to the USB stack is
                            // inefficient
                            write!(usb_str, "{}:", level).ok();
                            for c in module_slice {
                                usb_str.push_byte(*c).ok();
                            }
                            write!(usb_str, ": ").ok();
                            for c in args_slice {
                                usb_str.push_byte(*c).ok();
                            }
                            write!(usb_str, " (").ok();
                            for c in file_slice {
                                usb_str.push_byte(*c).ok();
                            }
                            if let Some(line) = lr.line {
                                write!(usb_str, ":{}", line.get()).ok();
                            }
                            writeln!(usb_str, ")").ok();
                            usb_send_str(conn, usb_str.to_str());
                        }
                    }
                    api::Opcode::StandardOutput | api::Opcode::StandardError => {
                        // let mut buffer_start_offset = mem.offset.map(|o| o.get()).unwrap_or(0);
                        let mut buffer_start_offset = 0;
                        let mut buffer_length = mem.valid.map(|v| v.get()).unwrap_or(mem.buf.len());

                        // Ensure that `buffer_start_offset` is within the range of `buffer`.
                        if buffer_start_offset >= mem.buf.len() {
                            buffer_start_offset = mem.buf.len() - 1;
                        }

                        // Clamp the buffer length so that it fits within the buffer
                        if buffer_start_offset + buffer_length >= mem.buf.len() {
                            buffer_length = mem.buf.len() - buffer_start_offset;
                        }

                        // Safe because we validated the offsets above
                        let buffer = unsafe {
                            core::slice::from_raw_parts(
                                mem.buf.as_ptr().add(buffer_start_offset),
                                buffer_length,
                            )
                        };
                        for c in buffer {
                            if *c == b'\n' {
                                output.putc(b'\r');
                            }
                            output.putc(*c);
                        }
                        // TODO: If the buffer is mutable, set `length` to 0.

                        #[cfg(feature = "usb")]
                        if let Some(conn) = usb_serial {
                            // safety: this routine will just blow up if you try to pass non utf-8 to it,
                            // so it's not very safe. On the other hand, it's fast and shame on you for
                            // sending non-utf8 to this API.
                            usb_send_str(conn, unsafe { std::str::from_utf8_unchecked(buffer) });
                        }
                    }
                    _ => {
                        writeln!(output, "Unhandled opcode").unwrap();
                    }
                }
            } else if let Some(scalar) = envelope.body.scalar_message() {
                // Scalar message
                let sender_pid = sender.pid().unwrap();
                match scalar.id {
                    1000 => {
                        writeln!(output, "PANIC in PID {}:", sender_pid).unwrap();
                        #[cfg(feature="usb")]
                        if let Some(conn) = usb_serial {
                            usb_send_str(conn, &format!("PANIC in PID {}:", sender_pid));
                        }
                    },
                    1100 => (),
                    1101..=1132 => {
                        let mut output_bfr = [0u8; core::mem::size_of::<usize>() * 4];
                        let output_iter = output_bfr.iter_mut();

                        // Combine the four arguments to form a single
                        // contiguous buffer. Note: The buffer size will change
                        // depending on the platfor's `usize` length.
                        let arg1_bytes = scalar.arg1.to_le_bytes();
                        let arg2_bytes = scalar.arg2.to_le_bytes();
                        let arg3_bytes = scalar.arg3.to_le_bytes();
                        let arg4_bytes = scalar.arg4.to_le_bytes();
                        let input_iter = arg1_bytes
                            .iter()
                            .chain(arg2_bytes.iter())
                            .chain(arg3_bytes.iter())
                            .chain(arg4_bytes.iter());
                        for (dest, src) in output_iter.zip(input_iter) {
                            *dest = *src;
                        }
                        let total_chars = scalar.id - 1100;
                        for (idx, c) in output_bfr.iter().enumerate() {
                            if idx >= total_chars {
                                break;
                            }
                            output.putc(*c);
                        }
                        #[cfg(feature="usb")]
                        // safety: this definitely blows up if you send illegal characters here. But if you're
                        // doing that, we really don't have any mechanism to handle that since this is the panic handler.
                        // Erring on the side of simplicity/"get any message out" versus correctness for this API.
                        if let Some(conn) = usb_serial {
                            usb_send_str(conn, unsafe{std::str::from_utf8_unchecked(&output_bfr[..total_chars])});
                        }
                    }
                    1200 => {
                        writeln!(output, "Terminating process").unwrap();
                        #[cfg(feature="usb")]
                        if let Some(conn) = usb_serial {
                            usb_send_str(conn, "Terminating process");
                        }
                    },
                    2000 => {
                        #[cfg(any(feature="precursor", feature="renode"))]
                        crate::platform::debug::DEFAULT.enable_rx();
                        writeln!(output, "Resuming logger").unwrap();
                    },
                    #[cfg(feature="usb")]
                    4 /* api::Opcode::TryHookUsbMirror */ => {
                        // The hook must be implemented with no dependencies (to avoid circular dependencies on crates).
                        // This this code is somewhat fragile as we copy in the API calls to these functions and assume they do not
                        // change.
                        let xns_conn = xous::connect(xous::SID::from_bytes(b"xous-name-server").unwrap())
                        .expect("Couldn't connect to XousNames");
                        let mut cr: ConnectRequest = Default::default();
                        let name_bytes = b"_Xous USB device driver_";

                        // Set the string length to the length of the passed-in String,
                        // or the maximum possible length. Which ever is smaller.
                        cr.len = cr.name.len().min(name_bytes.len()) as u32;

                        // Copy the string into our backing store.
                        for (&src_byte, dest_byte) in name_bytes.iter().zip(&mut cr.name) {
                            *dest_byte = src_byte;
                        }
                        let msg = xous::MemoryMessage {
                            id: 7 /* TryConnect */,
                            buf: unsafe{ // safety: `cr` is #[repr(C, align(4096))], and should be exactly on page in size
                                xous::MemoryRange::new(&mut cr as *mut _ as *mut u8 as usize, core::mem::size_of::<ConnectRequest>()).unwrap()
                            },
                            offset: None,
                            valid: xous::MemorySize::new(cr.len as usize),
                        };
                        xous::send_message(xns_conn, xous::Message::MutableBorrow(msg)).unwrap();

                        let response_ptr = &cr as *const ConnectRequest as *const u32;
                        let result = unsafe { response_ptr.read() }; // safety: because that's how it was packed on the server, a naked u32

                        if result == 0 {
                            let cid = unsafe { response_ptr.add(1).read() }.into(); // safety: because that's how it was packed on the server
                            writeln!(output, "USB serial connection established, mirroring console output!").ok();
                            usb_serial.replace(cid);
                            xous::return_scalar(envelope.sender, 1).ok();
                        } else {
                            writeln!(output, "USB serial connection failed, console mirror not established").ok();
                            xous::return_scalar(envelope.sender, 0).ok();
                        }
                    },
                    #[cfg(feature="usb")]
                    5 /* api::Opcode::UnhookUsbMirror */ => {
                        // Note: this routine should be coded so that it is never harmful if unhook is called in an already unhooked state.
                        usb_serial.take();
                        xous::return_scalar(envelope.sender, 1).ok();
                    },
                    _ => writeln!(
                        output,
                        "Unrecognized scalar message from {}: {:#?}",
                        sender, scalar
                    )
                    .unwrap(),
                }
            }
        } else {
            writeln!(
                output,
                "Unrecognized opcode from process {}: {}",
                sender.pid().map(|v| v.get()).unwrap_or_default(),
                envelope.body.id()
            )
            .unwrap();
        }
    }
    /* // all cases handled, this loop can never exit
    log::trace!("main loop exit, destroying servers");
    xous::destroy_server(server_addr).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
    */
}

fn main() -> ! {
    /*
    #[cfg(baremetal)]
    {
        // use this to select which UART to monitor in the main loop
        use utralib::generated::*;
        let gpio_base = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::gpio::HW_GPIO_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map GPIO CSR range");
        let mut gpio = CSR::new(gpio_base.as_mut_ptr() as *mut u32);
        gpio.wfo(utra::gpio::UARTSEL_UARTSEL, 1); // 0 = kernel, 1 = log, 2 = app_uart
    }
    */

    let mut output = implementation::init();
    let mut writer = output.get_writer();
    println!("LOG: my PID is {}", xous::process::id());
    println!("LOG: Creating the reader thread");
    xous::create_thread_1(reader_thread, &mut writer as *mut implementation::OutputWriter as usize)
        .expect("create reader thread");
    println!("LOG: Running the output");
    output.run();
    panic!("LOG: Exited");
}
