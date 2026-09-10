//! errors theme — io::Error kinds and error-path contracts of std::net.
//!
//! Error KINDS on xous are a property of the (rustc 1.96.1 xous std,
//! xous-core dev) toolchain pair, not of POSIX: every kind assertion below is
//! pinned to that pair and a toolchain move re-opens all of them.

use core::mem::ManuallyDrop;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::harness::{XorShift, bounded, check, discard, next_port, self_ip};
use crate::tests::{TestEntry, XfailEntry};

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// Binding a TcpListener to a foreign address (1.1.1.1) fails with
/// AddrNotAvailable: the server whitelists 0.0.0.0/127.0.0.1/DUT-IP and
/// answers anything else with Invalid, which the bind decode maps to that kind.
pub fn tcp_bind_foreign_addr_kind() {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), next_port());
    match TcpListener::bind(addr) {
        Ok(l) => {
            discard(l);
            panic!("bind to the foreign address {} unexpectedly succeeded", addr);
        }
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::AddrNotAvailable,
            "foreign-address bind surfaced as {:?} ({}), want AddrNotAvailable",
            e.kind(),
            e
        ),
    }
}

/// Connecting to the unspecified address 0.0.0.0 fails with AddrNotAvailable:
/// smoltcp rejects it up front and the connect decode maps every connect
/// failure to that kind, inside the contract's allowed set.
pub fn tcp_connect_unspecified_addr_kind() {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), next_port());
    let result = bounded("connect to 0.0.0.0", 10, move || TcpStream::connect(addr));
    match result {
        Ok(s) => {
            discard(s);
            panic!("connect to the unspecified address {} unexpectedly succeeded", addr);
        }
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::AddrNotAvailable,
            "connect to 0.0.0.0 surfaced as {:?} ({}), want AddrNotAvailable",
            e.kind(),
            e
        ),
    }
}

/// accept() on a nonblocking listener with no pending connection returns
/// WouldBlock: the server's nonblocking-accept path writes the code at byte 1,
/// where the pinned client's accept-error decode reads it.
pub fn tcp_nonblocking_accept_wouldblock() {
    let addr = SocketAddr::new(LOOPBACK, next_port());
    let listener = check!(TcpListener::bind(addr));
    check!(listener.set_nonblocking(true));
    // guarded: if the nonblocking flag were dropped on the wire, accept would
    // park forever (accept has no timeout surface, TO-12)
    let (result, listener) = bounded("nonblocking accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    discard(listener);
    match result {
        Ok((s, peer)) => {
            discard(s);
            panic!("nonblocking accept with no pending connection returned a stream from {}", peer);
        }
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::WouldBlock,
            "nonblocking accept surfaced as {:?} ({}), want WouldBlock",
            e.kind(),
            e
        ),
    }
}

/// peek() on a nonblocking stream with an empty rx queue returns WouldBlock
/// immediately, then sees the byte once the peer writes: TCP rx/peek errors
/// mirror the code at bytes 1 and 4, so the client's rx decode reads it.
pub fn tcp_nonblocking_peek_wouldblock() {
    let addr = SocketAddr::new(LOOPBACK, next_port());
    let listener = check!(TcpListener::bind(addr));
    let client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let (mut served, _peer) = check!(accepted);
    check!(client.set_nonblocking(true));
    // nonblocking peeks return immediately, so they are safe on the test thread
    let started = Instant::now();
    let mut buf = [0u8; 8];
    let first = client.peek(&mut buf);
    let first_elapsed = started.elapsed();
    let write_res = served.write_all(&[0x5a]);
    // poll for the byte; collect the outcome, defer the verdict until after
    // the sockets are discarded
    let mut polled = None;
    let mut tries = 0u32;
    while tries < 20 {
        match client.peek(&mut buf) {
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                tries += 1;
                if tries % 5 == 0 {
                    log::info!("nonblocking peek poll {}: still WouldBlock", tries);
                }
                thread::sleep(Duration::from_millis(100));
            }
            other => {
                polled = Some(other);
                break;
            }
        }
    }
    let got = buf[0];
    discard(client);
    discard(served);
    discard(listener);
    match first {
        Ok(n) => panic!("nonblocking peek with no pending data returned Ok({})", n),
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::WouldBlock,
            "nonblocking peek surfaced as {:?} ({}), want WouldBlock",
            e.kind(),
            e
        ),
    }
    assert!(
        first_elapsed < Duration::from_secs(1),
        "nonblocking peek took {:?}, want immediate",
        first_elapsed
    );
    check!(write_res);
    match polled {
        Some(Ok(1)) => assert_eq!(got, 0x5a, "wrong byte after nonblocking peek poll"),
        Some(Ok(n)) => panic!("nonblocking peek returned Ok({}), want exactly 1 byte", n),
        Some(Err(e)) => panic!("nonblocking peek failed: {}", e),
        None => panic!("data never arrived within 20 nonblocking peeks"),
    }
}

/// recv_from() on a nonblocking UDP socket with no queued datagram returns
/// WouldBlock immediately: the code is written at byte 1, where the pinned
/// client's UDP-rx decode reads it. Bound to self_ip() (no UDP loopback).
pub fn udp_nonblocking_recv_wouldblock() {
    let addr = SocketAddr::new(self_ip(), next_port());
    let socket = check!(UdpSocket::bind(addr));
    check!(socket.set_nonblocking(true));
    // guarded: if the nonblocking request byte were dropped on the wire, the
    // recv would park forever on the empty socket
    let (result, elapsed, socket) = bounded("nonblocking recv_from", 10, move || {
        let started = Instant::now();
        let mut buf = [0u8; 32];
        let r = socket.recv_from(&mut buf);
        (r, started.elapsed(), socket)
    });
    drop(socket); // UDP drops are synchronous and safe inline
    match result {
        Ok((n, from)) => {
            panic!("nonblocking recv_from on an empty socket returned {} bytes from {}", n, from)
        }
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::WouldBlock,
            "nonblocking recv_from surfaced as {:?} ({}), want WouldBlock",
            e.kind(),
            e
        ),
    }
    assert!(elapsed < Duration::from_secs(1), "nonblocking recv_from took {:?}, want immediate", elapsed);
}

/// A nonblocking write with both socket buffers full (~3060 B) must return
/// WouldBlock; it parks forever instead.
/// XFAIL: StdTcpTx carries no nonblocking flag, so the full-buffer tx parks in tcp_tx_waiting with no expiry, services/net/src/main.rs.
pub fn tcp_nonblocking_write_wouldblock() {
    let addr = SocketAddr::new(LOOPBACK, next_port());
    let listener = check!(TcpListener::bind(addr));
    let client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let (served, _peer) = check!(accepted);
    check!(client.set_nonblocking(true));
    // the peer must stay open (and never read) through the write loop, and
    // the expected-failure path (NTC-10) panics out of the bounded call right
    // past this frame: park the peer sockets in ManuallyDrop so the unwind
    // cannot block on a close (NTC-5) — they leak on the panic path and are
    // discarded on the happy path
    let served = ManuallyDrop::new(served);
    let listener = ManuallyDrop::new(listener);
    let (outcome, total, client) = bounded("nonblocking write until WouldBlock", 15, move || {
        let mut client = client;
        let mut chunk = [0u8; 512];
        XorShift::new(0xE8).fill(&mut chunk);
        let mut total = 0usize;
        let mut outcome = None;
        // 20 * 512 B = 10240 B >> 3060 B of buffering: the loop must hit the
        // full-buffer condition well before it runs out of iterations
        for i in 0..20u32 {
            match client.write(&chunk) {
                Ok(n) => total += n,
                Err(e) => {
                    outcome = Some(e);
                    break;
                }
            }
            if (i + 1) % 5 == 0 {
                log::info!("nonblocking write {}: {} bytes accepted so far", i + 1, total);
            }
        }
        (outcome, total, client)
    });
    discard(client);
    discard(ManuallyDrop::into_inner(served));
    discard(ManuallyDrop::into_inner(listener));
    match outcome {
        None => panic!("20 nonblocking writes ({} bytes) all succeeded on ~3060 B of buffering", total),
        Some(e) => assert_eq!(
            e.kind(),
            ErrorKind::WouldBlock,
            "nonblocking write on full buffers surfaced as {:?} ({}) after {} accepted bytes, want WouldBlock",
            e.kind(),
            e,
            total
        ),
    }
}

/// A write() after our own shutdown(Write) must fail; it succeeds instead.
/// XFAIL: the shutdown handler aborts only already-parked messages and never
/// closes the smoltcp socket, so a later StdTcpTx is accepted, services/net/src/main.rs.
pub fn tcp_write_after_shutdown_write_errs() {
    let addr = SocketAddr::new(LOOPBACK, next_port());
    let listener = check!(TcpListener::bind(addr));
    let client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let (served, _peer) = check!(accepted);
    check!(client.shutdown(Shutdown::Write));
    // the write should return promptly either way, but guard it — and park
    // the peer sockets in ManuallyDrop so an unexpected bounded panic cannot
    // unwind-drop them on the test thread (NTC-5); discarded on the happy path
    let served = ManuallyDrop::new(served);
    let listener = ManuallyDrop::new(listener);
    let (result, client) = bounded("write after own shutdown(Write)", 10, move || {
        let mut client = client;
        let r = client.write(b"post-shutdown");
        (r, client)
    });
    discard(client);
    discard(ManuallyDrop::into_inner(served));
    discard(ManuallyDrop::into_inner(listener));
    if let Ok(n) = result {
        panic!("write after own shutdown(Write) succeeded with Ok({}), want an error", n);
    }
}

/// shutdown(Read) through a clone wakes a read blocked on the same socket and
/// makes it return Ok(0) (EOF); the woken read returns Err(Other) instead.
/// XFAIL: the wake sets body.valid but never body.offset, so the rx decode takes the error path, services/net/src/main.rs.
pub fn tcp_shutdown_read_wakes_blocked_read() {
    let addr = SocketAddr::new(LOOPBACK, next_port());
    let listener = check!(TcpListener::bind(addr));
    let client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let (served, _peer) = check!(accepted);
    let clone = check!(client.try_clone());
    // hand-rolled bounded pattern (spawn + recv_timeout, as harness::bounded
    // does internally): the shutdown must be issued from the test thread
    // WHILE the worker's read is parked, so a single bounded() call cannot
    // express the interleaving
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut client = client;
        let mut buf = [0u8; 8];
        let r = client.read(&mut buf);
        tx.send((r, client)).ok();
    });
    // let the read reach the server and park in tcp_rx_waiting before the
    // shutdown chases it
    thread::sleep(Duration::from_millis(1500));
    let shutdown_res = clone.shutdown(Shutdown::Read);
    // the expected-failure path (NTC-11) panics below: park the test-thread
    // sockets in ManuallyDrop so the unwind cannot block on a close (NTC-5) —
    // leaked on the panic path, discarded on the happy path
    let clone = ManuallyDrop::new(clone);
    let served = ManuallyDrop::new(served);
    let listener = ManuallyDrop::new(listener);
    let woken = match rx.recv_timeout(Duration::from_secs(10)) {
        Ok((r, client)) => {
            discard(client);
            Some(r)
        }
        // reader still parked: leak the worker thread and its socket handle
        // (its port is never reused); the clone discard below only drops a
        // refcount, so no close is issued against the parked handle (H-1)
        Err(_) => None,
    };
    discard(ManuallyDrop::into_inner(clone));
    discard(ManuallyDrop::into_inner(served));
    discard(ManuallyDrop::into_inner(listener));
    check!(shutdown_res);
    match woken {
        None => panic!("read was not woken within 10 s of shutdown(Read) (worker thread leaked)"),
        Some(r) => {
            assert_eq!(check!(r), 0, "read woken by shutdown(Read) must return Ok(0) (EOF)")
        }
    }
}

/// take_error() returns Ok(None) on TcpStream, TcpListener, and UdpSocket:
/// all three are client-side stubs that never store a pending error.
pub fn take_error_none() {
    let addr = SocketAddr::new(LOOPBACK, next_port());
    let listener = check!(TcpListener::bind(addr));
    // the smoltcp listener completes the handshake by itself; no accept is
    // needed for the client to reach Established
    let client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let udp = check!(UdpSocket::bind(SocketAddr::new(self_ip(), next_port())));
    let stream_err = client.take_error();
    let listener_err = listener.take_error();
    let udp_err = udp.take_error();
    discard(client);
    discard(listener);
    drop(udp); // UDP drops are synchronous and safe inline
    assert!(check!(stream_err).is_none(), "TcpStream::take_error returned a pending error");
    assert!(check!(listener_err).is_none(), "TcpListener::take_error returned a pending error");
    assert!(check!(udp_err).is_none(), "UdpSocket::take_error returned a pending error");
}

/// This theme's registry (aggregated by tests::all_tests / all_xfails).
pub const TESTS: &[TestEntry] = &[
    ("errors::tcp_bind_foreign_addr_kind", tcp_bind_foreign_addr_kind as fn()),
    ("errors::tcp_connect_unspecified_addr_kind", tcp_connect_unspecified_addr_kind as fn()),
    ("errors::tcp_nonblocking_accept_wouldblock", tcp_nonblocking_accept_wouldblock as fn()),
    ("errors::tcp_nonblocking_peek_wouldblock", tcp_nonblocking_peek_wouldblock as fn()),
    ("errors::udp_nonblocking_recv_wouldblock", udp_nonblocking_recv_wouldblock as fn()),
    ("errors::tcp_nonblocking_write_wouldblock", tcp_nonblocking_write_wouldblock as fn()),
    ("errors::tcp_write_after_shutdown_write_errs", tcp_write_after_shutdown_write_errs as fn()),
    ("errors::tcp_shutdown_read_wakes_blocked_read", tcp_shutdown_read_wakes_blocked_read as fn()),
    ("errors::take_error_none", take_error_none as fn()),
];

pub const XFAILS: &[XfailEntry] = &[
    ("errors::tcp_nonblocking_write_wouldblock", "NTC-10"),
    ("errors::tcp_write_after_shutdown_write_errs", "NTC-4"),
    ("errors::tcp_shutdown_read_wakes_blocked_read", "NTC-11"),
];
