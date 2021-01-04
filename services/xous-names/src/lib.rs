#![cfg_attr(target_os = "none", no_std)]

pub mod api;

/*
Calls are made using the xous::ipc::Sendable IPC wrapper, for example:

    use xous::ipc::*;

    let mut registration = Registration::new();
    let mut sendable_registration = Sendable::new(registration)
        .expect("can't create sendable registration structure");

    let test_name = b"A test Name!";
    sendable_registration.name[..test_name.len()].clone_from_slice(test_name);
    sendable_registration.name_len = test_name.len();

    sendable_registration.lend_mut(ns_conn, sendable_registration.mid());

*/
