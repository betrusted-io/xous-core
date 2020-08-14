#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod backend;

#[xous::xous_main]
fn xmain() -> ! {
    backend::run();
    loop {};
}
