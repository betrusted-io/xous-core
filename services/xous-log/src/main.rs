#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use xous_api_log::api;

#[cfg(any(feature="precursor", feature="renode"))]
#[macro_use]
mod platform;

use core::fmt::Write;
use num_traits::FromPrimitive;

use platform::implementation;

fn handle_scalar(
    output: &mut implementation::OutputWriter,
    sender: xous::MessageSender,
    msg: &xous::ScalarMessage,
    sender_pid: xous::PID,
) {
    match msg.id {
        1000 => writeln!(output, "PANIC in PID {}:", sender_pid).unwrap(),
        1100 => (),
        1101..=1132 => {
            let mut output_bfr = [0u8; core::mem::size_of::<usize>() * 4];
            let output_iter = output_bfr.iter_mut();

            // Combine the four arguments to form a single
            // contiguous buffer. Note: The buffer size will change
            // depending on the platfor's `usize` length.
            let arg1_bytes = msg.arg1.to_le_bytes();
            let arg2_bytes = msg.arg2.to_le_bytes();
            let arg3_bytes = msg.arg3.to_le_bytes();
            let arg4_bytes = msg.arg4.to_le_bytes();
            let input_iter = arg1_bytes
                .iter()
                .chain(arg2_bytes.iter())
                .chain(arg3_bytes.iter())
                .chain(arg4_bytes.iter());
            for (dest, src) in output_iter.zip(input_iter) {
                *dest = *src;
            }
            let total_chars = msg.id - 1100;
            for (idx, c) in output_bfr.iter().enumerate() {
                if idx >= total_chars {
                    break;
                }
                output.putc(*c);
            }
        }
        1200 => writeln!(output, "Terminating process").unwrap(),
        2000 => {
            #[cfg(any(feature="precursor", feature="renode"))]
            crate::debug::DEFAULT.enable_rx();
            writeln!(output, "Resuming logger").unwrap();
        }
        _ => writeln!(
            output,
            "Unrecognized scalar message from {}: {:#?}",
            sender, msg
        )
        .unwrap(),
    }
}

fn handle_opcode(
    output: &mut implementation::OutputWriter,
    sender: xous::MessageSender,
    opcode: api::Opcode,
    message: &xous::Message,
) {
    if let Some(mem) = message.memory_message() {
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
                output.write_all(buffer).unwrap();
                // TODO: If the buffer is mutable, set `length` to 0.
            }
            _ => {
                writeln!(output, "Unhandled opcode").unwrap();
            }
        }
    } else if let Some(scalar) = message.scalar_message() {
        // Scalar message
        handle_scalar(output, sender, scalar, sender.pid().unwrap());
    }
}

fn reader_thread(arg: usize) {
    let output = unsafe { &mut *(arg as *mut implementation::OutputWriter) };
    writeln!(output, "LOG: Xous Logging Server starting up...").ok();
    let server_addr = xous::create_server_with_address(b"xous-log-server ").expect("create server");
    writeln!(output, "LOG: Server listening on address {:?}", server_addr).ok();

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
            handle_opcode(output, sender, opcode, &envelope.body);
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
    xous::create_thread_1(
        reader_thread,
        &mut writer as *mut implementation::OutputWriter as usize,
    )
    .expect("create reader thread");
    println!("LOG: Running the output");
    output.run();
    panic!("LOG: Exited");
}
