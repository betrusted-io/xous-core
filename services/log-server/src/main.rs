#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

#[cfg(baremetal)]
#[macro_use]
mod debug;

use core::fmt::Write;
use xous_ipc::String;

#[cfg(not(target_os = "none"))]
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
    }

    impl Write for OutputWriter {
        fn write_str(&mut self, s: &str) -> Result<(), Error> {
            // It would be nice if this worked with &str
            self.tx.send(ControlMessage::Text(s.to_owned())).unwrap();
            Ok(())
        }
    }
}

#[cfg(target_os = "none")]
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
            println!("Mapped UART @ {:08x}", uart.addr.get());

            println!("Process: map success!");
            crate::debug::DEFAULT.enable_rx();

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
                handle_irq,
                inject_mem.as_mut_ptr() as *mut usize,
            )
            .expect("couldn't claim interrupt");
            println!("Claimed IRQ {}", utra::console::CONSOLE_IRQ);
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

    fn handle_irq(_irq_no: usize, arg: *mut usize) {
        if cfg!(feature = "logging") {
            let mut inject_csr = CSR::new(arg as *mut u32);
            //print!("Handling IRQ {} (arg: {:08x}): ", irq_no, arg as usize);

            while let Some(c) = crate::debug::DEFAULT.getc() {
                //print!("0x{:02x} ", c);
                inject_csr.wfo(utra::keyinject::UART_CHAR_CHAR, c as u32);
            }
            println!();
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
            }
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
        1000 => writeln!(output, "PANIC from PID {} | {}", sender_pid, sender).unwrap(),
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
            crate::debug::DEFAULT.enable_rx();
            writeln!(output, "Resuming logger").unwrap();
        },
        _ => writeln!(
            output,
            "Unrecognized scalar message from {}: {:#?}",
            sender, msg
        )
        .unwrap(),
    }
}

fn reader_thread(arg: usize) {
    let mut output = unsafe { &mut *(arg as *mut implementation::OutputWriter) };
    writeln!(output, "LOG: Xous Logging Server starting up...").unwrap();

    writeln!(output, "LOG: ****************************************************************").unwrap();
    writeln!(output, "LOG: *** Welcome to Xous {:40} ***", env!("VERGEN_SHA")).unwrap();
    // time stamp isn't actually the time stamp of the build, unfortunately. It's the time stamp of the
    // last time you managed to force a rebuild that also causes log-server to be rebuilt, not necessarily
    // capturing the build time of the very most recent change!
    // writeln!(output, "LOG: *** Built: {:49} ***", env!("VERGEN_BUILD_TIMESTAMP")).unwrap();
    writeln!(output, "LOG: ****************************************************************").unwrap();
    println!("LOG: my PID is {}", xous::process::id());
    let server_addr = xous::create_server_with_address(b"xous-log-server ").unwrap();
    writeln!(output, "LOG: Server listening on address {:?}", server_addr).unwrap();

    let mut counter: usize = 0;
    loop {
        if counter.trailing_zeros() >= 12 {
            writeln!(output, "LOG: Counter tick: {}", counter).unwrap();
        }
        counter += 1;
        // writeln!(output, "LOG: Waiting for an event...").unwrap();
        let mut envelope =
            xous::syscall::receive_message(server_addr).expect("couldn't get address");
        let sender = envelope.sender;
        // writeln!(output, "LOG: Got message envelope: {:?}", envelope).unwrap();
        match &mut envelope.body {
            xous::Message::Scalar(msg) =>
                handle_scalar(&mut output, sender, msg, envelope.sender.pid().unwrap()),
            xous::Message::BlockingScalar(msg) => {
                writeln!(
                    output,
                    "LOG: BlockingScalar message from {}: {:?}",
                    envelope.sender, msg
                )
                .unwrap();
            }
            xous::Message::Move(msg) => {
                String::<4000>::from_message(msg)
                    .map(|log_entry: String<4000>| {
                        writeln!(
                            output,
                            "LOG: Moved log  message from {}: {}",
                            sender, log_entry
                        )
                        .unwrap()
                    })
                    .or_else(|e| {
                        writeln!(output, "LOG: unable to convert Move message to str: {}", e)
                    })
                    .ok();
            }
            xous::Message::Borrow(_msg) => {
                let mem = envelope.body.memory_message().unwrap();
                let buffer = unsafe { xous_ipc::Buffer::from_memory_message(mem) };
                let lr: LogRecord = buffer.to_original::<LogRecord, _>().unwrap();
                let level =
                    if log::Level::Error as u32 == lr.level {
                        "ERR " }
                    else if log::Level::Warn as u32 == lr.level {
                        "WARN" }
                    else if log::Level::Info as u32 == lr.level {
                        "INFO" }
                    else if log::Level::Debug as u32 == lr.level {
                        "DBG " }
                    else if log::Level::Trace as u32 == lr.level {
                        "TRCE" }
                    else {
                        "UNKNOWN"
                    };
                if let Some(line)= lr.line {
                    writeln!(output, "{}:{}: {} ({}:{})",
                    level, lr.module, lr.args, lr.file, line
                    ).unwrap();
                } else {
                    writeln!(output, "{}:{}: {} ({})",
                    level, lr.module, lr.args, lr.file
                    ).unwrap();
                }
            }
            xous::Message::MutableBorrow(msg) => {
                String::<4000>::from_message(msg)
                    .map(|mut log_entry: String<4000>| {
                        writeln!(
                            output,
                            "LOG: Mutable borrowed log message from {} len {}:\n\r  {}\n\r",
                            sender,
                            log_entry.len(),
                            log_entry,
                        )
                        .unwrap();
                        writeln!(log_entry, " << HELLO FROM THE SERVER").unwrap();
                    })
                    .or_else(|e| {
                        writeln!(
                            output,
                            "LOG: unable to convert MutableBorrow message to str: {}",
                            e
                        )
                    })
                    .ok();
            }
        }
    }
    /* // all cases handled, this loop can never exit
    log::trace!("main loop exit, destroying servers");
    xous::destroy_server(server_addr).unwrap();
    log::trace!("quitting");
    xous::terminate_process(); loop {}
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
    xous::create_thread_1(reader_thread, &mut writer as *mut implementation::OutputWriter as usize).unwrap();
    println!("LOG: Running the output");
    output.run();
    panic!("LOG: Exited");
}
