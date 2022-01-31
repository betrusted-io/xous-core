use vergen::{ConstantsFlags, generate_cargo_keys};

fn main() {
    // Setup the flags, toggling off the 'SEMVER_FROM_CARGO_PKG' flag
    let flags = ConstantsFlags::all();

    // Generate the 'cargo:' key output
    generate_cargo_keys(flags).expect("Unable to generate the cargo keys!");

    // touch the log server to force it to pick up the version
    let ts = filetime::FileTime::from_system_time(std::time::SystemTime::now());
    filetime::set_file_mtime("../services/log-server/src/main.rs", ts).unwrap();

    // note that shellchat won't pick up the vesion, but because it's a big crate to rebuild we aren't re-triggering it
}
