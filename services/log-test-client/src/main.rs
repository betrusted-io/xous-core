#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use core::fmt::Write;

use xous::StringBuffer;

fn main() -> ! {
    let connection = xous::connect(xous::SID::from_bytes(b"xous-log-server ").unwrap()).unwrap();

    let mut log_string = StringBuffer::new();
    for i in 0.. {
        log_string.clear();
        writeln!(log_string, "Hello, world! Loop {}", i).unwrap();
        log_string.lend(connection, 1).unwrap();

        log_string.lend_mut(connection, 1).unwrap();
        log_string.lend(connection, 1).unwrap();
        xous::yield_slice();
    }
    panic!("Finished an infinite loop");
}
