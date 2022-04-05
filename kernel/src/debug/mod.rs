// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

#[cfg(baremetal)]
use core::fmt::{Error, Write};
#[cfg(baremetal)]
use utralib::generated::*;

#[cfg(baremetal)]
pub static mut DEBUG_OUTPUT: Option<&'static mut dyn Write> = None;

pub use crate::arch::process::Process as ArchProcess;

#[macro_use]
#[cfg(all(
    not(test),
    baremetal,
    any(feature = "debug-print", feature = "print-panics")
))]
pub mod debug_print_hardware {
    // the HW device mapping is done in main.rs/init(); the virtual address has to be in the top 4MiB as it is the only page shared among all processes
    pub const SUPERVISOR_UART_ADDR: *mut usize = 0xffcf_0000 as *mut usize; // see https://github.com/betrusted-io/xous-core/blob/master/docs/memory.md
}
#[cfg(all(
    not(test),
    baremetal,
    any(feature = "debug-print", feature = "print-panics")
))]
pub use crate::debug::debug_print_hardware::SUPERVISOR_UART_ADDR;

#[cfg(baremetal)]
#[macro_export]
macro_rules! print {
    ($($args:tt)+) => {{
        #[allow(unused_unsafe)]
        unsafe {
            if let Some(mut stream) = crate::debug::DEBUG_OUTPUT.as_mut() {
                write!(&mut stream, $($args)+).unwrap();
            }
        }
    }};
}

#[cfg(baremetal)]
#[macro_export]
macro_rules! println
{
	() => ({
		print!("\r\n")
	});
	($fmt:expr) => ({
		print!(concat!($fmt, "\r\n"))
	});
	($fmt:expr, $($args:tt)+) => ({
		print!(concat!($fmt, "\r\n"), $($args)+)
	});
}

#[cfg(baremetal)]
pub struct Uart {}
#[cfg(baremetal)]
pub static mut UART: Uart = Uart {};

#[cfg(all(baremetal, feature = "wrap-print"))]
static mut CHAR_COUNT: usize = 0;

#[cfg(baremetal)]
impl Uart {
    #[allow(dead_code)]
    pub fn init(self) {
        unsafe { DEBUG_OUTPUT = Some(&mut UART) };
        let mut uart_csr = CSR::new(crate::debug::SUPERVISOR_UART_ADDR as *mut u32);
        uart_csr.rmwf(utra::uart::EV_ENABLE_RX, 1);
    }

    pub fn putc(&self, c: u8) {
        if unsafe { DEBUG_OUTPUT.is_none() } {
            return;
        }

        let mut uart_csr = CSR::new(crate::debug::SUPERVISOR_UART_ADDR as *mut u32);
        // Wait until TXFULL is `0`
        while uart_csr.r(utra::uart::TXFULL) != 0 {}
        #[cfg(feature = "wrap-print")]
        unsafe {
            if c == b'\n' {
                CHAR_COUNT = 0;
            } else if CHAR_COUNT > 80 {
                CHAR_COUNT = 0;
                self.putc(b'\n');
                self.putc(b'\r');
                self.putc(b' ');
                self.putc(b' ');
                self.putc(b' ');
                self.putc(b' ');
            } else {
                CHAR_COUNT += 1;
            }
        }
        uart_csr.wfo(utra::uart::RXTX_RXTX, c as u32);
    }

    #[allow(dead_code)]
    pub fn getc(&self) -> Option<u8> {
        if unsafe { DEBUG_OUTPUT.is_none() } {
            return None;
        }
        let mut uart_csr = CSR::new(crate::debug::SUPERVISOR_UART_ADDR as *mut u32);
        // If EV_PENDING_RX is 1, return the pending character.
        // Otherwise, return None.
        match uart_csr.rf(utra::uart::EV_PENDING_RX) {
            0 => None,
            _ => {
                let ret = Some(uart_csr.r(utra::uart::RXTX) as u8);
                uart_csr.wfo(utra::uart::EV_PENDING_RX, 1);
                ret
            }
        }
    }
}

#[cfg(all(feature = "gdbserver", baremetal))]
mod gdb_server;

#[cfg(all(feature = "gdbserver", baremetal))]
impl gdbstub::Connection for Uart {
    type Error = ();

    fn write(&mut self, byte: u8) -> Result<(), Self::Error> {
        if unsafe { DEBUG_OUTPUT.is_none() } {
            return Err(());
        }
        let mut uart_csr = CSR::new(crate::debug::SUPERVISOR_UART_ADDR as *mut u32);
        // Wait until TXFULL is not `0`
        while uart_csr.r(utra::uart::TXFULL) != 0 {}
        uart_csr.wo(utra::uart::RXTX, byte as u32);
        Ok(())
    }
    fn peek(&mut self) -> Result<Option<u8>, Self::Error> {
        Ok(None)
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[cfg(all(
    baremetal,
    not(test),
    any(feature = "debug-print", feature = "print-panics")
))]
pub fn irq(_irq_number: usize, _arg: *mut usize) {
    let uart = Uart {};
    while let Some(b) = uart.getc() {
        process_irq_character(b);
    }
    // uart.acknowledge_irq();
}

#[cfg(all(
    baremetal,
    not(test),
    any(feature = "debug-print", feature = "print-panics")
))]
fn process_irq_character(b: u8) {
    #[cfg(all(feature = "gdbserver", baremetal))]
    if gdb_server::handle(b) {
        return;
    }

    match b {
        b'i' => {
            println!("Interrupt handlers:");
            println!("  IRQ | Process | Handler | Argument");
            crate::services::SystemServices::with(|system_services| {
                crate::irq::for_each_irq(|irq, pid, address, arg| {
                    println!(
                        "    {}:  {} @ {:x?} {:x?}",
                        irq,
                        system_services.process_name(*pid).unwrap_or(""),
                        address,
                        arg
                    );
                });
            });
        }
        b'm' => {
            println!("Printing memory page tables");
            crate::services::SystemServices::with(|system_services| {
                let current_pid = system_services.current_pid();
                for process in &system_services.processes {
                    if !process.free() {
                        println!(
                            "PID {} {}:",
                            process.pid,
                            system_services.process_name(process.pid).unwrap_or("")
                        );
                        process.activate().unwrap();
                        crate::arch::mem::MemoryMapping::current().print_map();
                        println!();
                    }
                }
                system_services
                    .get_process(current_pid)
                    .unwrap()
                    .activate()
                    .unwrap();
            });
        }
        b'p' => {
            println!("Printing processes");
            crate::services::SystemServices::with(|system_services| {
                let current_pid = system_services.current_pid();
                for process in &system_services.processes {
                    if !process.free() {
                        process.activate().unwrap();
                        let mut connection_count = 0;
                        ArchProcess::with_inner(|process_inner| {
                            for conn in &process_inner.connection_map {
                                if conn.is_some() {
                                    connection_count += 1;
                                }
                            }
                        });
                        println!(
                            "{:?} conns:{}/32 {}",
                            process,
                            connection_count,
                            system_services.process_name(process.pid).unwrap_or("")
                        );
                    }
                }
                system_services
                    .get_process(current_pid)
                    .unwrap()
                    .activate()
                    .unwrap();
            });
        }
        b'P' => {
            println!("Printing processes and threads");
            crate::services::SystemServices::with(|system_services| {
                let current_pid = system_services.current_pid();
                for process in &system_services.processes {
                    if !process.free() {
                        println!(
                            "{:?} {}:",
                            process,
                            system_services.process_name(process.pid).unwrap_or("")
                        );
                        process.activate().unwrap();
                        crate::arch::process::Process::with_current_mut(|arch_process| {
                            arch_process.print_all_threads()
                        });
                        println!();
                    }
                }
                system_services
                    .get_process(current_pid)
                    .unwrap()
                    .activate()
                    .unwrap();
            });
        }
        b'r' => {
            println!("RAM usage:");
            let mut total_bytes = 0;
            crate::services::SystemServices::with(|system_services| {
                crate::mem::MemoryManager::with(|mm| {
                    for process in &system_services.processes {
                        if !process.free() {
                            let bytes_used = mm.ram_used_by(process.pid);
                            total_bytes += bytes_used;
                            println!(
                                "    PID {:>3}: {:>4} k {}",
                                process.pid,
                                bytes_used / 1024,
                                system_services.process_name(process.pid).unwrap_or("")
                            );
                        }
                    }
                });
            });
            println!("{} k total", total_bytes / 1024);
        }
        b's' => {
            println!("Servers in use:");
            crate::services::SystemServices::with(|system_services| {
                println!(" idx | pid | process              | sid");
                println!(" --- + --- + -------------------- | ------------------");
                for (idx, server) in system_services.servers.iter().enumerate() {
                    if let Some(s) = server {
                        println!(
                            " {:3} | {:3} | {:20} | {:x?}",
                            idx,
                            s.pid,
                            system_services.process_name(s.pid).unwrap_or(""),
                            s.sid
                        );
                    }
                }
            });
        }
        #[cfg(all(feature = "gdbserver", baremetal))]
        b'g' => {
            println!("Starting GDB server -- attach your debugger now");
            gdb_server::setup();
        }
        b'h' => {
            println!("Xous Kernel Debug");
            println!("key | command");
            println!("--- + -----------------------");
            #[cfg(all(feature = "gdbserver", baremetal))]
            println!(" g  | enter the gdb server");
            println!(" i  | print irq handlers");
            println!(" m  | print MMU page tables of all processes");
            println!(" p  | print all processes");
            println!(" P  | print all processes and threads");
            println!(" r  | report RAM usage of all processes");
            println!(" s  | print all allocated servers");
        }
        _ => {}
    }
}

#[cfg(baremetal)]
impl Write for Uart {
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        for c in s.bytes() {
            self.putc(c);
        }
        Ok(())
    }
}

#[cfg(feature = "debug-print")]
#[macro_export]
macro_rules! klog
{
	() => ({
		print!(" [{}:{}]", file!(), line!())
	});
	($fmt:expr) => ({
        print!(concat!(" [{}:{} ", $fmt, "]"), file!(), line!())
	});
	($fmt:expr, $($args:tt)+) => ({
		print!(concat!(" [{}:{} ", $fmt, "]"), file!(), line!(), $($args)+)
	});
}

#[cfg(not(feature = "debug-print"))]
#[macro_export]
macro_rules! klog {
    ($($args:tt)+) => {{}};
}
