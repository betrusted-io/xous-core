//! Seed tests proving the std::net plumbing end-to-end under Renode.
//! Assertions state the CORRECT behavior; known failures are registered in
//! the XFAILS table below — never weaken an assertion here.
//!
//! Structural discipline (see also tests/mod.rs): every blocking socket op
//! runs inside `harness::bounded`, workers hand their sockets back to the test
//! thread BEFORE any drop, and TCP sockets are released via `harness::discard`
//! — so neither a product hang nor a failing assert can wedge the suite (NTC-5).

use core::mem::ManuallyDrop;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::time::{Duration, Instant};

use crate::harness::{XorShift, bounded, check, discard, echo_server, next_port, self_ip};

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// TCP echo roundtrip over 127.0.0.1: bind, connect, write, read back.
pub fn tcp_echo_loopback() { tcp_echo_roundtrip(LOOPBACK); }

/// Same roundtrip over the DUT's own (static) IP, which loops back via the
/// dst-MAC==local path in services/net/src/device.rs.
pub fn tcp_echo_self_ip() { tcp_echo_roundtrip(self_ip()); }

fn tcp_echo_roundtrip(ip: IpAddr) {
    let port = next_port();
    let addr = SocketAddr::new(ip, port);
    let listener = check!(TcpListener::bind(addr));
    // the echo thread (and the listener it owns) leak by design: EOF-driven
    // teardown is pinned separately by tcp_drop_close_delivers_eof (NTC-3)
    let _server = echo_server(listener);
    let mut client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let mut payload = vec![0u8; 1024];
    XorShift::new(port as u32).fill(&mut payload);
    check!(client.write_all(&payload));
    let (result, client) = bounded("echo readback", 10, move || {
        let mut buf = vec![0u8; 1024];
        let r = client.read_exact(&mut buf).map(|_| buf);
        (r, client)
    });
    discard(client);
    assert_eq!(check!(result), payload, "echo payload corrupted");
}

/// A dropped (closed) TcpStream must deliver EOF to its peer: the echo
/// server's blocking read returns Ok(0) and the thread exits.
/// XFAIL: a reader parked in tcp_rx_waiting is never completed with EOF across CloseWait, services/net/src/main.rs NetPump.
pub fn tcp_drop_close_delivers_eof() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    let server = echo_server(listener);
    let mut client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    check!(client.write_all(b"bye"));
    let (result, client) = bounded("echo readback", 10, move || {
        let mut buf = [0u8; 3];
        let r = client.read_exact(&mut buf).map(|_| buf);
        (r, client)
    });
    check!(result);
    discard(client); // close on a worker thread; must FIN the peer
    assert_eq!(server.wait(), 3, "echo server byte count at EOF");
}

/// One listener must serve two sequential connect/accept cycles.
/// The listener rides in ManuallyDrop so a failing round cannot unwind-drop it
/// on the test thread (blocking close, NTC-5); discarded on the happy path.
pub fn tcp_accept_two_sequential() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let mut listener = ManuallyDrop::new(Some(check!(TcpListener::bind(addr))));
    for round in 0u8..2 {
        log::info!("accept round {}: connecting", round);
        let mut client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
        log::info!("accept round {}: accepting", round);
        let l = listener.take().unwrap();
        let (accepted, l) = bounded("accept", 10, move || {
            let r = l.accept();
            (r, l)
        });
        *listener = Some(l);
        let (served, _peer) = check!(accepted);
        log::info!("accept round {}: exchanging", round);
        check!(client.write_all(&[round]));
        let (server_read, mut served) = bounded("server read", 10, move || {
            let mut served = served;
            let mut buf = [0u8; 1];
            let r = served.read_exact(&mut buf).map(|_| buf[0]);
            (r, served)
        });
        let reply_write = served.write_all(&[round ^ 0xff]);
        let (client_read, client) = bounded("client read", 10, move || {
            let mut buf = [0u8; 1];
            let r = client.read_exact(&mut buf).map(|_| buf[0]);
            (r, client)
        });
        discard(served);
        discard(client);
        assert_eq!(check!(server_read), round, "server read the wrong byte in round {}", round);
        check!(reply_write);
        assert_eq!(check!(client_read), round ^ 0xff, "client read the wrong byte in round {}", round);
        log::info!("accept round {}: done", round);
    }
    discard(listener.take().unwrap());
}

/// Connecting to a port nobody listens on must fail with ConnectionRefused,
/// quickly (the local stack RSTs the SYN over loopback).
/// XFAIL: the connect fails up front as AddrNotAvailable via ConnectError::Unaddressable, services/net/src/std_tcpstream.rs.
pub fn tcp_connect_refused() {
    let addr = SocketAddr::new(self_ip(), next_port()); // never listened on
    let started = Instant::now();
    let result = bounded("connect to a closed port", 10, move || {
        TcpStream::connect_timeout(&addr, Duration::from_secs(5))
    });
    let elapsed = started.elapsed();
    match result {
        Ok(s) => {
            discard(s);
            panic!("connect to a never-listened port unexpectedly succeeded");
        }
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::ConnectionRefused,
            "refused connect surfaced as {:?} ({}), want ConnectionRefused",
            e.kind(),
            e
        ),
    }
    assert!(
        elapsed < Duration::from_secs(4),
        "refusal took {:?}, want well under the 5 s connect timeout",
        elapsed
    );
}

/// shutdown(Write) must deliver EOF to the peer: its read returns Ok(0).
/// XFAIL: StdTcpStreamShutdown never calls socket.close(), so no FIN is
/// emitted and the peer never sees EOF, services/net/src/main.rs.
pub fn tcp_shutdown_write() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    let mut client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let (served, _peer) = check!(accepted);
    check!(client.write_all(b"x"));
    let (first_read, served) = bounded("server read of the pre-shutdown byte", 10, move || {
        let mut served = served;
        let mut buf = [0u8; 1];
        let r = served.read_exact(&mut buf);
        (r, served)
    });
    check!(first_read);
    check!(client.shutdown(Shutdown::Write));
    // the expected-failure path panics past this frame, and client must stay
    // open through the read (a close would race its own EOF into it): park
    // both in ManuallyDrop so the unwind cannot block on a close (NTC-5) —
    // they leak on the panic path and are discarded on the happy path
    let client = ManuallyDrop::new(client);
    let listener = ManuallyDrop::new(listener);
    let (result, served) = bounded("read after peer shutdown(Write)", 10, move || {
        let mut served = served;
        let mut buf = [0u8; 8];
        let r = served.read(&mut buf);
        (r, served)
    });
    discard(served);
    discard(ManuallyDrop::into_inner(client));
    discard(ManuallyDrop::into_inner(listener));
    assert_eq!(check!(result), 0, "peer read after shutdown(Write) must see EOF (Ok(0))");
}

/// Closing a TCP socket that generated no traffic must complete: dropping a
/// never-connected listener returns promptly.
/// XFAIL: StdTcpClose parks in tcp_tx_closing, serviced only when the pump poll() reports activity — never on a quiet socket, services/net/src/main.rs.
pub fn tcp_close_idle_listener() {
    let port = next_port();
    let listener = check!(TcpListener::bind(SocketAddr::new(LOOPBACK, port)));
    // the close IS the operation under test, so here (and only here) a drop
    // runs inside the bounded worker itself
    bounded("close of an idle listener", 10, move || drop(listener));
}

/// local_addr/peer_addr must be coherent across both ends of a connection.
pub fn tcp_local_peer_addr() {
    let port = next_port();
    let bind_addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(bind_addr));
    let client = check!(bounded("connect", 10, move || TcpStream::connect(bind_addr)));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let (served, accept_peer) = check!(accepted);
    let c_local = client.local_addr();
    let c_peer = client.peer_addr();
    let s_local = served.local_addr();
    let s_peer = served.peer_addr();
    discard(client);
    discard(served);
    discard(listener);
    let c_local = check!(c_local);
    let c_peer = check!(c_peer);
    let s_local = check!(s_local);
    let s_peer = check!(s_peer);
    assert_eq!(c_peer, bind_addr, "client peer_addr");
    assert_eq!(s_local, bind_addr, "server local_addr");
    assert_eq!(s_peer, c_local, "server peer_addr vs client local_addr");
    assert_eq!(accept_peer, s_peer, "accept()'s peer address vs peer_addr()");
    assert_ne!(c_local.port(), 0, "client local port was never assigned");
}

/// UDP send_to/recv_from roundtrip between two sockets bound to the DUT's own
/// (static) IP, both directions. (UDP closes are synchronous on the server,
/// so plain drops are safe here.)
pub fn udp_send_recv_self_ip() { udp_roundtrip(self_ip()); }

/// Same roundtrip over 127.0.0.1.
/// XFAIL: std_udp force-rebinds every socket to iface.ipv4_addr(), so a
/// datagram addressed to 127.0.0.1 matches no socket, services/net/src/std_udp.rs.
pub fn udp_send_recv_loopback() { udp_roundtrip(LOOPBACK); }

fn udp_roundtrip(ip: IpAddr) {
    let port_a = next_port();
    let port_b = next_port();
    let a = check!(UdpSocket::bind(SocketAddr::new(ip, port_a)));
    let b = check!(UdpSocket::bind(SocketAddr::new(ip, port_b)));

    assert_eq!(check!(a.send_to(b"ping", SocketAddr::new(ip, port_b))), 4, "send_to(ping) length");
    // contained: an undeliverable datagram parks recv_from forever
    let (result, buf, b) = bounded("udp recv_from on b", 10, move || {
        let mut buf = [0u8; 32];
        let r = b.recv_from(&mut buf);
        (r, buf, b)
    });
    let (n, from) = check!(result);
    assert_eq!(&buf[..n], b"ping", "b received the wrong payload");
    assert_eq!(from, SocketAddr::new(ip, port_a), "b saw the wrong source address");

    assert_eq!(check!(b.send_to(b"pong", SocketAddr::new(ip, port_a))), 4, "send_to(pong) length");
    let (result, buf, _a) = bounded("udp recv_from on a", 10, move || {
        let mut buf = [0u8; 32];
        let r = a.recv_from(&mut buf);
        (r, buf, a)
    });
    let (n, from) = check!(result);
    assert_eq!(&buf[..n], b"pong", "a received the wrong payload");
    assert_eq!(from, SocketAddr::new(ip, port_b), "a saw the wrong source address");
}

/// A 2 s read timeout on a quiet connected socket must surface as WouldBlock
/// or TimedOut after roughly 2 s.
/// XFAIL: the timeout reapers run only past the pump poll() early-return that a quiet socket never trips, services/net/src/main.rs.
pub fn tcp_read_timeout_quiet() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    let mut client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let (served, _peer) = check!(accepted);
    // the peer must stay open AND quiet through the read window, and the
    // expected-failure path (NTC-1) panics right past this frame: park the
    // peer sockets in ManuallyDrop so the unwind cannot block on a close
    // (NTC-5) — they leak on the panic path, discarded on the happy path
    let served = ManuallyDrop::new(served);
    let listener = ManuallyDrop::new(listener);
    check!(client.set_read_timeout(Some(Duration::from_secs(2))));
    let (result, elapsed, client) = bounded("read with a 2 s timeout on a quiet socket", 8, move || {
        let started = Instant::now();
        let mut buf = [0u8; 8];
        let r = client.read(&mut buf);
        (r, started.elapsed(), client)
    });
    discard(client);
    discard(ManuallyDrop::into_inner(served));
    discard(ManuallyDrop::into_inner(listener));
    match result {
        Ok(n) => panic!("read on a quiet socket returned Ok({}) instead of a timeout error", n),
        Err(e) => assert!(
            e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut,
            "read timeout surfaced as {:?} ({}), want WouldBlock or TimedOut",
            e.kind(),
            e
        ),
    }
    assert!(
        elapsed >= Duration::from_millis(1500) && elapsed <= Duration::from_secs(4),
        "read returned after {:?}, want roughly the 2 s timeout",
        elapsed
    );
}

/// set_nonblocking(true): a read with no pending data must return WouldBlock
/// immediately; once the peer writes, a bounded poll loop must see the data.
pub fn tcp_nonblocking_read_wouldblock() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    let mut client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let (mut served, _peer) = check!(accepted);
    check!(client.set_nonblocking(true));
    // nonblocking reads return immediately, so they are safe on the test thread
    let started = Instant::now();
    let mut buf = [0u8; 8];
    let first = client.read(&mut buf);
    let first_elapsed = started.elapsed();
    let write_res = served.write_all(&[42]);
    // poll for the byte; collect the outcome, defer the verdict until after
    // the sockets are discarded
    let mut polled = None;
    let mut tries = 0u32;
    while tries < 20 {
        match client.read(&mut buf) {
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                tries += 1;
                if tries % 5 == 0 {
                    log::info!("nonblocking poll {}: still WouldBlock", tries);
                }
                std::thread::sleep(Duration::from_millis(100));
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
        Ok(n) => panic!("nonblocking read with no pending data returned Ok({})", n),
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::WouldBlock,
            "nonblocking read surfaced as {:?} ({}), want WouldBlock",
            e.kind(),
            e
        ),
    }
    assert!(
        first_elapsed < Duration::from_secs(1),
        "nonblocking read took {:?}, want immediate",
        first_elapsed
    );
    check!(write_res);
    match polled {
        Some(Ok(1)) => assert_eq!(got, 42, "wrong byte after nonblocking poll"),
        Some(Ok(n)) => panic!("nonblocking read returned Ok({}), want exactly 1 byte", n),
        Some(Err(e)) => panic!("nonblocking read failed: {}", e),
        None => panic!("data never arrived within 20 nonblocking polls"),
    }
}

/// This theme's registry (aggregated by tests::all_tests / all_xfails).
pub const TESTS: &[(&str, fn())] = &[
    ("smoke::tcp_echo_loopback", tcp_echo_loopback as fn()),
    ("smoke::tcp_echo_self_ip", tcp_echo_self_ip as fn()),
    ("smoke::tcp_drop_close_delivers_eof", tcp_drop_close_delivers_eof as fn()),
    ("smoke::tcp_accept_two_sequential", tcp_accept_two_sequential as fn()),
    ("smoke::tcp_connect_refused", tcp_connect_refused as fn()),
    ("smoke::tcp_shutdown_write", tcp_shutdown_write as fn()),
    ("smoke::tcp_close_idle_listener", tcp_close_idle_listener as fn()),
    ("smoke::tcp_local_peer_addr", tcp_local_peer_addr as fn()),
    ("smoke::udp_send_recv_self_ip", udp_send_recv_self_ip as fn()),
    ("smoke::udp_send_recv_loopback", udp_send_recv_loopback as fn()),
    ("smoke::tcp_read_timeout_quiet", tcp_read_timeout_quiet as fn()),
    ("smoke::tcp_nonblocking_read_wouldblock", tcp_nonblocking_read_wouldblock as fn()),
];

pub const XFAILS: &[(&str, &str)] = &[
    ("smoke::tcp_drop_close_delivers_eof", "NTC-3"),
    ("smoke::tcp_connect_refused", "NTC-6"),
    ("smoke::tcp_shutdown_write", "NTC-4"),
    ("smoke::tcp_close_idle_listener", "NTC-5"),
    ("smoke::udp_send_recv_loopback", "NTC-2"),
    ("smoke::tcp_read_timeout_quiet", "NTC-1"),
];
