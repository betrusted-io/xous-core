//! timeouts theme: read/peek/write/recv/connect timeout semantics; Renode's
//! virtual clock makes every elapsed-time assertion deterministic.
//! Theme-wide hazard (#880 / NTC-1): on dev the timeout reapers run only in
//! the NetPump handler, whose `if !iface.poll(..) { continue; }` early-return
//! skips them while the interface is quiet — exactly when a timeout matters.
//! Every quiet-socket timeout op therefore runs inside `harness::bounded`
//! (generous guard) that turns the hang into a deterministic panic; such tests
//! are XFAIL NTC-1. Assertions state the correct contract; never weaken one.

use core::mem::ManuallyDrop;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use crate::harness::{XorShift, bounded, check, discard, next_port, self_ip};
use crate::tests::{TestEntry, XfailEntry};

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// Off-subnet blackhole (the DUT is 10.0.2.15/24): under Loopback nothing
/// answers the gateway ARP, so a SYN toward this address is never delivered,
/// never acknowledged, and never reset — only a timeout can end the attempt.
const BLACKHOLE: Ipv4Addr = Ipv4Addr::new(10, 254, 254, 254);

/// With a 2 s read timeout set, a first read on a socket with data pending
/// returns that data, and a second read on the now-quiet socket must err
/// WouldBlock-or-TimedOut after roughly the timeout.
/// XFAIL: the quiet-socket rx reaper is skipped by the NetPump early-return, so the second read never returns, services/net/src/main.rs:1305-1308.
pub fn tcp_read_timeout_after_data() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    let client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let (mut served, _peer) = check!(accepted);
    check!(client.set_read_timeout(Some(Duration::from_secs(2))));
    check!(served.write_all(b"data"));
    // the peer must stay open AND quiet through the timeout window, and the
    // expected-failure path (NTC-1) panics right past this frame: park the
    // peer sockets in ManuallyDrop so the unwind cannot block on a close
    // (NTC-5) — they leak on the panic path, discarded on the happy path
    let served = ManuallyDrop::new(served);
    let listener = ManuallyDrop::new(listener);
    let (first, first_buf, client) = bounded("first read with data pending", 10, move || {
        let mut client = client;
        let mut buf = [0u8; 4];
        let r = client.read_exact(&mut buf);
        (r, buf, client)
    });
    let (second, second_elapsed, client) =
        bounded("second read with a 2 s timeout on a quiet socket", 8, move || {
            let mut client = client;
            let started = Instant::now();
            let mut buf = [0u8; 8];
            let r = client.read(&mut buf);
            (r, started.elapsed(), client)
        });
    discard(client);
    discard(ManuallyDrop::into_inner(served));
    discard(ManuallyDrop::into_inner(listener));
    check!(first);
    assert_eq!(&first_buf, b"data", "first read returned the wrong bytes");
    match second {
        Ok(n) => panic!("second read on a quiet socket returned Ok({}) instead of a timeout error", n),
        Err(e) => assert!(
            e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut,
            "read timeout surfaced as {:?} ({}), want WouldBlock or TimedOut",
            e.kind(),
            e
        ),
    }
    assert!(
        second_elapsed >= Duration::from_millis(1500) && second_elapsed <= Duration::from_secs(4),
        "second read returned after {:?}, want roughly the 2 s timeout",
        second_elapsed
    );
}

/// With a 2 s read timeout set, peek() on a quiet connected socket must err
/// WouldBlock-or-TimedOut after roughly 2 s (SO_RCVTIMEO applies to MSG_PEEK).
/// XFAIL: the tcp_peek_waiting reaper sits behind the NetPump quiet-iface early-return, so peek never returns, services/net/src/main.rs:1413-1476.
pub fn tcp_peek_timeout_quiet() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    let client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let (served, _peer) = check!(accepted);
    // peer sockets stay open and quiet through the window; ManuallyDrop so
    // the expected NTC-1 panic cannot unwind into a blocking close (NTC-5)
    let served = ManuallyDrop::new(served);
    let listener = ManuallyDrop::new(listener);
    check!(client.set_read_timeout(Some(Duration::from_secs(2))));
    let (result, elapsed, client) = bounded("peek with a 2 s timeout on a quiet socket", 8, move || {
        let started = Instant::now();
        let mut buf = [0u8; 8];
        let r = client.peek(&mut buf);
        (r, started.elapsed(), client)
    });
    discard(client);
    discard(ManuallyDrop::into_inner(served));
    discard(ManuallyDrop::into_inner(listener));
    match result {
        Ok(n) => panic!("peek on a quiet socket returned Ok({}) instead of a timeout error", n),
        Err(e) => assert!(
            e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut,
            "peek timeout surfaced as {:?} ({}), want WouldBlock or TimedOut",
            e.kind(),
            e
        ),
    }
    assert!(
        elapsed >= Duration::from_millis(1500) && elapsed <= Duration::from_secs(4),
        "peek returned after {:?}, want roughly the 2 s timeout",
        elapsed
    );
}

/// With the peer not reading and a 2 s write timeout set, a write past the
/// ~3060 B of buffering must error after roughly the timeout, not park. Kinds:
/// WouldBlock/TimedOut (contract) or BrokenPipe (xous tx maps NetError::TimedOut).
/// XFAIL: the tcp_tx_waiting reaper is skipped by the NetPump quiet-iface early-return, services/net/src/main.rs:1305-1308.
pub fn tcp_write_timeout_full_buffers() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    let client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let (served, _peer) = check!(accepted);
    // the peer must stay open and must NOT read through the whole test, and
    // the expected NTC-1 panic unwinds past this frame: ManuallyDrop (NTC-5)
    let served = ManuallyDrop::new(served);
    let listener = ManuallyDrop::new(listener);
    check!(client.set_write_timeout(Some(Duration::from_secs(2))));
    let (outcome, client) = bounded("write past full buffers with a 2 s write timeout", 10, move || {
        let mut client = client;
        let chunk = [0x5Au8; 512]; // deterministic filler
        let mut total = 0usize;
        let mut verdict = None;
        for i in 1..=32u32 {
            let started = Instant::now();
            match client.write(&chunk) {
                Ok(n) => total += n,
                Err(e) => {
                    verdict = Some((e, started.elapsed()));
                    break;
                }
            }
            if i % 5 == 0 {
                log::info!("write {}: {} bytes accepted so far, still not blocked", i, total);
            }
            if total > 8192 {
                break; // buffering is ~3060 B; way past means writes never block
            }
        }
        ((total, verdict), client)
    });
    discard(client);
    discard(ManuallyDrop::into_inner(served));
    discard(ManuallyDrop::into_inner(listener));
    let (total, verdict) = outcome;
    let (err, err_elapsed) = verdict.unwrap_or_else(|| {
        panic!(
            "writes never blocked or errored: {} bytes accepted with the peer not reading \
             (~3060 B of buffering expected)",
            total
        )
    });
    assert!(
        err.kind() == ErrorKind::WouldBlock
            || err.kind() == ErrorKind::TimedOut
            || err.kind() == ErrorKind::BrokenPipe,
        "write timeout surfaced as {:?} ({}), want WouldBlock/TimedOut (contract) or BrokenPipe (pinned xous tx map)",
        err.kind(),
        err
    );
    assert!(
        err_elapsed >= Duration::from_millis(1500) && err_elapsed <= Duration::from_secs(4),
        "blocked write returned after {:?}, want roughly the 2 s timeout",
        err_elapsed
    );
    log::info!("{} bytes accepted before the blocking write", total);
}

/// Shared body for the two UDP recv-timeout tests: bind a UDP socket on the
/// DUT's own IP (never 127.0.0.1 — NTC-2), set a 2 s read timeout, recv_from on
/// the quiet socket under an 8 s bounded guard, and hand back (result,
/// elapsed); the socket leaks with the worker on the hang path.
fn udp_recv_timeout_quiet_outcome(desc: &str) -> (io::Result<(usize, SocketAddr)>, Duration) {
    let port = next_port();
    let sock = check!(UdpSocket::bind(SocketAddr::new(self_ip(), port)));
    check!(sock.set_read_timeout(Some(Duration::from_secs(2))));
    let (result, elapsed, _sock) = bounded(desc, 8, move || {
        let started = Instant::now();
        let mut buf = [0u8; 32];
        let r = sock.recv_from(&mut buf);
        (r, started.elapsed(), sock)
    });
    (result, elapsed)
}

/// recv_from with a 2 s read timeout on a quiet UDP socket must return an error
/// after roughly 2 s. This owns the returns-at-all + elapsed contract; the
/// error kind is asserted separately by udp_recv_timeout_error_kind.
/// XFAIL: the udp_rx_waiting reaper sits behind the NetPump quiet-iface early-return, so recv_from never returns, services/net/src/main.rs:1561-1590.
pub fn udp_recv_timeout_quiet() {
    let (result, elapsed) =
        udp_recv_timeout_quiet_outcome("udp recv_from with a 2 s timeout on a quiet socket");
    match result {
        Ok((n, from)) => {
            panic!("recv_from on a quiet socket returned Ok(({}, {})) — nobody sent anything", n, from)
        }
        Err(e) => log::info!(
            "recv_from errored as {:?} ({}) after {:?} (kind pinned by udp_recv_timeout_error_kind)",
            e.kind(),
            e,
            elapsed
        ),
    }
    assert!(
        elapsed >= Duration::from_millis(1500) && elapsed <= Duration::from_secs(4),
        "recv_from returned after {:?}, want roughly the 2 s timeout",
        elapsed
    );
}

/// The timeout error from recv_from must be WouldBlock-or-TimedOut, per the
/// contract. Split from the hang layer so the kind is asserted independently.
/// XFAIL: the recv_from never returns (same NetPump quiet-iface early-return), so the kind is never observed, services/net/src/main.rs:1305-1308.
pub fn udp_recv_timeout_error_kind() {
    let (result, _elapsed) = udp_recv_timeout_quiet_outcome("udp recv_from with a 2 s timeout (kind layer)");
    match result {
        Ok((n, from)) => {
            panic!("recv_from on a quiet socket returned Ok(({}, {})) — nobody sent anything", n, from)
        }
        Err(e) => assert!(
            e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut,
            "udp recv timeout surfaced as {:?} ({}), want WouldBlock or TimedOut",
            e.kind(),
            e
        ),
    }
}

/// set/get read & write timeout roundtrips — including reset to None and a
/// large 15410 s value surviving intact — on both TcpStream and UdpSocket.
/// Timeouts are client-side state stored in whole milliseconds.
pub fn timeout_get_set_roundtrip() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    let stream = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let udp = check!(UdpSocket::bind(SocketAddr::new(self_ip(), next_port())));
    let big = Duration::new(15410, 0); // rust std's `timeouts` test value

    // collect every outcome, release the sockets, then assert
    // (collect-discard-assert: a failing assert must not unwind-drop a live
    // TCP socket on the test thread — NTC-5)
    let t_defaults = (stream.read_timeout(), stream.write_timeout());
    let t_set = (stream.set_read_timeout(Some(big)), stream.set_write_timeout(Some(big)));
    let t_get = (stream.read_timeout(), stream.write_timeout());
    let t_clear = (stream.set_read_timeout(None), stream.set_write_timeout(None));
    let t_cleared = (stream.read_timeout(), stream.write_timeout());
    let u_defaults = (udp.read_timeout(), udp.write_timeout());
    let u_set = (udp.set_read_timeout(Some(big)), udp.set_write_timeout(Some(big)));
    let u_get = (udp.read_timeout(), udp.write_timeout());
    let u_clear = (udp.set_read_timeout(None), udp.set_write_timeout(None));
    let u_cleared = (udp.read_timeout(), udp.write_timeout());
    discard(stream);
    discard(listener);
    drop(udp); // UDP closes are synchronous — safe inline

    assert_eq!(check!(t_defaults.0), None, "TcpStream read_timeout default");
    assert_eq!(check!(t_defaults.1), None, "TcpStream write_timeout default");
    check!(t_set.0);
    check!(t_set.1);
    assert_eq!(check!(t_get.0), Some(big), "TcpStream read_timeout after set(15410 s)");
    assert_eq!(check!(t_get.1), Some(big), "TcpStream write_timeout after set(15410 s)");
    check!(t_clear.0);
    check!(t_clear.1);
    assert_eq!(check!(t_cleared.0), None, "TcpStream read_timeout after reset to None");
    assert_eq!(check!(t_cleared.1), None, "TcpStream write_timeout after reset to None");
    assert_eq!(check!(u_defaults.0), None, "UdpSocket read_timeout default");
    assert_eq!(check!(u_defaults.1), None, "UdpSocket write_timeout default");
    check!(u_set.0);
    check!(u_set.1);
    assert_eq!(check!(u_get.0), Some(big), "UdpSocket read_timeout after set(15410 s)");
    assert_eq!(check!(u_get.1), Some(big), "UdpSocket write_timeout after set(15410 s)");
    check!(u_clear.0);
    check!(u_clear.1);
    assert_eq!(check!(u_cleared.0), None, "UdpSocket read_timeout after reset to None");
    assert_eq!(check!(u_cleared.1), None, "UdpSocket write_timeout after reset to None");
}

/// set_read_timeout/set_write_timeout(Some(ZERO)) must fail with InvalidInput
/// on both protocols — Duration::ZERO is reserved to mean "unset". The check is
/// client-side and never reaches the server.
pub fn timeout_zero_duration_invalid_input() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    let stream = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let udp = check!(UdpSocket::bind(SocketAddr::new(self_ip(), next_port())));
    let outcomes = vec![
        ("TcpStream::set_read_timeout", stream.set_read_timeout(Some(Duration::ZERO))),
        ("TcpStream::set_write_timeout", stream.set_write_timeout(Some(Duration::ZERO))),
        ("UdpSocket::set_read_timeout", udp.set_read_timeout(Some(Duration::ZERO))),
        ("UdpSocket::set_write_timeout", udp.set_write_timeout(Some(Duration::ZERO))),
    ];
    let after = (stream.read_timeout(), udp.read_timeout());
    discard(stream);
    discard(listener);
    drop(udp); // UDP closes are synchronous — safe inline
    for (what, r) in outcomes {
        match r {
            Ok(()) => panic!("{}(Some(ZERO)) unexpectedly succeeded, want InvalidInput", what),
            Err(e) => assert_eq!(
                e.kind(),
                ErrorKind::InvalidInput,
                "{}(Some(ZERO)) surfaced as {:?} ({}), want InvalidInput",
                what,
                e.kind(),
                e
            ),
        }
    }
    // the rejected set must not have clobbered the (default None) state
    assert_eq!(check!(after.0), None, "TcpStream read_timeout after a rejected zero set");
    assert_eq!(check!(after.1), None, "UdpSocket read_timeout after a rejected zero set");
}

/// Deviation-pin: Some(1 ns) is a valid std timeout, but the xous client stores
/// timeouts via as_millis(), truncating 1 ns to the 0-ms "unset" sentinel, so
/// the setter returns Ok and the getter reports None. Getter assertions only.
pub fn timeout_submillisecond_getter_none() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    let stream = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let udp = check!(UdpSocket::bind(SocketAddr::new(self_ip(), next_port())));
    let one_ns = Duration::from_nanos(1);
    let set_r = stream.set_read_timeout(Some(one_ns));
    let get_r = stream.read_timeout();
    let set_w = stream.set_write_timeout(Some(one_ns));
    let get_w = stream.write_timeout();
    let set_u = udp.set_read_timeout(Some(one_ns));
    let get_u = udp.read_timeout();
    discard(stream);
    discard(listener);
    drop(udp); // UDP closes are synchronous — safe inline
    check!(set_r);
    check!(set_w);
    check!(set_u);
    assert_eq!(check!(get_r), None, "pinned: TcpStream read_timeout(1 ns) truncates to the None sentinel");
    assert_eq!(check!(get_w), None, "pinned: TcpStream write_timeout(1 ns) truncates to the None sentinel");
    assert_eq!(check!(get_u), None, "pinned: UdpSocket read_timeout(1 ns) truncates to the None sentinel");
}

/// connect_timeout(500 ms) to an off-subnet blackhole must err after >= the
/// timeout and within budget; the pinned kind is AddrNotAvailable (contract:
/// TimedOut). Quarantined at the theme tail — it leaks a SynSent socket.
/// XFAIL: the connect-timeout abort is serviced only via the NetPump connect scan, skipped by the quiet-iface early-return, services/net/src/main.rs:1336-1362.
pub fn tcp_connect_timeout_fires() {
    let addr = SocketAddr::new(IpAddr::V4(BLACKHOLE), 1);
    let (result, elapsed) = bounded("connect_timeout(500 ms) to a blackhole", 10, move || {
        let started = Instant::now();
        let r = TcpStream::connect_timeout(&addr, Duration::from_millis(500));
        (r, started.elapsed())
    });
    match result {
        Ok(s) => {
            discard(s);
            panic!("connect_timeout to a blackhole unexpectedly succeeded");
        }
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::AddrNotAvailable,
            "blackhole connect_timeout surfaced as {:?} ({}), want the pinned AddrNotAvailable \
             (the contract kind would be TimedOut)",
            e.kind(),
            e
        ),
    }
    assert!(
        elapsed >= Duration::from_millis(450) && elapsed <= Duration::from_secs(5),
        "connect_timeout(500 ms) returned after {:?}, want >= the timeout and well under the guard",
        elapsed
    );
}

/// Documented-hang pin: a PLAIN connect() to the blackhole parks forever — the
/// client sends timeout 0, so smoltcp sets no socket timeout and never gives up
/// SYN retransmission; PASSES only if the connect is still parked after a 10 s inverted guard, any completion means reclassify.
pub fn tcp_plain_connect_blackhole_parks() {
    let addr = SocketAddr::new(IpAddr::V4(BLACKHOLE), 2);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        tx.send(TcpStream::connect(addr)).ok();
    });
    match rx.recv_timeout(Duration::from_secs(10)) {
        Err(RecvTimeoutError::Timeout) => {
            log::info!(
                "plain blackhole connect still parked after 10 s — pinned behavior holds; leaking the worker"
            );
        }
        Ok(Ok(s)) => {
            discard(s);
            panic!(
                "plain connect to a blackhole succeeded — the documented park-forever pin no longer \
                 holds; reclassify TO-10"
            );
        }
        Ok(Err(e)) => panic!(
            "plain connect to a blackhole returned {:?} ({}) within 10 s — the documented \
             park-forever pin no longer holds (smoltcp SYN give-up?); reclassify TO-10",
            e.kind(),
            e
        ),
        Err(RecvTimeoutError::Disconnected) => panic!("blackhole connect worker panicked"),
    }
}

/// DANGER — disabled, NOT registered: asserts a connect timeout must affect
/// only the connect, not linger as a session timeout. Disabled because the
/// reproducer WEDGES rather than failing cleanly (the aborted connection's tx
/// waiter never completes, NTC-1), and the runner counts any wedge as a hard fail.
#[allow(dead_code)]
pub fn connect_timeout_not_a_session_timeout() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    let client = check!(bounded("connect_timeout(1 s) to a live listener", 10, move || {
        TcpStream::connect_timeout(&addr, Duration::from_secs(1))
    }));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let (served, _peer) = check!(accepted);
    // guard-panic paths below must not unwind-drop the listener (NTC-5)
    let listener = ManuallyDrop::new(listener);

    // 4096 B > the ~3060 B of buffering (1530 tx + 1530 peer rx)
    let mut payload = vec![0u8; 4096];
    XorShift::new(0x7011).fill(&mut payload);
    let expected = payload.clone();

    // hand-rolled worker instead of bounded(): bounded blocks the test
    // thread, which must spend the next 3 s stalling as the non-reading peer
    let (wtx, wrx) = mpsc::channel();
    thread::spawn(move || {
        let mut client = client;
        let r = client.write_all(&payload);
        wtx.send((r, client)).ok();
    });

    log::info!("peer stalling 3 s (virtual) with client tx data pending");
    thread::sleep(Duration::from_secs(3));
    log::info!("stall over, draining 4096 B");

    let (drained, served) = bounded("peer drain after the 3 s stall", 10, move || {
        let mut served = served;
        let mut got = Vec::with_capacity(4096);
        let mut buf = [0u8; 1024];
        let mut chunks = 0usize;
        let r = loop {
            match served.read(&mut buf) {
                Ok(0) => break Err(format!("EOF after {} bytes — connection died mid-stall", got.len())),
                Ok(n) => {
                    got.extend_from_slice(&buf[..n]);
                    chunks += 1;
                    if chunks % 5 == 0 {
                        log::info!("drain: {} chunks, {} bytes", chunks, got.len());
                    }
                    if got.len() >= 4096 {
                        break Ok(got);
                    }
                }
                Err(e) => break Err(format!("peer read failed after the stall: {}", e)),
            }
        };
        (r, served)
    });
    let write_outcome = match wrx.recv_timeout(Duration::from_secs(10)) {
        Ok((r, client)) => {
            discard(client);
            Some(r)
        }
        // writer thread and its socket leak (ports are never reused)
        Err(RecvTimeoutError::Timeout) => None,
        Err(RecvTimeoutError::Disconnected) => panic!("writer thread panicked"),
    };
    discard(served);
    discard(ManuallyDrop::into_inner(listener));
    match write_outcome {
        None => panic!(
            "PROBE TO-11: write_all(4096) never completed within 10 s of the drain — possible \
             lingering-session-timeout abort with the tx waiter stranded (writer leaked)"
        ),
        Some(Err(e)) => panic!(
            "PROBE TO-11: write_all failed with {:?} ({}) — the connect timeout appears to persist \
             as a session timeout",
            e.kind(),
            e
        ),
        Some(Ok(())) => {}
    }
    match drained {
        Ok(got) => assert_eq!(got, expected, "payload corrupted across the 3 s stall"),
        Err(msg) => panic!("PROBE TO-11: {}", msg),
    }
}

/// Regression guard — MUST stay the FINAL registered test in the suite: a loopback
/// connect to a live listener must still succeed after the two preceding blackhole
/// tests each leak a SynSent socket; its 10 s bounded guard fires on any post-blackhole connect jam.
pub fn tcp_connect_after_blackhole_still_works() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    // if a jam ever reintroduces itself the connect panics out of bounded()
    // past this frame: park the listener in ManuallyDrop so the unwind cannot
    // block on a close (NTC-5) — leaked on the panic path, discarded on the
    // happy path (the one-socket listener completes the handshake without an
    // accept)
    let listener = ManuallyDrop::new(listener);
    let client = check!(bounded("connect after blackhole", 10, move || TcpStream::connect(addr)));
    discard(client);
    discard(ManuallyDrop::into_inner(listener));
}

/// This theme's registry (aggregated by tests::all_tests / all_xfails).
pub const TESTS: &[TestEntry] = &[
    ("timeouts::tcp_read_timeout_after_data", tcp_read_timeout_after_data as fn()),
    ("timeouts::tcp_peek_timeout_quiet", tcp_peek_timeout_quiet as fn()),
    ("timeouts::tcp_write_timeout_full_buffers", tcp_write_timeout_full_buffers as fn()),
    ("timeouts::udp_recv_timeout_quiet", udp_recv_timeout_quiet as fn()),
    ("timeouts::udp_recv_timeout_error_kind", udp_recv_timeout_error_kind as fn()),
    ("timeouts::timeout_get_set_roundtrip", timeout_get_set_roundtrip as fn()),
    ("timeouts::timeout_zero_duration_invalid_input", timeout_zero_duration_invalid_input as fn()),
    ("timeouts::timeout_submillisecond_getter_none", timeout_submillisecond_getter_none as fn()),
    // connect_timeout_not_a_session_timeout (NTC-20) is DISABLED, not
    // registered — it wedges rather than failing cleanly (see its doc comment).
    // The two blackhole tests below leak SynSent sockets, so
    // tcp_connect_after_blackhole_still_works stays the FINAL entry as a guard.
    ("timeouts::tcp_connect_timeout_fires", tcp_connect_timeout_fires as fn()),
    ("timeouts::tcp_plain_connect_blackhole_parks", tcp_plain_connect_blackhole_parks as fn()),
    ("timeouts::tcp_connect_after_blackhole_still_works", tcp_connect_after_blackhole_still_works as fn()),
];

pub const XFAILS: &[XfailEntry] = &[
    ("timeouts::tcp_read_timeout_after_data", "NTC-1"),
    ("timeouts::tcp_peek_timeout_quiet", "NTC-1"),
    ("timeouts::tcp_write_timeout_full_buffers", "NTC-1"),
    ("timeouts::udp_recv_timeout_quiet", "NTC-1"),
    ("timeouts::udp_recv_timeout_error_kind", "NTC-1"),
    ("timeouts::tcp_connect_timeout_fires", "NTC-1"),
];
