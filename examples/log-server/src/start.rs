
/// Rust entry point (_start_rust)
///
/// Zeros bss section, initializes data section and calls main. This function
/// never returns.
#[link_section = ".init.rust"]
#[export_name = "_start"]
pub unsafe extern "C" fn start_rust() -> ! {
    extern "Rust" {
        // This symbol will be provided by the kernel
        fn main();
    }
    main();
    panic!("exited main");
}

#[export_name = "abort"]
pub extern "C" fn abort() -> ! {
    panic!("aborted");
}
