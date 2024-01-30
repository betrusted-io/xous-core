// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2023 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use core::fmt;

use crate::{
    args::KernelArguments,
    io::{SerialRead, SerialWrite},
};

/// Instance of the shell output.
pub static mut OUTPUT: Option<Output> = None;

/// Shell output.
pub struct Output {
    serial: &'static mut dyn SerialWrite,
    #[cfg(feature = "wrap-print")]
    character_count: usize,
}

impl Output {
    fn new(serial: &'static mut dyn SerialWrite) -> Output {
        Output {
            serial,
            #[cfg(feature = "wrap-print")]
            character_count: 0,
        }
    }
}

impl fmt::Write for Output {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.bytes() {
            #[cfg(feature = "wrap-print")]
            if c == b'\n' {
                self.character_count = 0;
            } else if self.character_count > 80 {
                self.character_count = 0;
                self.serial.putc(b'\n');
                self.serial.putc(b'\r');
                self.serial.putc(b' ');
                self.serial.putc(b' ');
                self.serial.putc(b' ');
                self.serial.putc(b' ');
            } else {
                self.character_count += 1;
            }

            self.serial.putc(c);
        }
        Ok(())
    }
}

/// Initialize the kernel shell.
///
/// This should be called in platform initialization code.
pub fn init(serial: &'static mut dyn SerialWrite) {
    unsafe { OUTPUT = Some(Output::new(serial)) }

    // Print the processed kernel arguments
    let args = KernelArguments::get();
    println!("Kernel arguments:");
    for arg in args.iter() {
        println!("    {}", arg);
    }

    println!("=== Kernel Debug Shell Available ====");
    print_help();
    println!("=====================================");
}

/// Process possible characters received through a serial interface.
///
/// This should be called when a serial interface has new data, for example,
/// on an interrupt.
pub fn process_characters<R: SerialRead>(serial: &mut R) {
    while let Some(b) = serial.getc() {
        println!("> {}", b as char);
        handle_character(b);
    }
}

fn handle_character(b: u8) {
    use crate::services::ArchProcess;

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
        #[cfg(all(baremetal, target_arch = "riscv32"))]
        b'k' => {
            println!("Checking RAM for duplicate pages (this will take a few minutes)");
            crate::mem::MemoryManager::with(|mm| {
                mm.check_for_duplicates();
            });
            println!("Check complete");
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
                system_services.get_process(current_pid).unwrap().activate().unwrap();
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
                            "{:x?} conns:{}/32 {}",
                            process,
                            connection_count,
                            system_services.process_name(process.pid).unwrap_or("")
                        );
                    }
                }
                system_services.get_process(current_pid).unwrap().activate().unwrap();
            });
        }
        b'P' => {
            println!("Printing processes and threads");
            crate::services::SystemServices::with(|system_services| {
                let current_pid = system_services.current_pid();
                for process in &system_services.processes {
                    if !process.free() {
                        println!(
                            "{:x?} {}:",
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
                system_services.get_process(current_pid).unwrap().activate().unwrap();
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
        b'h' => print_help(),
        _ => {}
    }
}

fn print_help() {
    println!("Xous Kernel Debug");
    println!("key | command");
    println!("--- + -----------------------");
    println!(" h  | print this message");
    println!(" i  | print irq handlers");
    #[cfg(all(baremetal, target_arch = "riscv32"))]
    println!(" k  | check RAM to make sure pages are unique");
    println!(" m  | print MMU page tables of all processes");
    println!(" p  | print all processes");
    println!(" P  | print all processes and threads");
    println!(" r  | report RAM usage of all processes");
    println!(" s  | print all allocated servers");
}
