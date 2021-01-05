#![cfg_attr(target_os = "none", no_std)]

pub mod api;

/*
Calls are made using the xous::ipc::Sendable IPC wrapper, for example:

    use xous::ipc::*;

    let mut registration = Registration::new();
    let mut sendable_registration = Sendable::new(registration)
        .expect("can't create sendable registration structure");

    write!(sendable_registration.name, "I'm a server, call me Fred!");

    sendable_registration.lend_mut(ns_conn, sendable_registration.mid());

*/
