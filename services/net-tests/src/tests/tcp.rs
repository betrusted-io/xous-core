//! tcp theme — TCP loopback semantics beyond the smoke seeds: EOF/half-close,
//! short writes against the 1530-byte buffers (TCP_BUFFER_SIZE = NET_MTU),
//! peek, listener lifecycle, clones, vectored/zero-length ops, blocked writes.
//!
//! Live constraints (see also tests/mod.rs): accepted sockets auto-close on
//! remote FIN (tcp_server_remote_close_poll, main.rs); idle TCP closes hang
//! (NTC-5), so a must-be-GONE listener closes via `close_listener_with_pump_kick`
//! and a non-load-bearing idle one is `discard`ed (worker may leak; ports never reuse).

use core::mem::ManuallyDrop;
use std::io::{ErrorKind, IoSlice, IoSliceMut, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::harness::{XorShift, bounded, check, discard, echo_server, next_port, self_ip};
use crate::tests::{TestEntry, XfailEntry};

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// bind + connect + accept on a fresh port; the standard three-socket fixture.
/// Panics (via check!/bounded) on any setup failure.
fn connected_pair() -> (TcpStream, TcpStream, TcpListener, SocketAddr) {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    let client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let (served, _peer) = check!(accepted);
    (client, served, listener, addr)
}

/// Close a TCP listener and force the close to COMPLETE: StdTcpClose parks until
/// a pump pass where iface.poll() reports activity (NTC-5), so a UDP kicker fires
/// at a dead port every 100 ms until it lands; panics (leaking the closer) if not.
fn close_listener_with_pump_kick(desc: &str, listener: TcpListener) {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        drop(listener); // blocking StdTcpClose
        tx.send(()).ok();
    });
    let kicker = check!(UdpSocket::bind(SocketAddr::new(self_ip(), next_port())));
    let dead = SocketAddr::new(self_ip(), next_port()); // nobody listens: fire-and-forget (U-7)
    for kick in 0..100u32 {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(()) => {
                log::info!("{}: close completed after {} pump kicks", desc, kick);
                return; // kicker's UDP drop is synchronous and safe here
            }
            Err(_) => {}
        }
        kicker.send_to(&[0xEE], dead).ok();
        if (kick + 1) % 5 == 0 {
            log::info!("{}: pump kick {} (close still pending)", desc, kick + 1);
        }
    }
    panic!("{}: listener close did not complete within 100 pump kicks (~10 s)", desc);
}

/// After the peer drops (FIN), read returns Ok(0), and Ok(0) again on repeat.
/// XFAIL: a read issued after the FIN parks forever, the same rx-completion gap
/// as smoke::tcp_drop_close_delivers_eof, services/net/src/main.rs.
pub fn tcp_read_eof_after_peer_drop() {
    let (client, served, listener, _addr) = connected_pair();
    discard(client); // FIN toward the accepted socket
    thread::sleep(Duration::from_secs(2)); // let the FIN (and any auto-close) land first
    // expected-failure paths panic below; keep the listener out of unwind-drop
    let listener = ManuallyDrop::new(listener);
    let (first, second, served) = bounded("read after peer drop", 10, move || {
        let mut served = served;
        let mut buf = [0u8; 8];
        let first = served.read(&mut buf);
        let second = served.read(&mut buf);
        (first, second, served)
    });
    discard(served);
    discard(ManuallyDrop::into_inner(listener));
    assert_eq!(check!(first), 0, "read after peer drop must return EOF (Ok(0))");
    assert_eq!(check!(second), 0, "repeated read after EOF must return Ok(0) again");
}

/// Writes on the accepted socket after the client drops must eventually fail
/// (early Ok()s allowed; xous-pinned kind BrokenPipe).
/// XFAIL: the write parks in tcp_tx_waiting forever, the state-Closed tx reaper never running on a quiet pump, services/net/src/main.rs.
pub fn tcp_write_after_peer_drop() {
    let (client, served, listener, _addr) = connected_pair();
    discard(client); // FIN + auto-close of `served`
    thread::sleep(Duration::from_secs(2));
    let listener = ManuallyDrop::new(listener);
    let (outcome, served) = bounded("writes after peer drop", 15, move || {
        let mut served = served;
        let mut outcome = None;
        for i in 0..30u32 {
            match served.write(&[0x5A; 32]) {
                Ok(_) => {
                    if (i + 1) % 5 == 0 {
                        log::info!("write {} after peer drop still Ok", i + 1);
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    outcome = Some((i, e));
                    break;
                }
            }
        }
        (outcome, served)
    });
    discard(served);
    discard(ManuallyDrop::into_inner(listener));
    match outcome {
        None => panic!("30 writes after peer drop all returned Ok — want an eventual BrokenPipe"),
        Some((i, e)) => assert_eq!(
            e.kind(),
            ErrorKind::BrokenPipe,
            "write {} after peer drop failed with {:?} ({}), want the pinned BrokenPipe",
            i,
            e.kind(),
            e
        ),
    }
}

/// With 1 byte pending, a read into a 10-byte buffer returns Ok(1) immediately
/// instead of blocking for more.
pub fn tcp_partial_read() {
    let (mut client, served, listener, _addr) = connected_pair();
    check!(client.write_all(&[0xC3]));
    let (result, buf, served) = bounded("partial read", 10, move || {
        let mut served = served;
        let mut buf = [0u8; 10];
        let r = served.read(&mut buf);
        (r, buf, served)
    });
    discard(client);
    discard(served);
    discard(listener);
    assert_eq!(check!(result), 1, "read with a 10-byte buffer must return the single pending byte");
    assert_eq!(buf[0], 0xC3, "partial read returned the wrong byte");
}

/// 64 KiB each direction through the echo server, verifying end-to-end
/// integrity across the 1530-byte buffers. Writer (try_clone) and reader run
/// concurrently — sequential write-then-read deadlocks past ~3060 bytes in flight.
pub fn tcp_large_transfer_echo_64k() {
    const CHUNK: usize = 1024;
    const CHUNKS: usize = 64;
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    // echo thread + listener leak by design: EOF teardown is pinned separately
    // (smoke::tcp_drop_close_delivers_eof, NTC-3)
    let _server = echo_server(listener);
    let client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let writer_half = check!(client.try_clone());
    let seed = 0x64AB_0000 ^ port as u32;
    let (write_res, read_res, client, writer_half) = bounded("64 KiB echo transfer", 120, move || {
        let (wtx, wrx) = mpsc::channel();
        thread::spawn(move || {
            let mut writer = writer_half;
            let mut rng = XorShift::new(seed);
            let mut chunk = vec![0u8; CHUNK];
            let res: Result<(), String> = (|| {
                for i in 0..CHUNKS {
                    rng.fill(&mut chunk);
                    writer.write_all(&chunk).map_err(|e| format!("write chunk {}: {}", i, e))?;
                    if (i + 1) % 5 == 0 {
                        log::info!("64k writer: {} KiB sent", i + 1);
                    }
                }
                Ok(())
            })();
            wtx.send((res, writer)).ok();
        });
        let mut client = client;
        let mut rng = XorShift::new(seed);
        let mut expect = vec![0u8; CHUNK];
        let mut got = vec![0u8; CHUNK];
        let read_res: Result<(), String> = (|| {
            for i in 0..CHUNKS {
                rng.fill(&mut expect);
                client.read_exact(&mut got).map_err(|e| format!("read chunk {}: {}", i, e))?;
                if got != expect {
                    return Err(format!("chunk {} corrupted in the 64 KiB echo", i));
                }
                if (i + 1) % 5 == 0 {
                    log::info!("64k reader: {} KiB verified", i + 1);
                }
            }
            Ok(())
        })();
        let (write_res, writer_half) = match wrx.recv_timeout(Duration::from_secs(60)) {
            Ok((r, w)) => (r, Some(w)),
            Err(_) => (Err("writer thread never reported (stalled?)".to_string()), None),
        };
        (write_res, read_res, client, writer_half)
    });
    discard(client);
    if let Some(w) = writer_half {
        discard(w);
    }
    check!(write_res);
    check!(read_res);
}

/// A single write() may be short: an 8192-byte write returns 0 < n <= 1530
/// (client one-page cap plus the 1530-byte tx buffer), and the peer drains and
/// verifies exactly n bytes.
pub fn tcp_single_write_is_short() {
    let (client, served, listener, _addr) = connected_pair();
    let mut payload = vec![0u8; 8192];
    XorShift::new(0x51C0_0DE1).fill(&mut payload);
    let expected = payload.clone();
    let (wrote, client) = bounded("single 8 KiB write", 10, move || {
        let mut client = client;
        let r = client.write(&payload);
        (r, client)
    });
    let n = match wrote {
        Ok(n) => n,
        Err(e) => {
            discard(client);
            discard(served);
            discard(listener);
            panic!("single 8 KiB write failed with: {}", e);
        }
    };
    let (drained, served) = bounded("drain the short write", 10, move || {
        let mut served = served;
        let mut got = vec![0u8; n];
        let r = served.read_exact(&mut got).map(|_| got);
        (r, served)
    });
    discard(client);
    discard(served);
    discard(listener);
    assert!(n > 0 && n <= 1530, "single write returned {} bytes, want 0 < n <= 1530", n);
    assert_eq!(check!(drained), expected[..n], "short-write payload corrupted");
}

/// Peek twice returns the same 4 bytes without consuming them, a read still
/// gets them, and a nonblocking peek on an empty queue returns WouldBlock.
pub fn tcp_peek_does_not_consume() {
    let (client, served, listener, _addr) = connected_pair();
    let mut served = ManuallyDrop::new(served);
    let listener = ManuallyDrop::new(listener);
    check!(served.write_all(b"peek"));
    let (peek1, peek2, readback, client) = bounded("peek twice then read", 15, move || {
        let mut client = client;
        let mut p1 = [0u8; 4];
        // wait until all 4 bytes are pending so both peeks see identical data
        let mut tries = 0u32;
        let peek1 = loop {
            match client.peek(&mut p1) {
                Ok(4) => break Ok(4usize),
                Ok(n) => {
                    tries += 1;
                    if tries % 5 == 0 {
                        log::info!("peek warmup: {} of 4 bytes pending (try {})", n, tries);
                    }
                    if tries >= 50 {
                        break Ok(n);
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => break Err(e),
            }
        };
        let mut p2 = [0u8; 4];
        let peek2 = client.peek(&mut p2);
        let mut r = [0u8; 4];
        let readback = client.read_exact(&mut r).map(|_| r);
        (peek1.map(|n| (n, p1)), peek2.map(|n| (n, p2)), readback, client)
    });
    // nonblocking peek returns immediately: safe on the test thread
    check!(client.set_nonblocking(true));
    let mut b = [0u8; 4];
    let empty_peek = client.peek(&mut b);
    discard(client);
    discard(ManuallyDrop::into_inner(served));
    discard(ManuallyDrop::into_inner(listener));
    let (n1, p1) = check!(peek1);
    let (n2, p2) = check!(peek2);
    assert_eq!((n1, &p1), (4, b"peek"), "first peek");
    assert_eq!((n2, &p2), (4, b"peek"), "second peek must see the same unconsumed bytes");
    assert_eq!(&check!(readback), b"peek", "read after two peeks must still get the bytes");
    match empty_peek {
        Ok(n) => panic!("nonblocking peek on an empty queue returned Ok({})", n),
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::WouldBlock,
            "nonblocking empty peek surfaced as {:?} ({}), want WouldBlock",
            e.kind(),
            e
        ),
    }
}

/// With 4 bytes pending, a peek into a 2-byte buffer returns Ok(2) and a
/// following 4-byte read still gets all 4 — peek leaves the queue intact.
pub fn tcp_peek_shorter_buffer() {
    let (client, served, listener, _addr) = connected_pair();
    let mut served = ManuallyDrop::new(served);
    let listener = ManuallyDrop::new(listener);
    check!(served.write_all(b"wxyz"));
    let (short_peek, readback, client) = bounded("short peek then full read", 15, move || {
        let mut client = client;
        // warm up until all 4 bytes are pending (determinism for the 2-byte peek)
        let mut warm = [0u8; 4];
        let mut tries = 0u32;
        loop {
            match client.peek(&mut warm) {
                Ok(4) => break,
                Ok(n) => {
                    tries += 1;
                    if tries % 5 == 0 {
                        log::info!("short-peek warmup: {} of 4 bytes pending (try {})", n, tries);
                    }
                    if tries >= 50 {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                Err(_) => break, // surfaced again by the asserts below
            }
        }
        let mut two = [0u8; 2];
        let short_peek = client.peek(&mut two).map(|n| (n, two));
        let mut four = [0u8; 4];
        let readback = client.read_exact(&mut four).map(|_| four);
        (short_peek, readback, client)
    });
    discard(client);
    discard(ManuallyDrop::into_inner(served));
    discard(ManuallyDrop::into_inner(listener));
    let (n, two) = check!(short_peek);
    assert_eq!((n, &two), (2, b"wx"), "2-byte peek of 4 pending bytes");
    assert_eq!(&check!(readback), b"wxyz", "read after a short peek must return the whole queue");
}

/// Half-close: the client sends a request then drops (FIN); the server reads it,
/// sees EOF, and its reply write must succeed (full drop since shutdown(Write) emits no FIN — NTC-4).
/// XFAIL: the request read itself parks forever, the same FIN-adjacent rx gap as smoke::tcp_drop_close_delivers_eof, services/net/src/main.rs.
pub fn tcp_half_close_server_replies_after_client_fin() {
    let (mut client, served, listener, _addr) = connected_pair();
    check!(client.write_all(b"req?"));
    discard(client); // FIN right behind the request
    thread::sleep(Duration::from_secs(2));
    let listener = ManuallyDrop::new(listener);
    let (request, eof, reply, served) = bounded("read request+EOF then reply", 15, move || {
        let mut served = served;
        let mut req = [0u8; 4];
        let request = served.read_exact(&mut req).map(|_| req);
        let mut b = [0u8; 8];
        let eof = served.read(&mut b);
        let reply = served.write_all(b"resp");
        (request, eof, reply, served)
    });
    discard(served);
    discard(ManuallyDrop::into_inner(listener));
    assert_eq!(&check!(request), b"req?", "request bytes sent just before the FIN");
    assert_eq!(check!(eof), 0, "read after the request must see EOF (Ok(0))");
    check!(reply);
}

/// Reverse half-close: the accepted stream drops (graceful FIN), the client
/// reads Ok(0), and its first small write after EOF still completes locally.
/// XFAIL: the client read parks forever; the EOF gap is in the rx wait path itself, not the accepted-socket auto-close, services/net/src/main.rs.
pub fn tcp_half_close_server_fin_client_still_writes() {
    let (client, served, listener, _addr) = connected_pair();
    discard(served); // graceful FIN from the accepted side
    thread::sleep(Duration::from_secs(2));
    let listener = ManuallyDrop::new(listener);
    let (eof, first_write, client) = bounded("client read EOF after server drop", 10, move || {
        let mut client = client;
        let mut b = [0u8; 8];
        let eof = client.read(&mut b);
        let first_write = client.write(b"after-fin");
        (eof, first_write, client)
    });
    discard(client);
    discard(ManuallyDrop::into_inner(listener));
    assert_eq!(check!(eof), 0, "client read after server drop must see EOF (Ok(0))");
    let n = check!(first_write);
    assert!(n > 0, "first small write after EOF returned Ok({}), want a local success", n);
}

/// Deviation-pin: a second shutdown() in the same direction returns Ok — the
/// server's StdTcpStreamShutdown unconditionally return_scalar(1). The std
/// contract only requires no crash, so pinning Ok stays within contract.
pub fn tcp_double_shutdown_is_ok() {
    let (client, served, listener, _addr) = connected_pair();
    let w1 = client.shutdown(Shutdown::Write);
    let w2 = client.shutdown(Shutdown::Write);
    let r1 = client.shutdown(Shutdown::Read);
    let r2 = client.shutdown(Shutdown::Read);
    discard(client);
    discard(served);
    discard(listener);
    check!(w1);
    check!(w2);
    check!(r1);
    check!(r2);
}

/// Name bookkeeping across a connection: listener.local_addr() equals the bound
/// address, stream.peer_addr() == listener.local_addr(), and accepted.peer_addr()
/// == stream.local_addr(). Adds the listener query (StdGetAddress) over smoke.
pub fn tcp_socket_and_peer_name() {
    let port = next_port();
    let bind_addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(bind_addr));
    let l_local = listener.local_addr();
    let client = check!(bounded("connect", 10, move || TcpStream::connect(bind_addr)));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let (served, _peer) = check!(accepted);
    let c_local = client.local_addr();
    let c_peer = client.peer_addr();
    let s_local = served.local_addr();
    let s_peer = served.peer_addr();
    discard(client);
    discard(served);
    discard(listener);
    let l_local = check!(l_local);
    assert_eq!(l_local, bind_addr, "listener.local_addr() vs the bound address");
    assert_eq!(check!(c_peer), l_local, "stream.peer_addr() vs listener.local_addr()");
    assert_eq!(check!(s_local), bind_addr, "accepted.local_addr() vs the bound address");
    assert_eq!(check!(s_peer), check!(c_local), "accepted.peer_addr() vs stream.local_addr()");
}

/// Binding port 0 assigns a real ephemeral port in [49152, 65535] and two
/// listeners get distinct ports (the only sanctioned exception to next_port()).
/// The distinct-ports assert has 1-in-16384 collision odds — a lone failure means rerun, a repeat means broken trng.
pub fn tcp_listener_port_zero_assigned() {
    let l1 = check!(TcpListener::bind(SocketAddr::new(LOOPBACK, 0)));
    let a1 = l1.local_addr();
    let l2 = check!(TcpListener::bind(SocketAddr::new(LOOPBACK, 0)));
    let a2 = l2.local_addr();
    discard(l1);
    discard(l2);
    let a1 = check!(a1);
    let a2 = check!(a2);
    assert!(
        (49152..=65535).contains(&a1.port()),
        "assigned port {} outside the documented ephemeral range [49152, 65535]",
        a1.port()
    );
    assert!(
        (49152..=65535).contains(&a2.port()),
        "assigned port {} outside the documented ephemeral range [49152, 65535]",
        a2.port()
    );
    assert_ne!(a1.port(), a2.port(), "two port-0 listeners drew the same ephemeral port");
}

/// connect_timeout(addr, Duration::MAX) to a live listener must succeed, not
/// overflow into an immediate failure.
/// XFAIL: the saturated u64 timeout-ms overflow smoltcp's i64-ms Instant math and poison the connect before the SYN, services/net/src/std_tcpstream.rs.
pub fn tcp_connect_timeout_duration_max_ok() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    let result = bounded("connect_timeout(Duration::MAX)", 10, move || {
        TcpStream::connect_timeout(&addr, Duration::MAX)
    });
    let verdict = match result {
        Ok(s) => {
            discard(s);
            Ok(())
        }
        Err(e) => Err(e),
    };
    discard(listener);
    check!(verdict);
}

/// Five serial connections each accepted through the incoming() iterator,
/// echoing one byte both ways. Exercises the xous listener-replenish path:
/// accept() converts the listener fd to the stream fd and rebinds a replacement.
pub fn tcp_incoming_iterator_serial() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    // ManuallyDrop so a failing round cannot unwind-drop the listener on the
    // test thread (blocking close, NTC-5); explicitly discarded on success
    let mut listener = ManuallyDrop::new(Some(check!(TcpListener::bind(addr))));
    for round in 0u8..5 {
        log::info!("incoming round {}: connecting", round);
        let mut client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
        let l = listener.take().unwrap();
        let (accepted, l) = bounded("incoming().next()", 10, move || {
            let r = l.incoming().next().expect("incoming() returned None for a listener");
            (r, l)
        });
        *listener = Some(l);
        let served = check!(accepted);
        check!(client.write_all(&[round]));
        let (server_read, mut served) = bounded("server read", 10, move || {
            let mut served = served;
            let mut b = [0u8; 1];
            let r = served.read_exact(&mut b).map(|_| b[0]);
            (r, served)
        });
        let reply = served.write_all(&[round ^ 0xAA]);
        let (client_read, client) = bounded("client read", 10, move || {
            let mut client = client;
            let mut b = [0u8; 1];
            let r = client.read_exact(&mut b).map(|_| b[0]);
            (r, client)
        });
        discard(served);
        discard(client);
        assert_eq!(check!(server_read), round, "server byte in round {}", round);
        check!(reply);
        assert_eq!(check!(client_read), round ^ 0xAA, "client byte in round {}", round);
        log::info!("incoming round {}: done", round);
    }
    discard(listener.take().unwrap());
}

/// Deviation-pin: a second bind of a bound port should fail EADDRINUSE, but on
/// xous BOTH binds succeed — each bind makes a fresh smoltcp socket and nothing
/// checks for cross-socket port conflicts; a conflict check would flip this to Err.
pub fn tcp_double_bind_same_port() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let first = check!(TcpListener::bind(addr));
    let second = TcpListener::bind(addr);
    let verdict = match second {
        Ok(l) => {
            discard(l); // idle listener: worker may leak (NTC-5)
            Ok(())
        }
        Err(e) => Err((e.kind(), e.to_string())),
    };
    discard(first);
    match verdict {
        Ok(()) => {
            log::info!("second bind of {} succeeded (pinned: no cross-socket conflict detection)", addr)
        }
        Err((kind, msg)) => panic!(
            "second bind of {} was refused with {:?} ({}) — the no-conflict-detection pin no longer holds; \
             re-point this test at the contract (AddrInUse) and drop the pin",
            addr, kind, msg
        ),
    }
}

/// After tearing a listener down, an immediate rebind of the same port succeeds
/// and the rebound listener actually serves — a stale Listen socket from an
/// incomplete close would steal the SYN and park the accept (pump-kicker closes).
pub fn tcp_fast_rebind_after_close() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    // round 1: give the sockets traffic so their closes can complete (NTC-5)
    let mut client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let (served, _peer) = check!(accepted);
    check!(client.write_all(&[0x17]));
    let (got1, served) = bounded("server read (round 1)", 10, move || {
        let mut served = served;
        let mut b = [0u8; 1];
        let r = served.read_exact(&mut b).map(|_| b[0]);
        (r, served)
    });
    discard(served);
    discard(client);
    // the rebind is only meaningful once the old listener socket is GONE
    close_listener_with_pump_kick("T-17 old listener", listener);
    let relisten = check!(TcpListener::bind(addr)); // the immediate rebind under test
    // round 2: the rebound listener must actually serve
    let mut client2 = check!(bounded("connect after rebind", 10, move || TcpStream::connect(addr)));
    let (accepted2, relisten) = bounded("accept after rebind", 10, move || {
        let r = relisten.accept();
        (r, relisten)
    });
    let (served2, _peer) = check!(accepted2);
    check!(client2.write_all(&[0x71]));
    let (got2, served2) = bounded("server read (round 2)", 10, move || {
        let mut served2 = served2;
        let mut b = [0u8; 1];
        let r = served2.read_exact(&mut b).map(|_| b[0]);
        (r, served2)
    });
    discard(served2);
    discard(client2);
    discard(relisten);
    assert_eq!(check!(got1), 0x17, "pre-close roundtrip byte");
    assert_eq!(check!(got2), 0x71, "post-rebind roundtrip byte");
}

/// Deviation-pin: connecting to a just-dropped listener's port fails fast; the
/// contract kind is ConnectionRefused but on this toolchain it decodes as
/// AddrNotAvailable (NTC-6 family, pinned here). Teardown uses the pump-kicker.
pub fn tcp_connect_to_dropped_listener_refused() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    // give the connection sockets traffic so their closes complete (NTC-5)
    let mut client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let (served, _peer) = check!(accepted);
    check!(client.write_all(&[0x18]));
    let (got, served) = bounded("server read", 10, move || {
        let mut served = served;
        let mut b = [0u8; 1];
        let r = served.read_exact(&mut b).map(|_| b[0]);
        (r, served)
    });
    discard(served);
    discard(client);
    assert_eq!(check!(got), 0x18, "setup roundtrip byte");
    close_listener_with_pump_kick("T-18 listener", listener);
    let started = Instant::now();
    let result = bounded("connect to the dropped listener's port", 10, move || {
        TcpStream::connect_timeout(&addr, Duration::from_secs(5))
    });
    let elapsed = started.elapsed();
    match result {
        Ok(s) => {
            discard(s);
            panic!("connect to a just-dropped listener's port unexpectedly succeeded");
        }
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::AddrNotAvailable,
            "dropped-listener connect surfaced as {:?} ({}), want the pinned AddrNotAvailable \
             (contract kind on fix-day: ConnectionRefused)",
            e.kind(),
            e
        ),
    }
    assert!(elapsed < Duration::from_secs(4), "refusal took {:?}, want well under the 5 s budget", elapsed);
}

/// read(&mut []) with data already pending returns Ok(0) promptly and does not
/// consume the pending byte; the follow-up read proves it was left intact.
pub fn tcp_read_zero_len_buffer_pending() {
    let (client, served, listener, _addr) = connected_pair();
    let mut served = ManuallyDrop::new(served);
    let listener = ManuallyDrop::new(listener);
    check!(served.write_all(&[0x77]));
    thread::sleep(Duration::from_secs(1)); // let the byte land in the rx buffer
    let (zero, follow, client) = bounded("zero-len read with pending data", 10, move || {
        let mut client = client;
        let zero = client.read(&mut []);
        let mut b = [0u8; 1];
        let follow = client.read_exact(&mut b).map(|_| b[0]);
        (zero, follow, client)
    });
    discard(client);
    discard(ManuallyDrop::into_inner(served));
    discard(ManuallyDrop::into_inner(listener));
    assert_eq!(check!(zero), 0, "read(&mut []) with pending data must return Ok(0)");
    assert_eq!(check!(follow), 0x77, "zero-len read must not consume the pending byte");
}

/// read(&mut []) with NO pending data must still return Ok(0) promptly.
/// XFAIL: the server parks any read while can_recv() is false without checking
/// the buffer length, so a zero-length read waits forever, services/net/src/std_tcpstream.rs.
pub fn tcp_read_zero_len_buffer_quiet() {
    let (client, served, listener, _addr) = connected_pair();
    // the peer must stay open and quiet through the read window, and the
    // expected-failure path panics past this frame: park both in ManuallyDrop
    // (leaked on panic, discarded on the happy path — NTC-5)
    let served = ManuallyDrop::new(served);
    let listener = ManuallyDrop::new(listener);
    let (zero, elapsed, client) = bounded("zero-len read on a quiet socket", 8, move || {
        let started = Instant::now();
        let mut client = client;
        let r = client.read(&mut []);
        (r, started.elapsed(), client)
    });
    discard(client);
    discard(ManuallyDrop::into_inner(served));
    discard(ManuallyDrop::into_inner(listener));
    assert_eq!(check!(zero), 0, "read(&mut []) on a quiet socket must return Ok(0)");
    assert!(elapsed < Duration::from_secs(2), "zero-len read took {:?}, want prompt", elapsed);
}

/// write(&[]) returns Ok(0), the peer receives NOTHING, and a following 1-byte
/// marker arrives alone and first.
/// XFAIL: on valid=0 the server falls back to length=data.len() and send_slice injects up to 1530 garbage bytes, services/net/src/std_tcpstream.rs.
pub fn tcp_write_zero_len() {
    let (client, served, listener, _addr) = connected_pair();
    let mut served = ManuallyDrop::new(served);
    let listener = ManuallyDrop::new(listener);
    check!(served.set_nonblocking(true));
    let (zero_write, client) = bounded("zero-length write", 10, move || {
        let mut client = client;
        let r = client.write(&[]);
        (r, client)
    });
    // (b) watch window: nothing may arrive (nonblocking reads, safe inline)
    let mut buf = [0u8; 2048];
    let mut leaked: Option<String> = None;
    for i in 0..10u32 {
        match served.read(&mut buf) {
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                if (i + 1) % 5 == 0 {
                    log::info!("zero-write watch {}: peer still quiet", i + 1);
                }
                thread::sleep(Duration::from_millis(100));
            }
            Ok(n) => {
                leaked = Some(format!("Ok({}), first bytes {:02x?}", n, &buf[..n.min(8)]));
                break;
            }
            Err(e) => {
                leaked = Some(format!("Err({:?}: {})", e.kind(), e));
                break;
            }
        }
    }
    // (c) the marker must be the FIRST thing the peer ever receives
    let (marker_write, client) = bounded("marker write", 10, move || {
        let mut client = client;
        let r = client.write_all(b"M");
        (r, client)
    });
    let mut marker: Option<Result<(usize, u8), String>> = None;
    for i in 0..30u32 {
        match served.read(&mut buf) {
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                if (i + 1) % 5 == 0 {
                    log::info!("marker poll {}: still WouldBlock", i + 1);
                }
                thread::sleep(Duration::from_millis(100));
            }
            Ok(n) => {
                marker = Some(Ok((n, if n > 0 { buf[0] } else { 0 })));
                break;
            }
            Err(e) => {
                marker = Some(Err(format!("{:?}: {}", e.kind(), e)));
                break;
            }
        }
    }
    discard(client);
    discard(ManuallyDrop::into_inner(served));
    discard(ManuallyDrop::into_inner(listener));
    assert_eq!(check!(zero_write), 0, "write(&[]) must return Ok(0)");
    if let Some(l) = leaked {
        panic!("peer received data after a zero-length write: {} (NTC-7 garbage injection)", l);
    }
    check!(marker_write);
    match marker {
        Some(Ok((1, b'M'))) => {}
        Some(Ok((n, first))) => panic!(
            "peer read {} bytes with first byte 0x{:02x} after the marker write, want exactly \
             the 1-byte marker 0x4d (garbage prepended? NTC-7)",
            n, first
        ),
        Some(Err(e)) => panic!("marker readback failed: {}", e),
        None => panic!("the marker byte never arrived within 30 nonblocking polls"),
    }
}

/// try_clone interop: a write through the clone is echoed back through the
/// original, and dropping the clone does NOT close the fd (refcounted via
/// handle_count; only the last drop closes) — proven by a second roundtrip.
pub fn tcp_clone_smoke() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    // echo thread + listener leak by design (EOF teardown is NTC-3's business)
    let _server = echo_server(listener);
    let client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let clone = check!(client.try_clone());
    let mut p1 = vec![0u8; 256];
    XorShift::new(0xC10E_0001).fill(&mut p1);
    let p1_send = p1.clone();
    let (w1, clone) = bounded("write via clone", 10, move || {
        let mut clone = clone;
        let r = clone.write_all(&p1_send);
        (r, clone)
    });
    let (r1, client) = bounded("echo readback via original", 10, move || {
        let mut client = client;
        let mut got = vec![0u8; 256];
        let r = client.read_exact(&mut got).map(|_| got);
        (r, client)
    });
    // a clone drop is a client-side handle_count decrement (no server close
    // while other handles live); discarded off-thread for uniformity, with a
    // sleep to order it before the survival roundtrip
    discard(clone);
    thread::sleep(Duration::from_secs(1));
    let mut p2 = vec![0u8; 256];
    XorShift::new(0xC10E_0002).fill(&mut p2);
    let p2_send = p2.clone();
    let (w2, client) = bounded("write via original after clone drop", 10, move || {
        let mut client = client;
        let r = client.write_all(&p2_send);
        (r, client)
    });
    let (r2, client) = bounded("echo readback after clone drop", 10, move || {
        let mut client = client;
        let mut got = vec![0u8; 256];
        let r = client.read_exact(&mut got).map(|_| got);
        (r, client)
    });
    discard(client);
    check!(w1);
    assert_eq!(check!(r1), p1, "echo payload written via the clone");
    check!(w2);
    assert_eq!(check!(r2), p2, "echo payload after the clone was dropped — fd must survive");
}

/// PROBE: two clones each blocked in read() on their own thread receive one
/// byte apiece when the peer sends two — none lost or duplicated. Both park in
/// tcp_rx_waiting on the same handle; on a hang the parked readers/sockets leak.
pub fn tcp_clone_two_threads_read() {
    let (client, served, listener, _addr) = connected_pair();
    let mut served = ManuallyDrop::new(served);
    let listener = ManuallyDrop::new(listener);
    let c2 = check!(client.try_clone());
    let (tx, rx) = mpsc::channel();
    for (idx, mut sock) in vec![(0u8, client), (1u8, c2)] {
        let tx = tx.clone();
        thread::spawn(move || {
            let mut b = [0u8; 1];
            let r = sock.read_exact(&mut b).map(|_| b[0]);
            tx.send((idx, r, sock)).ok();
        });
    }
    thread::sleep(Duration::from_millis(500)); // let both readers park
    let w1 = served.write_all(b"A");
    thread::sleep(Duration::from_millis(300));
    let w2 = served.write_all(b"B");
    let mut got: Vec<(u8, std::io::Result<u8>)> = Vec::new();
    for k in 0..2 {
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok((idx, r, sock)) => {
                discard(sock);
                got.push((idx, r));
            }
            Err(_) => {
                // leak the parked reader(s) and their clone handles (H-1)
                discard(ManuallyDrop::into_inner(served));
                discard(ManuallyDrop::into_inner(listener));
                panic!("clone reader {} of 2 never completed within 10 s (PROBE T-22)", k + 1);
            }
        }
    }
    discard(ManuallyDrop::into_inner(served));
    discard(ManuallyDrop::into_inner(listener));
    check!(w1);
    check!(w2);
    let mut bytes = Vec::new();
    for (idx, r) in got {
        match r {
            Ok(b) => bytes.push(b),
            Err(e) => panic!("clone reader {} failed with: {}", idx, e),
        }
    }
    bytes.sort_unstable();
    assert_eq!(
        bytes,
        vec![b'A', b'B'],
        "two clone readers must receive exactly the two sent bytes, none lost or duplicated"
    );
}

/// Vectored ops fall back to single-buffer behavior: the xous client reports
/// is_read/write_vectored() == false, so std's default fallbacks service only
/// the first non-empty buffer per call.
pub fn tcp_vectored_fallback() {
    let (client, served, listener, _addr) = connected_pair();
    let (wres, client) = bounded("write_vectored", 10, move || {
        let mut client = client;
        let r = client.write_vectored(&[IoSlice::new(b"he"), IoSlice::new(b"llo")]);
        (r, client)
    });
    let wn = match wres {
        Ok(n) => n,
        Err(e) => {
            discard(client);
            discard(served);
            discard(listener);
            panic!("write_vectored failed with: {}", e);
        }
    };
    let (drained, mut served) = bounded("drain the vectored write", 10, move || {
        let mut served = served;
        let mut got = vec![0u8; wn];
        let r = served.read_exact(&mut got).map(|_| got);
        (r, served)
    });
    let reply = served.write_all(b"xyz");
    let (rres, a, b, client) = bounded("read_vectored", 10, move || {
        let mut client = client;
        let mut a = [0u8; 2];
        let mut b = [0u8; 2];
        let r = {
            let mut bufs = [IoSliceMut::new(&mut a), IoSliceMut::new(&mut b)];
            client.read_vectored(&mut bufs)
        };
        (r, a, b, client)
    });
    discard(client);
    discard(served);
    discard(listener);
    assert!((1..=5).contains(&wn), "write_vectored returned {}, want 1..=5", wn);
    assert_eq!(check!(drained), b"hello"[..wn], "vectored write payload");
    check!(reply);
    let rn = check!(rres);
    assert!(rn >= 1, "read_vectored returned Ok(0) with data pending");
    let mut joined = Vec::new();
    joined.extend_from_slice(&a);
    joined.extend_from_slice(&b);
    assert_eq!(joined[..rn.min(3)], b"xyz"[..rn.min(3)], "vectored read payload");
}

/// flush() returns Ok and Debug formatting of the listener and both streams
/// produces a non-empty string without panicking (xous has its own Debug
/// impls, so only non-emptiness is asserted).
pub fn tcp_flush_and_debug_no_panic() {
    let (mut client, served, listener, _addr) = connected_pair();
    check!(client.write_all(b"f"));
    let flush = client.flush();
    let dbg_listener = format!("{:?}", listener);
    let dbg_client = format!("{:?}", client);
    let dbg_served = format!("{:?}", served);
    let (drain, served) = bounded("drain the flushed byte", 10, move || {
        let mut served = served;
        let mut b = [0u8; 1];
        let r = served.read_exact(&mut b).map(|_| b[0]);
        (r, served)
    });
    discard(client);
    discard(served);
    discard(listener);
    check!(flush);
    assert_eq!(check!(drain), b'f', "flushed byte must reach the peer");
    assert!(
        !dbg_listener.is_empty() && !dbg_client.is_empty() && !dbg_served.is_empty(),
        "Debug output must be non-empty (listener {:?} / client {:?} / served {:?})",
        dbg_listener.len(),
        dbg_client.len(),
        dbg_served.len()
    );
}

/// A writer blocked on full buffers resumes when the peer drains: 8 KiB pushed
/// through ~3060 bytes of tx+rx buffering parks the writer after ~3 KiB, then
/// the peer's drain and window-update traffic re-arm the pump so the tx completes.
pub fn tcp_blocked_write_resumes_on_peer_read() {
    const CHUNK: usize = 1024;
    const CHUNKS: usize = 8;
    const SEED: u32 = 0xB10C_ED01;
    let (client, served, listener, _addr) = connected_pair();
    let listener = ManuallyDrop::new(listener);
    let (wtx, wrx) = mpsc::channel();
    thread::spawn(move || {
        let mut client = client;
        let mut rng = XorShift::new(SEED);
        let mut chunk = vec![0u8; CHUNK];
        let res: Result<(), String> = (|| {
            for i in 0..CHUNKS {
                rng.fill(&mut chunk);
                client.write_all(&chunk).map_err(|e| format!("write chunk {}: {}", i, e))?;
                if (i + 1) % 4 == 0 {
                    log::info!("blocked-writer: {} KiB written", i + 1);
                }
            }
            Ok(())
        })();
        wtx.send((res, client)).ok();
    });
    thread::sleep(Duration::from_millis(1500)); // let the writer fill 1530+1530 and park
    let (read_res, served) = bounded("drain 8 KiB from the blocked writer", 30, move || {
        let mut served = served;
        let mut rng = XorShift::new(SEED);
        let mut expect = vec![0u8; CHUNK];
        let mut got = vec![0u8; CHUNK];
        let res: Result<(), String> = (|| {
            for i in 0..CHUNKS {
                rng.fill(&mut expect);
                served.read_exact(&mut got).map_err(|e| format!("read chunk {}: {}", i, e))?;
                if got != expect {
                    return Err(format!("chunk {} corrupted after the write stall", i));
                }
                if (i + 1) % 4 == 0 {
                    log::info!("blocked-writer drain: {} KiB verified", i + 1);
                }
            }
            Ok(())
        })();
        (res, served)
    });
    let (write_res, client) = match wrx.recv_timeout(Duration::from_secs(20)) {
        Ok((r, c)) => (r, Some(c)),
        Err(_) => {
            (Err("writer never resumed after the peer drained (parked tx not woken?)".to_string()), None)
        }
    };
    discard(served);
    if let Some(c) = client {
        discard(c);
    }
    discard(ManuallyDrop::into_inner(listener));
    check!(read_res);
    check!(write_res);
}

/// This theme's registry (aggregated by tests::all_tests / all_xfails).
pub const TESTS: &[TestEntry] = &[
    ("tcp::tcp_read_eof_after_peer_drop", tcp_read_eof_after_peer_drop as fn()),
    ("tcp::tcp_write_after_peer_drop", tcp_write_after_peer_drop as fn()),
    ("tcp::tcp_partial_read", tcp_partial_read as fn()),
    ("tcp::tcp_large_transfer_echo_64k", tcp_large_transfer_echo_64k as fn()),
    ("tcp::tcp_single_write_is_short", tcp_single_write_is_short as fn()),
    ("tcp::tcp_peek_does_not_consume", tcp_peek_does_not_consume as fn()),
    ("tcp::tcp_peek_shorter_buffer", tcp_peek_shorter_buffer as fn()),
    (
        "tcp::tcp_half_close_server_replies_after_client_fin",
        tcp_half_close_server_replies_after_client_fin as fn(),
    ),
    (
        "tcp::tcp_half_close_server_fin_client_still_writes",
        tcp_half_close_server_fin_client_still_writes as fn(),
    ),
    ("tcp::tcp_double_shutdown_is_ok", tcp_double_shutdown_is_ok as fn()),
    ("tcp::tcp_socket_and_peer_name", tcp_socket_and_peer_name as fn()),
    ("tcp::tcp_listener_port_zero_assigned", tcp_listener_port_zero_assigned as fn()),
    ("tcp::tcp_connect_timeout_duration_max_ok", tcp_connect_timeout_duration_max_ok as fn()),
    ("tcp::tcp_incoming_iterator_serial", tcp_incoming_iterator_serial as fn()),
    ("tcp::tcp_double_bind_same_port", tcp_double_bind_same_port as fn()),
    ("tcp::tcp_fast_rebind_after_close", tcp_fast_rebind_after_close as fn()),
    ("tcp::tcp_connect_to_dropped_listener_refused", tcp_connect_to_dropped_listener_refused as fn()),
    ("tcp::tcp_read_zero_len_buffer_pending", tcp_read_zero_len_buffer_pending as fn()),
    ("tcp::tcp_read_zero_len_buffer_quiet", tcp_read_zero_len_buffer_quiet as fn()),
    ("tcp::tcp_write_zero_len", tcp_write_zero_len as fn()),
    ("tcp::tcp_clone_smoke", tcp_clone_smoke as fn()),
    ("tcp::tcp_clone_two_threads_read", tcp_clone_two_threads_read as fn()),
    ("tcp::tcp_vectored_fallback", tcp_vectored_fallback as fn()),
    ("tcp::tcp_flush_and_debug_no_panic", tcp_flush_and_debug_no_panic as fn()),
    ("tcp::tcp_blocked_write_resumes_on_peer_read", tcp_blocked_write_resumes_on_peer_read as fn()),
];

pub const XFAILS: &[XfailEntry] = &[
    ("tcp::tcp_read_eof_after_peer_drop", "NTC-3"),
    ("tcp::tcp_write_after_peer_drop", "NTC-1"),
    ("tcp::tcp_half_close_server_replies_after_client_fin", "NTC-3"),
    ("tcp::tcp_half_close_server_fin_client_still_writes", "NTC-3"),
    ("tcp::tcp_connect_timeout_duration_max_ok", "NTC-13"),
    ("tcp::tcp_read_zero_len_buffer_quiet", "NTC-14"),
    ("tcp::tcp_write_zero_len", "NTC-7"),
];
