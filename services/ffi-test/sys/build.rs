extern crate cc;

use std::env::set_var;

fn main() {
    set_var("CC", "riscv64-unknown-elf-gcc");  // set the compiler to what's installed on the system

    let ffi_srcs = vec![
        "ffi/ffi.c",
        "ffi/libc.c",
        "ffi/libc_split.c",
        "ffi/scanf.c",
    ];
    let ffi_includes = vec![
        "./",
    ];

	let mut base_config = cc::Build::new();
    base_config.target("riscv32imac-unknown-none-elf");

    for inc in ffi_includes {
        base_config.include(inc);
    }

    for src in ffi_srcs {
        base_config.file(src);
    }
    base_config.define("XOUS", None);
    base_config.define("NO_STD", None);
	base_config.compile("libffi.a");
}
