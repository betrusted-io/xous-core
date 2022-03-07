extern crate cc;

use std::env::set_var;

fn main() {
    set_var("CC", "riscv64-unknown-elf-gcc");  // set the compiler to what's installed on the system

    let espeak_srcs = vec![
        "ffi/ffi.c",
        "ffi/libc.c",
        "ffi/scanf.c",
    ];
    let espeak_includes = vec![
        "./",
        // "espeak-ng/src",
        // "espeak-ng/src/include/compat",
        // "espeak-ng/src/include/espeak",
        // "espeak-ng/src/include/espeak-ng",
        // "espeak-ng/src/ucd-tools/src/include",
        // "espeak-ng/src/include",
    ];

	let mut base_config = cc::Build::new();
    base_config.target("riscv32imac-unknown-none-elf");

    for inc in espeak_includes {
        base_config.include(inc);
    }
    base_config.include("C:\\Users\\bunnie\\riscv64\\riscv64-unknown-elf\\include");
    base_config.include("C:\\Users\\bunnie\\riscv64\\riscv64-unknown-elf\\include\\sys");
    base_config.include("C:\\Users\\bunnie\\riscv64\\riscv64-unknown-elf\\include\\machine");
    base_config.include("C:\\Users\\bunnie\\riscv64\\riscv64-unknown-elf\\include\\bits");

    for src in espeak_srcs {
        base_config.file(src);
    }
    base_config.define("XOUS", None);
    base_config.define("NO_STD", None);
	base_config.compile("libffi.a");
}
