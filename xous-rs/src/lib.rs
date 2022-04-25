#![cfg_attr(any(target_os = "none", target_os = "xous"), no_std)]

#[cfg(feature = "bitflags")]
#[macro_use]
extern crate bitflags;

#[cfg(not(feature = "rustc-dep-of-std"))]
extern crate xous_macros as macros;
#[cfg(not(feature = "rustc-dep-of-std"))]
pub use macros::xous_main;

pub mod arch;

pub mod carton;
pub mod definitions;

pub mod process;
pub mod string;
pub mod stringbuffer;
pub mod syscall;

pub use arch::{ProcessArgs, ProcessInit, ProcessKey, ThreadInit};
pub use definitions::*;
pub use string::*;
pub use stringbuffer::*;
pub use syscall::*;

pub mod locale;
pub use locale::LANG;

#[cfg(feature = "processes-as-threads")]
pub use crate::arch::ProcessArgsAsThread;

#[cfg(any(target_os = "none", target_os = "xous"))]
pub fn init() {}

#[cfg(not(any(target_os = "none", target_os = "xous")))]
pub fn init() {
    use std::panic;
    panic::set_hook(Box::new(|arg| {
        println!("PANIC!");
        println!("Details: {:?}", arg);
        debug_here::debug_here!();
    }));
}

/// Convert a four-letter string into a 32-bit int.
#[macro_export]
macro_rules! make_name {
    ($fcc:expr) => {{
        let mut c: [u8; 4] = Default::default();
        c.copy_from_slice($fcc.as_bytes());
        u32::from_le_bytes(c) as usize
    }};
}

#[cfg(not(target_os = "none"))]
#[macro_export]
macro_rules! maybe_main {
    () => {
        extern "Rust" {
            fn xous_entry() -> !;
        }

        fn main() {
            #[cfg(not(target_os = "xous"))]
            xous::arch::set_thread_id(1);
            unsafe { xous_entry() };
        }
    };
}

#[cfg(target_os = "none")]
#[macro_export]
macro_rules! maybe_main {
    () => {
        use core::panic::PanicInfo;

        #[panic_handler]
        fn handle_panic(arg: &PanicInfo) -> ! {
            use core::fmt::Write;
            use xous::{
                terminate_process, try_connect, try_send_message, wait_event, Message,
                ScalarMessage, CID, SID,
            };

            // Try to connect to the log server. If this fails, we won't be able to print
            // anything anyway.
            // We use `try_connect()` here because we want this to work even during an interrupt handler.
            // If we've already connected to the log server, then the kernel will reuse the
            // connection number.
            if let Ok(conn) = try_connect(SID::from_bytes(b"xous-log-server ").unwrap()) {
                struct PanicWriter {
                    conn: CID,
                }
                impl PanicWriter {
                    // Group `usize` bytes into a `usize` and return it, beginning
                    // from `offset` * sizeof(usize) bytes from the start. For example,
                    // `group_or_null([1,2,3,4,5,6,7,8], 1)` on a 32-bit system will
                    // return a usize with 5678 packed into it.
                    fn group_or_null(data: &[u8], offset: usize) -> usize {
                        let start = offset * core::mem::size_of::<usize>();
                        let mut out_array = [0u8; core::mem::size_of::<usize>()];
                        for i in 0..core::mem::size_of::<usize>() {
                            out_array[i] = if i + start < data.len() {
                                data[start + i]
                            } else {
                                0
                            };
                        }
                        usize::from_le_bytes(out_array)
                    }
                }
                impl core::fmt::Write for PanicWriter {
                    fn write_str(&mut self, s: &str) -> core::result::Result<(), core::fmt::Error> {
                        for c in s.as_bytes().chunks(core::mem::size_of::<usize>() * 4) {
                            // Text is grouped into 4x `usize` words. The id is 1100 plus
                            // the number of characters in this message.
                            let mut panic_msg = ScalarMessage {
                                id: 1100 + c.len(),
                                arg1: Self::group_or_null(&c, 0),
                                arg2: Self::group_or_null(&c, 1),
                                arg3: Self::group_or_null(&c, 2),
                                arg4: Self::group_or_null(&c, 3),
                            };
                            try_send_message(self.conn, Message::Scalar(panic_msg)).ok();
                        }
                        Ok(())
                    }
                }

                let mut pw = PanicWriter { conn };
                // Send the "We're panicking" message (1000).
                let panic_start_msg = ScalarMessage {
                    id: 1000,
                    arg1: 0,
                    arg2: 0,
                    arg3: 0,
                    arg4: 0,
                };
                try_send_message(conn, Message::Scalar(panic_start_msg)).ok();

                // Send the contents of the panic.
                writeln!(&mut pw, "{}", arg).ok();

                // Send the "We're done panicking now it's time to quit" message (1200)
                let panic_start_msg = ScalarMessage {
                    id: 1200,
                    arg1: 0,
                    arg2: 0,
                    arg3: 0,
                    arg4: 0,
                };
                try_send_message(conn, Message::Scalar(panic_start_msg)).ok();
            }
            wait_event();
            terminate_process(1);
        }

        extern "Rust" {
            fn xous_entry() -> !;
        }

        #[export_name = "_start"]
        pub extern "C" fn _start(pid: u32) {
            xous::process::set_id(pid);
            unsafe { xous_entry() };
        }
    };
}
