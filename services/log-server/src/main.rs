#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

#[cfg(any(target_os = "none", target_os = "xous"))]
#[macro_use]
mod debug;

use core::fmt::Write;
use num_traits::FromPrimitive;

#[cfg(not(any(target_os = "none", target_os = "xous")))]
mod implementation {
    use core::fmt::{Error, Write};
    use std::sync::mpsc::{channel, Receiver, Sender};

    enum ControlMessage {
        Text(String),
        Byte(u8),
        Exit,
    }

    pub struct Output {
        tx: Sender<ControlMessage>,
        rx: Receiver<ControlMessage>,
        stdout: std::io::Stdout,
    }

    pub fn init() -> Output {
        let (tx, rx) = channel();

        Output {
            tx,
            rx,
            stdout: std::io::stdout(),
        }
    }

    impl Output {
        pub fn run(&mut self) {
            use std::io::Write;
            loop {
                match self.rx.recv_timeout(std::time::Duration::from_millis(50)) {
                    Ok(msg) => match msg {
                        ControlMessage::Exit => break,
                        ControlMessage::Text(s) => print!("{}", s),
                        ControlMessage::Byte(s) => {
                            let mut handle = self.stdout.lock();
                            handle.write_all(&[s]).unwrap();
                        }
                    },
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(e) => panic!("Error: {}", e),
                }
            }
        }

        pub fn get_writer(&self) -> OutputWriter {
            OutputWriter {
                tx: self.tx.clone(),
            }
        }
    }

    impl Drop for Output {
        fn drop(&mut self) {
            self.tx.send(ControlMessage::Exit).unwrap();
        }
    }

    impl Write for Output {
        fn write_str(&mut self, s: &str) -> Result<(), Error> {
            // It would be nice if this worked with &str
            self.tx.send(ControlMessage::Text(s.to_owned())).unwrap();
            Ok(())
        }
    }

    pub struct OutputWriter {
        tx: Sender<ControlMessage>,
    }

    impl OutputWriter {
        pub fn putc(&self, c: u8) {
            self.tx.send(ControlMessage::Byte(c)).unwrap();
        }

        /// Write a buffer to the output and return the number of
        /// bytes written. This is mostly compatible with `std::io::Write`,
        /// except it is infallible.
        pub fn write(&mut self, buf: &[u8]) -> usize {
            for c in buf {
                self.putc(*c);
            }
            buf.len()
        }

        pub fn write_all(&mut self, buf: &[u8]) -> core::result::Result<usize, ()> {
            Ok(self.write(buf))
        }
    }

    impl Write for OutputWriter {
        fn write_str(&mut self, s: &str) -> Result<(), Error> {
            // It would be nice if this worked with &str
            self.tx.send(ControlMessage::Text(s.to_owned())).unwrap();
            Ok(())
        }
    }
}

#[cfg(any(target_os = "none", target_os = "xous"))]
mod implementation {
    use core::fmt::{Error, Write};
    use utralib::generated::*;

    pub struct Output {}

    pub fn init() -> Output {
        if cfg!(feature = "logging") {
            let uart = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::console::HW_CONSOLE_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map serial port");
            unsafe { crate::debug::DEFAULT_UART_ADDR = uart.as_mut_ptr() as _ };
            println!("Mapped UART @ {:08x}", uart.as_ptr() as usize);
            let mut uart_csr = CSR::new(uart.as_mut_ptr() as *mut u32);

            println!("Process: map success!");

            let inject_mem = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::keyinject::HW_KEYINJECT_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map keyinjection CSR range");
            println!("Note: character injection via console UART is enabled.");

            println!("Allocating IRQ...");
            xous::syscall::claim_interrupt(
                utra::console::CONSOLE_IRQ,
                handle_console_irq,
                inject_mem.as_mut_ptr() as *mut usize,
            )
            .expect("couldn't claim interrupt");
            println!("Claimed IRQ {}", utra::console::CONSOLE_IRQ);
            uart_csr.rmwf(utra::uart::EV_ENABLE_RX, 1);
        }
        Output {}
    }

    impl Output {
        pub fn get_writer(&self) -> OutputWriter {
            OutputWriter {}
        }

        pub fn run(&mut self) {
            loop {
                xous::wait_event();
            }
        }
    }

    fn handle_console_irq(_irq_no: usize, arg: *mut usize) {
        if cfg!(feature = "logging") {
            let mut inject_csr = CSR::new(arg as *mut u32);
            let mut uart_csr = CSR::new(unsafe { crate::debug::DEFAULT_UART_ADDR as *mut u32 });
            // println!("rxe {}", uart_csr.rf(utra::uart::RXEMPTY_RXEMPTY));
            while uart_csr.rf(utra::uart::RXEMPTY_RXEMPTY) == 0 {
                // I really rather think this is more readable, than the "Rusty" version below.
                inject_csr.wfo(utra::keyinject::UART_CHAR_CHAR,
                    uart_csr.rf(utra::uart::RXTX_RXTX));
                uart_csr.wfo(utra::uart::EV_PENDING_RX, 1);

                // I guess this is how you would do it if you were "really doing Rust"
                // (except this is checking pending not fifo status for loop termination)
                // (which was really hard to figure out just looking at this loop)
                /*
                let maybe_c = match uart_csr.rf(utra::uart::EV_PENDING_RX) {
                    0 => None,
                    ack => {
                        let c = Some(uart_csr.rf(utra::uart::RXTX_RXTX) as u8);
                        uart_csr.wfo(utra::uart::EV_PENDING_RX, ack);
                        c
                    }
                };
                if let Some(c) = maybe_c {
                    inject_csr.wfo(utra::keyinject::UART_CHAR_CHAR, (c & 0xff) as u32);
                } else {
                    break;
                }*/
            }
            // println!("rxe {}", uart_csr.rf(utra::uart::RXEMPTY_RXEMPTY));
            // println!("pnd {}", uart_csr.rf(utra::uart::EV_PENDING_RX));
        }
    }

    pub struct OutputWriter {}

    impl OutputWriter {
        pub fn putc(&self, c: u8) {
            if cfg!(feature = "logging") {
                let mut uart_csr = CSR::new(unsafe { crate::debug::DEFAULT_UART_ADDR as *mut u32 });

                // Wait until TXFULL is `0`
                while uart_csr.r(utra::uart::TXFULL) != 0 {}
                uart_csr.wo(utra::uart::RXTX, c as u32);

                // there's a race condition in the handler, if a new character comes in while handling the interrupt,
                // the pending bit never clears. If the console seems to freeze, uncomment this line.
                // This kind of works around that, at the expense of maybe losing some Rx characters.
                // uart_csr.wfo(utra::uart::EV_PENDING_RX, 1);
            }
        }

        /// Write a buffer to the output and return the number of
        /// bytes written. This is mostly compatible with `std::io::Write`,
        /// except it is infallible.
        pub fn write(&mut self, buf: &[u8]) -> usize {
            for c in buf {
                self.putc(*c);
            }
            buf.len()
        }

        pub fn write_all(&mut self, buf: &[u8]) -> core::result::Result<usize, ()> {
            Ok(self.write(buf))
        }
    }

    impl Write for OutputWriter {
        fn write_str(&mut self, s: &str) -> Result<(), Error> {
            for c in s.bytes() {
                self.putc(c);
                if c == '\n' as u8 {
                    self.putc('\r' as u8);
                }
            }
            Ok(())
        }
    }
}

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
            #[cfg(any(target_os = "none", target_os = "xous"))]
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
                let buffer = unsafe { xous_ipc::Buffer::from_memory_message(mem) };
                let lr = unsafe { &*(buffer.as_ptr() as *const LogRecord) };
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
                    write!(output, ":{}", line).ok();
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
    writeln!(output, "LOG: Xous Logging Server starting up...").unwrap();
    let server_addr = xous::create_server_with_address(b"xous-log-server ").unwrap();
    writeln!(output, "LOG: Server listening on address {:?}", server_addr).unwrap();

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

#[xous::xous_main]
fn some_main() -> ! {
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
    .unwrap();
    println!("LOG: Running the output");
    output.run();
    panic!("LOG: Exited");
}
