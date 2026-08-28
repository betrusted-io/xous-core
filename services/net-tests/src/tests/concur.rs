//! concur theme — cross-thread and multi-socket concurrency semantics: the
//! suite's highest hang/leak-risk theme, every test coordinating at least two
//! threads around blocking socket ops. Discipline on top of tests/mod.rs:
//! cross-thread rendezvous go through mpsc recv_timeout (no unbounded
//! join/recv); blocking ops run inside harness::bounded; TCP sockets travel
//! back to the test thread in collect-discard-assert order and ride in
//! ManuallyDrop when they must survive a panic. The disabled DANGER reproducer
//! below (net-service panic) must NEVER be registered in TESTS.

use core::mem::ManuallyDrop;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::harness::{XorShift, bounded, check, discard, echo_server, next_port, self_ip};
use crate::tests::{TestEntry, XfailEntry};

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// One connect/accept/tagged-byte echo round against `addr`, accepting via
/// the listener stowed in `listener` (restored before any panic-capable
/// assert so a failing round leaks it rather than unwind-dropping it).
fn echo_round(listener: &mut ManuallyDrop<Option<TcpListener>>, addr: SocketAddr, tag: u8, what: &str) {
    let mut client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let l = listener.take().unwrap();
    let (accepted, l) = bounded("accept", 10, move || {
        let r = l.accept();
        (r, l)
    });
    **listener = Some(l);
    let (served, _peer) = check!(accepted);
    check!(client.write_all(&[tag]));
    let (server_read, served) = bounded("server read+reply", 10, move || {
        let mut served = served;
        let mut buf = [0u8; 1];
        let r = served.read_exact(&mut buf).map(|_| buf[0]);
        let r = r.and_then(|got| served.write_all(&[got ^ 0xff]).map(|_| got));
        (r, served)
    });
    let (client_read, client) = bounded("client readback", 10, move || {
        let mut client = client;
        let mut buf = [0u8; 1];
        let r = client.read_exact(&mut buf).map(|_| buf[0]);
        (r, client)
    });
    discard(served);
    discard(client);
    assert_eq!(check!(server_read), tag, "{}: server read the wrong tag byte", what);
    assert_eq!(check!(client_read), tag ^ 0xff, "{}: client read the wrong reply byte", what);
}

/// Two independent TCP connections make interleaved progress: every round
/// writes a chunk on both before reading either echo back, so both always
/// have data in flight, exercising NetPump rx fairness. Echo threads leak.
pub fn two_pairs_interleaved_echo() {
    const ROUNDS: u32 = 5;
    const CHUNK: usize = 256;
    let addr_a = SocketAddr::new(LOOPBACK, next_port());
    let addr_b = SocketAddr::new(LOOPBACK, next_port());
    let _srv_a = echo_server(check!(TcpListener::bind(addr_a)));
    let _srv_b = echo_server(check!(TcpListener::bind(addr_b)));
    let mut conn_a =
        ManuallyDrop::new(Some(check!(bounded("connect A", 10, move || TcpStream::connect(addr_a)))));
    let mut conn_b =
        ManuallyDrop::new(Some(check!(bounded("connect B", 10, move || TcpStream::connect(addr_b)))));
    let mut gen_a = XorShift::new(0xC1A0_0001);
    let mut gen_b = XorShift::new(0xC1B0_0002);
    for round in 0..ROUNDS {
        log::info!("interleave round {}/{}", round, ROUNDS);
        let mut chunk_a = vec![0u8; CHUNK];
        gen_a.fill(&mut chunk_a);
        let mut chunk_b = vec![0u8; CHUNK];
        gen_b.fill(&mut chunk_b);
        // both writes land before either echo is read back, so the two rx
        // completions must interleave across the connections
        check!(conn_a.as_mut().unwrap().write_all(&chunk_a));
        check!(conn_b.as_mut().unwrap().write_all(&chunk_b));
        let sa = conn_a.take().unwrap();
        let (echo_a, sa) = bounded("echo readback A", 10, move || {
            let mut sa = sa;
            let mut buf = vec![0u8; CHUNK];
            let r = sa.read_exact(&mut buf).map(|_| buf);
            (r, sa)
        });
        *conn_a = Some(sa);
        let sb = conn_b.take().unwrap();
        let (echo_b, sb) = bounded("echo readback B", 10, move || {
            let mut sb = sb;
            let mut buf = vec![0u8; CHUNK];
            let r = sb.read_exact(&mut buf).map(|_| buf);
            (r, sb)
        });
        *conn_b = Some(sb);
        // both clients are safely re-stowed (ManuallyDrop) before asserting
        assert_eq!(check!(echo_a), chunk_a, "connection A echo corrupted in round {}", round);
        assert_eq!(check!(echo_b), chunk_b, "connection B echo corrupted in round {}", round);
    }
    discard(conn_a.take().unwrap());
    discard(conn_b.take().unwrap());
}

/// smoltcp has no listen backlog: conn#1's handshake completes against the
/// single listener socket before any accept(), conn#2's SYN then draws an RST
/// (surfacing as AddrNotAvailable), and the replenished listener admits conn#3.
pub fn second_connect_while_unaccepted() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let mut listener = ManuallyDrop::new(Some(check!(TcpListener::bind(addr))));
    // conn#1: completes its handshake against the listener's single smoltcp
    // socket before any accept() is issued
    let mut conn1 = ManuallyDrop::new(Some(check!(bounded("connect #1 (pre-accept)", 10, move || {
        TcpStream::connect(addr)
    }))));
    // conn#2 while conn#1 sits un-accepted: nothing is listening on the port
    // any more, so the SYN draws an RST
    let started = Instant::now();
    let second = bounded("connect #2 (expect no-backlog RST)", 10, move || {
        TcpStream::connect_timeout(&addr, Duration::from_secs(5))
    });
    log::info!("second connect resolved in {:?}", started.elapsed());
    // strip any unexpected socket out of the result immediately so no later
    // panic can unwind-drop it on the test thread (NTC-5)
    let second = match second {
        Ok(s) => {
            discard(s);
            None
        }
        Err(e) => Some(e),
    };
    // accept() must now return conn#1 (already established) and, on its way
    // out, replenish the listener via the client-side re-bind
    let l = listener.take().unwrap();
    let (accepted1, l) = bounded("accept #1", 10, move || {
        let r = l.accept();
        (r, l)
    });
    *listener = Some(l);
    let (served1, _peer) = check!(accepted1);
    check!(conn1.as_mut().unwrap().write_all(&[0xA1]));
    let (read1, served1) = bounded("read on accepted #1", 10, move || {
        let mut s = served1;
        let mut buf = [0u8; 1];
        let r = s.read_exact(&mut buf).map(|_| buf[0]);
        (r, s)
    });
    let mut served1 = ManuallyDrop::new(Some(served1));
    // ...and the replenished listener must admit conn#3
    let mut conn3 = ManuallyDrop::new(Some(check!(bounded("connect #3 (post-replenish)", 10, move || {
        TcpStream::connect(addr)
    }))));
    let l = listener.take().unwrap();
    let (accepted3, l) = bounded("accept #3", 10, move || {
        let r = l.accept();
        (r, l)
    });
    *listener = Some(l);
    let (served3, _peer) = check!(accepted3);
    check!(conn3.as_mut().unwrap().write_all(&[0xA3]));
    let (read3, served3) = bounded("read on accepted #3", 10, move || {
        let mut s = served3;
        let mut buf = [0u8; 1];
        let r = s.read_exact(&mut buf).map(|_| buf[0]);
        (r, s)
    });
    // collect-discard-assert
    discard(served3);
    discard(served1.take().unwrap());
    discard(conn1.take().unwrap());
    discard(conn3.take().unwrap());
    discard(listener.take().unwrap());
    match second {
        None => panic!(
            "connect #2 while conn#1 sat un-accepted unexpectedly succeeded (smoltcp has no backlog; want an RST)"
        ),
        Some(e) => assert_eq!(
            e.kind(),
            ErrorKind::AddrNotAvailable,
            "pre-replenish connect surfaced as {:?} ({}), want the NTC-6-pinned AddrNotAvailable",
            e.kind(),
            e
        ),
    }
    assert_eq!(check!(read1), 0xA1, "conn#1 byte after the late accept");
    assert_eq!(check!(read3), 0xA3, "conn#3 byte after the listener replenish");
}

/// Ten serial connect/accept/exchange/close cycles against one listener all
/// succeed, hammering the accept-side listener replenish interleaved with
/// close churn while the next round's traffic keeps the pump running.
pub fn accept_loop_sequential_stress() {
    const ROUNDS: u8 = 10;
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let mut listener = ManuallyDrop::new(Some(check!(TcpListener::bind(addr))));
    for round in 0..ROUNDS {
        log::info!("stress round {}/{}", round, ROUNDS);
        echo_round(&mut listener, addr, round, "accept-loop stress");
    }
    discard(listener.take().unwrap());
}

/// TCP is full-duplex: a read parked on one thread must not serialize against
/// a concurrent write from another on the same socket. The reader clone parks
/// in rx first, the writer pushes the other way, then the server replies.
pub fn full_duplex_one_socket() {
    const N: usize = 512; // fits the 1530 B socket buffers: neither side can stall on a full window
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    let mut client =
        ManuallyDrop::new(Some(check!(bounded("connect", 10, move || { TcpStream::connect(addr) }))));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let mut listener = ManuallyDrop::new(Some(listener));
    let (served, _peer) = check!(accepted);
    let mut served = ManuallyDrop::new(Some(served));
    let read_half = check!(client.as_ref().unwrap().try_clone());
    let mut c2s = vec![0u8; N];
    XorShift::new(0xC4_0001).fill(&mut c2s);
    let mut s2c = vec![0u8; N];
    XorShift::new(0xC4_0002).fill(&mut s2c);
    // reader parks on the clone; nothing is inbound yet, so it stays parked
    // while the writer half works
    let (rd_tx, rd_rx) = mpsc::channel();
    {
        let mut sock = read_half;
        thread::spawn(move || {
            let mut buf = vec![0u8; N];
            let r = sock.read_exact(&mut buf).map(|_| buf);
            rd_tx.send((r, sock)).ok();
        });
    }
    // writer pushes client->server through the original object concurrently
    let (wr_tx, wr_rx) = mpsc::channel();
    {
        let mut sock = client.take().unwrap();
        let data = c2s.clone();
        thread::spawn(move || {
            let r = sock.write_all(&data);
            wr_tx.send((r, sock)).ok();
        });
    }
    // server side: drain the writer's payload while the reader is still
    // parked, then send the payload the reader is waiting for
    let s = served.take().unwrap();
    let (got_c2s, s) = bounded("server-side read of the writer's payload", 10, move || {
        let mut s = s;
        let mut buf = vec![0u8; N];
        let r = s.read_exact(&mut buf).map(|_| buf);
        (r, s)
    });
    *served = Some(s);
    check!(served.as_mut().unwrap().write_all(&s2c));
    let (got_s2c, read_half) = match rd_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(v) => v,
        Err(_) => panic!("full-duplex reader did not complete within 10 s (worker thread leaked)"),
    };
    let mut read_half = ManuallyDrop::new(Some(read_half));
    let (write_res, client_back) = match wr_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(v) => v,
        Err(_) => panic!("full-duplex writer did not complete within 10 s (worker thread leaked)"),
    };
    // collect-discard-assert
    discard(client_back);
    discard(read_half.take().unwrap());
    discard(served.take().unwrap());
    discard(listener.take().unwrap());
    check!(write_res);
    assert_eq!(check!(got_c2s), c2s, "client->server payload corrupted (writer direction)");
    assert_eq!(check!(got_s2c), s2c, "server->client payload corrupted (reader direction)");
}

/// A TCP listener and a UDP socket bound to the same (addr, port) coexist and
/// demux by protocol, each exercised while the other is live — a cross-talk
/// detector for the shared per-process fd table. UDP targets self_ip().
pub fn tcp_udp_same_port() {
    let ip = self_ip();
    let shared_port = next_port();
    let shared_addr = SocketAddr::new(ip, shared_port);
    let listener = check!(TcpListener::bind(shared_addr)); // whitelist admits the DUT IP (std_tcplistener.rs:38-44)
    let udp = check!(UdpSocket::bind(shared_addr));
    // TCP exchange through the shared port while the UDP socket is bound
    let mut client =
        ManuallyDrop::new(Some(check!(bounded("connect", 10, move || { TcpStream::connect(shared_addr) }))));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let mut listener = ManuallyDrop::new(Some(listener));
    let (served, _peer) = check!(accepted);
    check!(client.as_mut().unwrap().write_all(b"tcp!"));
    let (tcp_read, served) = bounded("read on accepted", 10, move || {
        let mut s = served;
        let mut buf = [0u8; 4];
        let r = s.read_exact(&mut buf).map(|_| buf);
        (r, s)
    });
    let mut served = ManuallyDrop::new(Some(served));
    // UDP exchange to the same port while the TCP connection is still open
    let sender_addr = SocketAddr::new(ip, next_port());
    let sender = check!(UdpSocket::bind(sender_addr));
    assert_eq!(check!(sender.send_to(b"udp?", shared_addr)), 4, "udp send_to length");
    let (udp_read, buf, udp) = bounded("udp recv on the shared port", 10, move || {
        let mut buf = [0u8; 16];
        let r = udp.recv_from(&mut buf);
        (r, buf, udp)
    });
    // collect-discard-assert (UDP drops are synchronous and safe inline)
    drop(udp);
    drop(sender);
    discard(client.take().unwrap());
    discard(served.take().unwrap());
    discard(listener.take().unwrap());
    assert_eq!(&check!(tcp_read), b"tcp!", "tcp payload wrong through the shared port");
    let (n, from) = check!(udp_read);
    assert_eq!(&buf[..n], b"udp?", "udp payload wrong through the shared port");
    assert_eq!(from, sender_addr, "udp datagram source address");
}

/// 60 open/close cycles (a TCP listener + a UDP pair each) never exhaust fds,
/// and every cycle's fresh sockets work (distinct payloads catch aliasing).
/// Each cycle's UDP roundtrip pumps the prior closes so they retire.
pub fn socket_slot_reuse() {
    const CYCLES: u32 = 60;
    let ip = self_ip();
    for cycle in 0..CYCLES {
        if cycle % 5 == 0 {
            log::info!("slot-reuse cycle {}/{}", cycle, CYCLES);
        }
        let mut listener =
            ManuallyDrop::new(Some(check!(TcpListener::bind(SocketAddr::new(LOOPBACK, next_port())))));
        let rx_addr = SocketAddr::new(ip, next_port());
        let tx_addr = SocketAddr::new(ip, next_port());
        let rx_sock = check!(UdpSocket::bind(rx_addr));
        let tx_sock = check!(UdpSocket::bind(tx_addr));
        let mut payload = [0u8; 8];
        XorShift::new(0xC600_0000 + cycle).fill(&mut payload);
        assert_eq!(check!(tx_sock.send_to(&payload, rx_addr)), 8, "cycle {}: send_to length", cycle);
        let (received, buf, rx_sock) = bounded("slot-reuse udp recv", 10, move || {
            let mut buf = [0u8; 16];
            let r = rx_sock.recv_from(&mut buf);
            (r, buf, rx_sock)
        });
        // collect-discard-assert, per cycle
        discard(listener.take().unwrap());
        drop(rx_sock);
        drop(tx_sock);
        let (n, from) = check!(received);
        assert_eq!(&buf[..n], &payload, "cycle {}: payload corrupted across slot reuse", cycle);
        assert_eq!(from, tx_addr, "cycle {}: datagram source address", cycle);
    }
}

/// DANGER — NEVER register in TESTS; run only by hand on a scratch image,
/// because on dev it panics the net service: closing a listener while another
/// thread's accept() is parked removes a smoltcp handle the parked accept
/// still references (a stale-handle panic), which the CI driver treats as fatal.
#[allow(dead_code)]
pub fn drop_listener_while_accept_parked_danger() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    // bitwise alias: same fd, same (un-incremented) refcount Arc — dropping
    // the alias below is therefore the LAST-handle drop even though the
    // accept thread still owns the original
    let alias = unsafe { core::ptr::read(&listener) };
    let (acc_tx, acc_rx) = mpsc::channel();
    thread::spawn(move || {
        let r = listener.accept();
        acc_tx.send(r).ok();
        // the alias drop consumed the refcount and closed the fd: forget the
        // original instead of double-closing / double-freeing the shared Arc
        core::mem::forget(listener);
    });
    // let the accept park in tcp_accept_waiting (std_tcplistener.rs:150-158)
    thread::sleep(Duration::from_millis(1000));
    // last-handle drop: fires StdTcpClose with the accept still parked. The
    // close of an idle listener needs pump traffic to retire (NTC-5), so a
    // UDP roundtrip below primes the pump — and with it, the panic window.
    discard(alias);
    let probe_addr = SocketAddr::new(self_ip(), next_port());
    let probe = check!(UdpSocket::bind(probe_addr));
    check!(probe.send_to(b"pump", probe_addr));
    let (pumped, _buf, probe) = bounded("pump-priming udp recv", 10, move || {
        let mut buf = [0u8; 8];
        let r = probe.recv_from(&mut buf);
        (r, buf, probe)
    });
    drop(probe);
    check!(pumped);
    // the contract half: the parked accept must now return rather than hang
    match acc_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok((stream, _peer))) => discard(stream),
        Ok(Err(e)) => log::info!("parked accept returned Err({}) after the close — acceptable", e),
        Err(_) => panic!(
            "accept did not return within 10 s of its listener closing (the net service may just have panicked — H-1)"
        ),
    }
}

/// Accept one connection via the original listener, then one via its
/// try_clone: clones share one fd Arc, so round 2 works only if accept()'s
/// listener replenish is visible through both clones.
pub fn listener_clone_accept_serial() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let l1 = check!(TcpListener::bind(addr));
    let l2 = check!(l1.try_clone());
    let mut l1 = ManuallyDrop::new(Some(l1));
    let mut l2 = ManuallyDrop::new(Some(l2));
    log::info!("clone-accept round 1: via the original");
    echo_round(&mut l1, addr, 0x81, "clone-accept round 1 (original)");
    log::info!("clone-accept round 2: via the clone");
    echo_round(&mut l2, addr, 0x82, "clone-accept round 2 (clone)");
    // first drop only decrements the shared refcount; the second closes the fd
    discard(l1.take().unwrap());
    discard(l2.take().unwrap());
}

/// Two threads parked in accept() on two clones of one listener, plus two
/// connections, must yield two distinct working accepted streams.
/// XFAIL: both AcceptingSocket entries reference the same smoltcp handle so both accepts return one connection, the pump accept scan in services/net/src/main.rs.
pub fn listener_clone_accept_concurrent_probe() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let l1 = check!(TcpListener::bind(addr));
    let l2 = check!(l1.try_clone());
    // both acceptors park first, then the connects arrive
    let (a_tx, a_rx) = mpsc::channel();
    {
        let l = l1;
        thread::spawn(move || {
            let r = l.accept();
            a_tx.send((r, l)).ok();
        });
    }
    let (b_tx, b_rx) = mpsc::channel();
    {
        let l = l2;
        thread::spawn(move || {
            let r = l.accept();
            b_tx.send((r, l)).ok();
        });
    }
    thread::sleep(Duration::from_millis(500)); // let both accepts park in tcp_accept_waiting
    let mut conn1 =
        ManuallyDrop::new(Some(check!(bounded("connect #1", 10, move || TcpStream::connect(addr)))));
    // give the winning accept time to return and replenish the listener:
    // pre-replenish, conn#2's SYN would draw the no-backlog RST (see C-2)
    thread::sleep(Duration::from_millis(1000));
    let mut conn2 =
        ManuallyDrop::new(Some(check!(bounded("connect #2", 10, move || TcpStream::connect(addr)))));
    // collect both acceptors (guarded rendezvous)
    let (acc_a, l1) = match a_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(v) => v,
        Err(_) => panic!("PROBE: accept via clone #1 did not return within 10 s of both connects"),
    };
    let mut l1 = ManuallyDrop::new(Some(l1));
    let mut srv_a = match acc_a {
        Ok((s, _peer)) => ManuallyDrop::new(Some(s)),
        Err(e) => panic!("PROBE: accept via clone #1 failed: {}", e),
    };
    let (acc_b, l2) = match b_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(v) => v,
        Err(_) => panic!("PROBE: accept via clone #2 did not return within 10 s of both connects"),
    };
    let mut l2 = ManuallyDrop::new(Some(l2));
    let mut srv_b = match acc_b {
        Ok((s, _peer)) => ManuallyDrop::new(Some(s)),
        Err(e) => panic!("PROBE: accept via clone #2 failed: {}", e),
    };
    // each client tags its connection; each accepted stream must produce
    // exactly one of the two tags (accept order is unspecified)
    check!(conn1.as_mut().unwrap().write_all(&[0xB1]));
    check!(conn2.as_mut().unwrap().write_all(&[0xB2]));
    let s = srv_a.take().unwrap();
    let (got_a, s) = bounded("read on accepted A", 10, move || {
        let mut s = s;
        let mut buf = [0u8; 1];
        let r = s.read_exact(&mut buf).map(|_| buf[0]);
        (r, s)
    });
    *srv_a = Some(s);
    let s = srv_b.take().unwrap();
    let (got_b, s) = bounded("read on accepted B", 10, move || {
        let mut s = s;
        let mut buf = [0u8; 1];
        let r = s.read_exact(&mut buf).map(|_| buf[0]);
        (r, s)
    });
    *srv_b = Some(s);
    // collect-discard-assert (no waiter is parked once both accepts returned)
    discard(conn1.take().unwrap());
    discard(conn2.take().unwrap());
    discard(srv_a.take().unwrap());
    discard(srv_b.take().unwrap());
    discard(l1.take().unwrap());
    discard(l2.take().unwrap());
    let mut got = [check!(got_a), check!(got_b)];
    got.sort_unstable();
    assert_eq!(
        got,
        [0xB1, 0xB2],
        "PROBE: the two concurrent accepts did not serve the two distinct connections"
    );
}

/// Two threads send one datagram each to a single receiver; both arrive with
/// the right payload and source address. Exactly two are in flight, matching
/// the rx queue's 2 slots; arrival order is unspecified, matched by source.
pub fn udp_two_senders_one_receiver() {
    let ip = self_ip();
    let rx_addr = SocketAddr::new(ip, next_port());
    let addr_1 = SocketAddr::new(ip, next_port());
    let addr_2 = SocketAddr::new(ip, next_port());
    let receiver = check!(UdpSocket::bind(rx_addr));
    let sender_1 = check!(UdpSocket::bind(addr_1));
    let sender_2 = check!(UdpSocket::bind(addr_2));
    let mut payload_1 = [0u8; 16];
    XorShift::new(0xC9_0001).fill(&mut payload_1);
    let mut payload_2 = [0u8; 16];
    XorShift::new(0xC9_0002).fill(&mut payload_2);
    let (tx1, rx1) = mpsc::channel();
    thread::spawn(move || {
        let r = sender_1.send_to(&payload_1, rx_addr);
        tx1.send((r, sender_1)).ok();
    });
    let (tx2, rx2) = mpsc::channel();
    thread::spawn(move || {
        let r = sender_2.send_to(&payload_2, rx_addr);
        tx2.send((r, sender_2)).ok();
    });
    let (first, buf_first, receiver) = bounded("recv of the first datagram", 10, move || {
        let mut buf = [0u8; 32];
        let r = receiver.recv_from(&mut buf);
        (r, buf, receiver)
    });
    let (second, buf_second, receiver) = bounded("recv of the second datagram", 10, move || {
        let mut buf = [0u8; 32];
        let r = receiver.recv_from(&mut buf);
        (r, buf, receiver)
    });
    let (send_1, sender_1) = match rx1.recv_timeout(Duration::from_secs(10)) {
        Ok(v) => v,
        Err(_) => panic!("sender #1 did not finish within 10 s (worker thread leaked)"),
    };
    let (send_2, sender_2) = match rx2.recv_timeout(Duration::from_secs(10)) {
        Ok(v) => v,
        Err(_) => panic!("sender #2 did not finish within 10 s (worker thread leaked)"),
    };
    drop(sender_1);
    drop(sender_2);
    drop(receiver);
    assert_eq!(check!(send_1), 16, "sender #1 send_to length");
    assert_eq!(check!(send_2), 16, "sender #2 send_to length");
    let (n_first, from_first) = check!(first);
    let (n_second, from_second) = check!(second);
    assert_ne!(from_first, from_second, "the same sender was received twice");
    for (from, buf, n) in [(from_first, buf_first, n_first), (from_second, buf_second, n_second)] {
        let expected: &[u8] = if from == addr_1 {
            &payload_1
        } else if from == addr_2 {
            &payload_2
        } else {
            panic!("datagram from unexpected source {}", from)
        };
        assert_eq!(&buf[..n], expected, "payload from {} corrupted", from);
    }
}

/// A connected TcpStream moved to another thread works from there: fds index
/// the per-process socket table, so a socket is valid from any thread. Write
/// and read both happen only on the destination thread.
pub fn cross_thread_move() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    let client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    // hand the connected stream to a fresh thread and drive it only there
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut client = client;
        let wrote = client.write_all(b"mova");
        let mut buf = [0u8; 4];
        let read = client.read_exact(&mut buf).map(|_| buf);
        tx.send((wrote, read, client)).ok();
    });
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let mut listener = ManuallyDrop::new(Some(listener));
    let (served, _peer) = check!(accepted);
    let (request, served) = bounded("server read", 10, move || {
        let mut s = served;
        let mut buf = [0u8; 4];
        let r = s.read_exact(&mut buf).map(|_| buf);
        (r, s)
    });
    let mut served = ManuallyDrop::new(Some(served));
    check!(served.as_mut().unwrap().write_all(b"avom"));
    let (wrote, read, client) = match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(v) => v,
        Err(_) => panic!("moved-stream worker did not finish within 10 s (worker thread leaked)"),
    };
    // collect-discard-assert
    discard(client);
    discard(served.take().unwrap());
    discard(listener.take().unwrap());
    check!(wrote);
    assert_eq!(&check!(request), b"mova", "server read the wrong request from the moved stream");
    assert_eq!(&check!(read), b"avom", "moved stream read the wrong reply");
}

/// This theme's registry (aggregated by tests::all_tests / all_xfails). The
/// DANGER reproducer (drop_listener_while_accept_parked_danger) is disabled
/// and must never appear here.
pub const TESTS: &[TestEntry] = &[
    ("concur::two_pairs_interleaved_echo", two_pairs_interleaved_echo as fn()),
    ("concur::second_connect_while_unaccepted", second_connect_while_unaccepted as fn()),
    ("concur::accept_loop_sequential_stress", accept_loop_sequential_stress as fn()),
    ("concur::full_duplex_one_socket", full_duplex_one_socket as fn()),
    ("concur::tcp_udp_same_port", tcp_udp_same_port as fn()),
    ("concur::socket_slot_reuse", socket_slot_reuse as fn()),
    ("concur::listener_clone_accept_serial", listener_clone_accept_serial as fn()),
    ("concur::listener_clone_accept_concurrent_probe", listener_clone_accept_concurrent_probe as fn()),
    ("concur::udp_two_senders_one_receiver", udp_two_senders_one_receiver as fn()),
    ("concur::cross_thread_move", cross_thread_move as fn()),
];

/// Known-bug registry: the concurrent-accept probe is XFAIL(NTC-19) — aliased
/// fds make concurrent accept() on clones of one listener unsupported.
pub const XFAILS: &[XfailEntry] = &[("concur::listener_clone_accept_concurrent_probe", "NTC-19")];
