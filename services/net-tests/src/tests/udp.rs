//! udp theme — datagram semantics over the DUT's self-IP loopback path.
//!
//! Live constraints (see also tests/mod.rs): every socket binds self_ip(),
//! never 127.0.0.1 (std_udp force-rebinds to iface.ipv4_addr()); every port is
//! explicit because UDP bind to port 0 FAILS on xous; the rx queue holds at
//! most TWO undelivered datagrams (2 PacketMetadata slots), so tests drain
//! between sends. UDP closes are synchronous, so plain drops are safe inline
//! but blocking recv/peek still runs inside bounded().

use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;

use crate::harness::{XorShift, bounded, check, next_port, self_ip};
use crate::tests::{TestEntry, XfailEntry};

/// Bind a UDP socket on the DUT's own IP at a fresh port and return it with its
/// address. Always explicit — UDP port-0 assignment does not exist on xous (see
/// udp_bind_port_zero_fails).
fn bind_self() -> (UdpSocket, SocketAddr) {
    let addr = SocketAddr::new(self_ip(), next_port());
    (check!(UdpSocket::bind(addr)), addr)
}

/// peek_from must not consume: two consecutive peeks return the same datagram
/// and source, recv_from then consumes it, and the queue is empty afterwards.
pub fn udp_peek_from_does_not_consume() {
    let (tx, tx_addr) = bind_self();
    let (rx, rx_addr) = bind_self();
    let mut payload = [0u8; 32];
    XorShift::new(rx_addr.port() as u32).fill(&mut payload);
    assert_eq!(check!(tx.send_to(&payload, rx_addr)), 32, "send_to length");
    let (first, first_buf, rx) = bounded("first peek_from", 10, move || {
        let mut buf = [0u8; 64];
        let r = rx.peek_from(&mut buf);
        (r, buf, rx)
    });
    let (n, from) = check!(first);
    assert_eq!(&first_buf[..n], &payload[..], "first peek payload");
    assert_eq!(from, tx_addr, "first peek source address");
    let (second, second_buf, rx) = bounded("second peek_from", 10, move || {
        let mut buf = [0u8; 64];
        let r = rx.peek_from(&mut buf);
        (r, buf, rx)
    });
    let (n, from) = check!(second);
    assert_eq!(&second_buf[..n], &payload[..], "second peek payload (peek must not consume)");
    assert_eq!(from, tx_addr, "second peek source address");
    let (consumed, recv_buf, rx) = bounded("recv_from after two peeks", 10, move || {
        let mut buf = [0u8; 64];
        let r = rx.recv_from(&mut buf);
        (r, buf, rx)
    });
    let (n, from) = check!(consumed);
    assert_eq!(&recv_buf[..n], &payload[..], "recv payload after peeks");
    assert_eq!(from, tx_addr, "recv source address");
    check!(rx.set_nonblocking(true));
    let mut buf = [0u8; 64];
    assert!(
        rx.recv_from(&mut buf).is_err(),
        "queue must be empty once recv_from consumed the peeked datagram"
    );
}

/// recv_from into a buffer smaller than the datagram (10-byte buffer, 100-byte
/// datagram) must return Ok(n <= 10) with the excess discarded.
/// XFAIL: the client copies min(buf, rxlen) bytes but returns the full datagram length — Ok(100) from a 10-byte buffer (client-side), xous udp.rs.
pub fn udp_recv_buffer_smaller_than_datagram() {
    let (tx, tx_addr) = bind_self();
    let (rx, rx_addr) = bind_self();
    let mut payload = [0u8; 100];
    XorShift::new(rx_addr.port() as u32).fill(&mut payload);
    assert_eq!(check!(tx.send_to(&payload, rx_addr)), 100, "send_to length");
    let (result, buf, _rx) = bounded("recv_from into a 10-byte buffer", 10, move || {
        let mut buf = [0u8; 10];
        let r = rx.recv_from(&mut buf);
        (r, buf, rx)
    });
    let (n, from) = check!(result);
    assert_eq!(from, tx_addr, "source address");
    assert_eq!(&buf[..], &payload[..10], "buffer must hold the datagram's first 10 bytes");
    assert!(
        n <= 10,
        "recv_from returned {} from a 10-byte buffer — client reports the full datagram \
         length instead of bytes written (NTC-8, xous_udp.rs:182-185)",
        n
    );
    assert_eq!(n, 10, "recv length must be min(buffer, datagram)");
}

/// PROBE: zero-length datagrams are legal — send_to(&[]) returns Ok(0) and the
/// peer's recv_from returns Ok((0, sender)). Unverified on xous; asserts the
/// standard contract, and the bounded recv reclassifies it if never delivered.
pub fn udp_zero_len_datagram() {
    let (tx, tx_addr) = bind_self();
    let (rx, rx_addr) = bind_self();
    assert_eq!(check!(tx.send_to(&[], rx_addr)), 0, "send_to(&[]) length");
    let (result, rx) = bounded("recv_from of a zero-length datagram", 10, move || {
        let mut buf = [0u8; 16];
        let r = rx.recv_from(&mut buf);
        (r, rx)
    });
    drop(rx); // UDP close is synchronous; safe inline
    let (n, from) = check!(result);
    assert_eq!(n, 0, "zero-length datagram recv length");
    assert_eq!(from, tx_addr, "zero-length datagram source address");
}

/// Deviation-pin: the rx queue holds at most TWO undelivered datagrams (2
/// PacketMetadata slots) — a third sent unread is dropped; draining yields #1
/// and #2 in order, then empty. Sends are spaced 2 s to serialize the loopback trip.
pub fn udp_rx_queue_depth_two() {
    let (tx, tx_addr) = bind_self();
    let (rx, rx_addr) = bind_self();
    let payloads: [&[u8]; 3] = [b"dgram-one", b"dgram-two", b"dgram-three"];
    for (i, p) in payloads.iter().enumerate() {
        assert_eq!(check!(tx.send_to(p, rx_addr)), p.len(), "send_to length for datagram {}", i);
        log::info!("queue-depth: datagram {} sent, letting it land", i);
        std::thread::sleep(Duration::from_millis(2000));
    }
    let mut rx = rx;
    for i in 0..2usize {
        let (result, buf, rx_back) = bounded("drain recv_from", 10, move || {
            let mut buf = [0u8; 32];
            let r = rx.recv_from(&mut buf);
            (r, buf, rx)
        });
        rx = rx_back;
        let (n, from) = check!(result);
        assert_eq!(&buf[..n], payloads[i], "drained datagram {} out of order or corrupted", i);
        assert_eq!(from, tx_addr, "drained datagram {} source address", i);
        log::info!("queue-depth: drained datagram {}", i);
    }
    check!(rx.set_nonblocking(true));
    let mut buf = [0u8; 32];
    assert!(
        rx.recv_from(&mut buf).is_err(),
        "third datagram must have been dropped (2 metadata slots), but the queue still has data"
    );
}

/// UDP is fire-and-forget: send_to a never-bound port returns Ok(len), later
/// sends still work, and nothing ever arrives back (xous never propagates ICMP
/// port-unreachable to UDP sockets).
pub fn udp_send_to_dead_port_ok() {
    let (sock, _sock_addr) = bind_self();
    let dead = SocketAddr::new(self_ip(), next_port()); // allocated, never bound
    assert_eq!(check!(sock.send_to(b"nobody-home", dead)), 11, "send_to a dead port length");
    // give any hypothetical error propagation time to land, then prove the
    // socket is still healthy and its queue still quiet
    std::thread::sleep(Duration::from_millis(2000));
    assert_eq!(check!(sock.send_to(b"still-alive", dead)), 11, "second send_to after the first");
    check!(sock.set_nonblocking(true));
    let mut buf = [0u8; 32];
    assert!(sock.recv_from(&mut buf).is_err(), "nothing must ever arrive back on the sender");
}

/// Deviation-pin: connected-UDP does NOT filter by peer on xous — the client's
/// connect() is bookkeeping only and never tells the server, so recv() behaves
/// like recv_from(); a third socket's datagram reaches a socket connected elsewhere.
pub fn udp_connect_does_not_filter_peer() {
    let (receiver, receiver_addr) = bind_self();
    let (_peer, peer_addr) = bind_self();
    let (third, third_addr) = bind_self();
    check!(receiver.connect(peer_addr));
    assert_eq!(check!(third.send_to(b"interloper", receiver_addr)), 10, "third-party send_to length");
    let (result, buf, _receiver) = bounded("recv on a connected socket", 10, move || {
        let mut buf = [0u8; 32];
        let r = receiver.recv(&mut buf);
        (r, buf, receiver)
    });
    let n = check!(result);
    assert_eq!(
        &buf[..n],
        b"interloper",
        "pin: recv() on a socket connected to {:?} must still deliver the third party's ({:?}) datagram",
        peer_addr,
        third_addr
    );
}

/// Unconnected-socket errors are client-side and correct: send() and peer_addr()
/// before connect() both fail NotConnected; after connect(), peer_addr() returns
/// the address and send() delivers to it.
pub fn udp_send_before_connect_notconnected() {
    let (sock, sock_addr) = bind_self();
    let (peer, peer_addr) = bind_self();
    match sock.send(b"early") {
        Ok(n) => panic!("send() before connect() returned Ok({})", n),
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::NotConnected,
            "send() before connect() surfaced as {:?} ({}), want NotConnected",
            e.kind(),
            e
        ),
    }
    match sock.peer_addr() {
        Ok(a) => panic!("peer_addr() before connect() returned {:?}", a),
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::NotConnected,
            "peer_addr() before connect() surfaced as {:?} ({}), want NotConnected",
            e.kind(),
            e
        ),
    }
    check!(sock.connect(peer_addr));
    assert_eq!(check!(sock.peer_addr()), peer_addr, "peer_addr() after connect()");
    assert_eq!(check!(sock.send(b"hello-peer")), 10, "send() after connect() length");
    let (result, buf, _peer) = bounded("recv_from on the connected-send peer", 10, move || {
        let mut buf = [0u8; 32];
        let r = peer.recv_from(&mut buf);
        (r, buf, peer)
    });
    let (n, from) = check!(result);
    assert_eq!(&buf[..n], b"hello-peer", "connected send() payload");
    assert_eq!(from, sock_addr, "connected send() source address");
}

/// Deviation-pin: binding UDP to port 0 FAILS on xous instead of assigning an
/// ephemeral port — smoltcp rejects port 0 and std_udp_bind has no assignment
/// path; the kind is pinned to Other on this toolchain (hence every test uses next_port()).
pub fn udp_bind_port_zero_fails() {
    match UdpSocket::bind(SocketAddr::new(self_ip(), 0)) {
        Ok(sock) => {
            let got = sock.local_addr();
            drop(sock); // UDP close is synchronous; safe inline
            panic!("bind to port 0 unexpectedly succeeded (local_addr {:?}) — re-evaluate the U-10 pin", got);
        }
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::Other,
            "port-0 bind error surfaced as {:?} ({}), pinned as Other on this toolchain",
            e.kind(),
            e
        ),
    }
}

/// DANGER, NOT REGISTERED — an oversized send_to PANICS THE NET SERVICE: the
/// client declares len=buf.len() uncapped and the server slices bytes[21..21+len]
/// out of the 4096-byte page (OOB for any buf > 4075), hanging the run. Kept as
/// the contract reproducer: an oversize send must fail cleanly or send the whole buffer.
#[allow(dead_code)]
pub fn udp_send_to_oversize_payload_disabled() {
    let (tx, _tx_addr) = bind_self();
    let (rx, rx_addr) = bind_self();
    let mut payload = vec![0u8; 4096]; // > 4075 == 4096 - 21-byte header
    XorShift::new(rx_addr.port() as u32).fill(&mut payload);
    match tx.send_to(&payload, rx_addr) {
        Ok(n) => assert_eq!(
            n,
            payload.len(),
            "oversize send_to silently truncated: sent {} of {} bytes",
            n,
            payload.len()
        ),
        Err(e) => log::info!("oversize send_to failed cleanly: {} ({:?})", e, e.kind()),
    }
    drop(rx);
}

/// Deviation-pin: a 2000-byte datagram (> NET_MTU=1530) silently vanishes —
/// send_to returns Ok(2000) but smoltcp emits no IPv4 fragments, so it is
/// dropped at dispatch. Payload <= 4075 B keeps clear of the oversize-send panic.
pub fn udp_datagram_larger_than_mtu() {
    let (tx, _tx_addr) = bind_self();
    let (rx, rx_addr) = bind_self();
    let mut payload = vec![0u8; 2000];
    XorShift::new(rx_addr.port() as u32).fill(&mut payload);
    assert_eq!(check!(tx.send_to(&payload, rx_addr)), 2000, "send_to(2000 B) length");
    check!(rx.set_nonblocking(true));
    let mut buf = vec![0u8; 4096];
    let mut outcome = None;
    for try_n in 1..=20u32 {
        match rx.recv_from(&mut buf) {
            Err(_) => {
                if try_n % 5 == 0 {
                    log::info!("mtu pin: poll {} of 20, queue still empty", try_n);
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            got => {
                outcome = Some(got);
                break;
            }
        }
    }
    match outcome {
        None => log::info!(
            "2000-byte datagram sent Ok and never delivered within 20 polls (~5 s) \
             (pinned: no IPv4 fragmentation past NET_MTU=1530)"
        ),
        Some(Ok((n, from))) => panic!(
            "a {}-byte datagram from {} was delivered — the no-fragmentation pin no longer \
             holds; re-point this test at the contract (intact 2000-byte delivery) and drop \
             the pin",
            n, from
        ),
        Some(Err(e)) => {
            panic!("recv_from failed with {:?} ({}) instead of the queue staying empty", e.kind(), e)
        }
    }
}

/// Deviation-pin: a second bind of an already-bound port should fail EADDRINUSE,
/// but on xous BOTH binds succeed — every bind makes a fresh smoltcp socket and
/// nothing scans for cross-socket conflicts (UDP mirror of tcp_double_bind_same_port).
pub fn udp_double_bind_same_port() {
    let addr = SocketAddr::new(self_ip(), next_port());
    let first = check!(UdpSocket::bind(addr));
    let second = UdpSocket::bind(addr);
    // UDP closes are synchronous: inline drops are safe, even mid-panic
    match second {
        Ok(sock) => {
            drop(sock);
            drop(first);
            log::info!("second bind of {} succeeded (pinned: no cross-socket conflict detection)", addr);
        }
        Err(e) => {
            let (kind, msg) = (e.kind(), e.to_string());
            drop(first);
            panic!(
                "second bind of {} was refused with {:?} ({}) — the no-conflict-detection pin \
                 no longer holds; re-point this test at the contract (AddrInUse) and drop the pin",
                addr, kind, msg
            );
        }
    }
}

/// Deviation-pin: binding UDP to a foreign address (1.1.1.1) SUCCEEDS on xous —
/// std_udp_bind has no bind-address whitelist and smoltcp accepts any endpoint.
/// local_addr() echoes the foreign address verbatim (the client never queries the server).
pub fn udp_bind_foreign_addr_succeeds() {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), next_port());
    let sock = check!(UdpSocket::bind(addr));
    assert_eq!(check!(sock.local_addr()), addr, "local_addr echoes the foreign bind address");
}

/// local_addr() returns the bound address exactly for explicit binds — the
/// client stores the requested SocketAddr at bind time and returns it verbatim
/// without a server round-trip.
pub fn udp_local_addr_is_bound_addr() {
    let (sock, addr) = bind_self();
    assert_eq!(check!(sock.local_addr()), addr, "local_addr vs bind address");
}

/// Drop a UDP socket and immediately rebind its port: must succeed (StdUdpClose
/// synchronously removes the socket, no UDP TIME_WAIT) and the rebound socket
/// must receive. The suite's ONLY deliberate port reuse — reuse is under test.
pub fn udp_fast_rebind_after_drop() {
    let port = next_port();
    let addr = SocketAddr::new(self_ip(), port);
    let first = check!(UdpSocket::bind(addr));
    drop(first); // synchronous close: the port is free once this returns
    let rebound = check!(UdpSocket::bind(addr));
    let (helper, helper_addr) = bind_self();
    assert_eq!(check!(helper.send_to(b"rebound", addr)), 7, "send_to the rebound socket");
    let (result, buf, _rebound) = bounded("recv_from on the rebound socket", 10, move || {
        let mut buf = [0u8; 32];
        let r = rebound.recv_from(&mut buf);
        (r, buf, rebound)
    });
    let (n, from) = check!(result);
    assert_eq!(&buf[..n], b"rebound", "rebound socket payload");
    assert_eq!(from, helper_addr, "rebound socket source address");
}

/// Two independent socket pairs with interleaved traffic demux by destination
/// port with no cross-delivery — a detector for cross-talk from the force-rebind
/// hack that leaves the port the only demux key. One datagram per flow per round.
pub fn udp_two_sockets_interleaved() {
    let (tx1, tx1_addr) = bind_self();
    let (mut rx1, rx1_addr) = bind_self();
    let (tx2, tx2_addr) = bind_self();
    let (mut rx2, rx2_addr) = bind_self();
    for round in 0u8..3 {
        let p1 = [0x10 | round; 12];
        let p2 = [0x20 | round; 12];
        assert_eq!(check!(tx1.send_to(&p1, rx1_addr)), 12, "pair-1 send length, round {}", round);
        assert_eq!(check!(tx2.send_to(&p2, rx2_addr)), 12, "pair-2 send length, round {}", round);
        let (r1, b1, rx1_back) = bounded("pair-1 recv_from", 10, move || {
            let mut buf = [0u8; 32];
            let r = rx1.recv_from(&mut buf);
            (r, buf, rx1)
        });
        rx1 = rx1_back;
        let (n, from) = check!(r1);
        assert_eq!(&b1[..n], &p1[..], "pair-1 payload, round {} (cross-delivery?)", round);
        assert_eq!(from, tx1_addr, "pair-1 source address, round {}", round);
        let (r2, b2, rx2_back) = bounded("pair-2 recv_from", 10, move || {
            let mut buf = [0u8; 32];
            let r = rx2.recv_from(&mut buf);
            (r, buf, rx2)
        });
        rx2 = rx2_back;
        let (n, from) = check!(r2);
        assert_eq!(&b2[..n], &p2[..], "pair-2 payload, round {} (cross-delivery?)", round);
        assert_eq!(from, tx2_addr, "pair-2 source address, round {}", round);
        log::info!("interleaved round {} complete", round);
    }
    // nothing may be left over anywhere: a stray datagram means cross-talk
    for (name, sock) in [("tx1", &tx1), ("rx1", &rx1), ("tx2", &tx2), ("rx2", &rx2)] {
        check!(sock.set_nonblocking(true));
        let mut buf = [0u8; 32];
        assert!(sock.recv_from(&mut buf).is_err(), "{} holds an unexpected leftover datagram", name);
    }
}

/// This theme's registry (aggregated by tests::all_tests / all_xfails).
/// udp_send_to_oversize_payload_disabled is intentionally absent: DANGER, it
/// panics the net service on the pinned toolchain.
pub const TESTS: &[TestEntry] = &[
    ("udp::udp_peek_from_does_not_consume", udp_peek_from_does_not_consume as fn()),
    ("udp::udp_recv_buffer_smaller_than_datagram", udp_recv_buffer_smaller_than_datagram as fn()),
    ("udp::udp_zero_len_datagram", udp_zero_len_datagram as fn()),
    ("udp::udp_rx_queue_depth_two", udp_rx_queue_depth_two as fn()),
    ("udp::udp_send_to_dead_port_ok", udp_send_to_dead_port_ok as fn()),
    ("udp::udp_connect_does_not_filter_peer", udp_connect_does_not_filter_peer as fn()),
    ("udp::udp_send_before_connect_notconnected", udp_send_before_connect_notconnected as fn()),
    ("udp::udp_bind_port_zero_fails", udp_bind_port_zero_fails as fn()),
    ("udp::udp_datagram_larger_than_mtu", udp_datagram_larger_than_mtu as fn()),
    ("udp::udp_double_bind_same_port", udp_double_bind_same_port as fn()),
    ("udp::udp_bind_foreign_addr_succeeds", udp_bind_foreign_addr_succeeds as fn()),
    ("udp::udp_local_addr_is_bound_addr", udp_local_addr_is_bound_addr as fn()),
    ("udp::udp_fast_rebind_after_drop", udp_fast_rebind_after_drop as fn()),
    ("udp::udp_two_sockets_interleaved", udp_two_sockets_interleaved as fn()),
];

pub const XFAILS: &[XfailEntry] = &[("udp::udp_recv_buffer_smaller_than_datagram", "NTC-8")];
