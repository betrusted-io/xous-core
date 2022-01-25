// NOTE: Adapted from cortex-m/build.rs
use std::env;

fn main() {
    let target = env::var("TARGET").unwrap();

    let target_os = target.split('-').nth(2).unwrap_or("none");

    // If we're not running on a desktop-class operating system, emit the "baremetal"
    // config setting. This will enable software to do tasks such as
    // managing memory.
    if target_os == "none" {
        println!("cargo:rustc-cfg=baremetal");
    }

    // BUILD_TIMESTAMP doesn't work -- it doesn't update because of the below line
    // removing the below line causes a lengthy full-rebuild just to capture a timestamp.
    // so, we're removing the timestamp.
    println!("cargo:rerun-if-changed=build.rs");
}
