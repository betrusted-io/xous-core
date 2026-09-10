//! socket-option (SO_*/IP_*-level) surface — ttl, nodelay, only_v6,
//! broadcast/multicast, the nonblocking flag, and IPv6 rejection. All
//! assertions state the correct/documented behavior; see tests/mod.rs for the
//! shared discipline (port isolation, hang/close hazards, collect-discard-
//! assert). No test here is XFAIL-registered: each is an as-is PASS or a
//! deviation-pin (asserting documented xous behavior), plus one PROBE
//! (ipv6_surface_rejected) whose UDP-v6 sub-case has no allocated bug id.

use std::io::{self, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};

use crate::harness::{bounded, check, discard, next_port, self_ip};

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// set_ttl(100) then ttl()==100 round-trips on a live TcpStream, TcpListener,
/// and UdpSocket alike — the server dispatches all three to one StdSetTtl/
/// StdGetTtl opcode pair, so this also covers fd/type dispatch.
pub fn ttl_roundtrip_stream_listener_udp() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    let client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let (served, _peer) = check!(accepted);

    let client_set = client.set_ttl(100);
    let client_get = client.ttl();
    let listener_set = listener.set_ttl(100);
    let listener_get = listener.ttl();

    discard(client);
    discard(served);
    discard(listener);

    check!(client_set);
    assert_eq!(check!(client_get), 100, "TcpStream ttl round-trip");
    check!(listener_set);
    assert_eq!(check!(listener_get), 100, "TcpListener ttl round-trip");

    // Independent UDP leg — synchronous drop, no hazard, no discard() needed.
    let udp_port = next_port();
    let udp = check!(UdpSocket::bind(SocketAddr::new(self_ip(), udp_port)));
    check!(udp.set_ttl(100));
    assert_eq!(check!(udp.ttl()), 100, "UdpSocket ttl round-trip");
}

/// Deviation-pin: unlike Linux (EINVAL), set_ttl(0) succeeds and ttl() reads
/// back 64 — the server maps ttl==0 to smoltcp's default hop limit rather than
/// forwarding a literal 0 (which would trip smoltcp's panic-on-zero).
pub fn ttl_zero_reads_back_default() {
    let port = next_port();
    let udp = check!(UdpSocket::bind(SocketAddr::new(self_ip(), port)));
    check!(udp.set_ttl(0));
    assert_eq!(check!(udp.ttl()), 64, "ttl(0) must read back as the smoltcp default (64), not 0");
}

/// set_ttl(256) fails InvalidInput before reaching the server — TTL is a
/// single wire byte, so out-of-range values are rejected client-side, same
/// as every other std::net platform backend.
pub fn ttl_gt_255_invalid_input() {
    let port = next_port();
    let udp = check!(UdpSocket::bind(SocketAddr::new(self_ip(), port)));
    match udp.set_ttl(256) {
        Ok(()) => panic!("set_ttl(256) unexpectedly succeeded"),
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::InvalidInput,
            "set_ttl(256) surfaced as {:?} ({}), want InvalidInput",
            e.kind(),
            e
        ),
    }
}

/// nodelay() defaults to false (Nagle enabled); set_nodelay(true)/false
/// round-trips.
pub fn nodelay_default_and_roundtrip() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    let client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let (served, _peer) = check!(accepted);

    let default = client.nodelay();
    let set_true = client.set_nodelay(true);
    let after_true = client.nodelay();
    let set_false = client.set_nodelay(false);
    let after_false = client.nodelay();

    discard(client);
    discard(served);
    discard(listener);

    assert_eq!(check!(default), false, "nodelay default should be false (Nagle enabled by default)");
    check!(set_true);
    assert_eq!(check!(after_true), true, "nodelay after set_nodelay(true)");
    check!(set_false);
    assert_eq!(check!(after_false), false, "nodelay after set_nodelay(false)");
}

/// UDP broadcast/multicast has no smoltcp backing and no server opcode: all
/// 12 methods in this surface fail Unsupported client-side, without ever
/// reaching the server.
pub fn udp_broadcast_multicast_unsupported() {
    let port = next_port();
    let udp = check!(UdpSocket::bind(SocketAddr::new(self_ip(), port)));
    let v4_group = Ipv4Addr::new(224, 0, 0, 1);
    let v4_iface = Ipv4Addr::UNSPECIFIED;
    let v6_group = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1);

    let checks: Vec<(&str, io::Result<()>)> = vec![
        ("set_broadcast", udp.set_broadcast(true)),
        ("broadcast", udp.broadcast().map(|_| ())),
        ("join_multicast_v4", udp.join_multicast_v4(&v4_group, &v4_iface)),
        ("leave_multicast_v4", udp.leave_multicast_v4(&v4_group, &v4_iface)),
        ("join_multicast_v6", udp.join_multicast_v6(&v6_group, 0)),
        ("leave_multicast_v6", udp.leave_multicast_v6(&v6_group, 0)),
        ("multicast_loop_v4", udp.multicast_loop_v4().map(|_| ())),
        ("set_multicast_loop_v4", udp.set_multicast_loop_v4(true)),
        ("multicast_loop_v6", udp.multicast_loop_v6().map(|_| ())),
        ("set_multicast_loop_v6", udp.set_multicast_loop_v6(true)),
        ("multicast_ttl_v4", udp.multicast_ttl_v4().map(|_| ())),
        ("set_multicast_ttl_v4", udp.set_multicast_ttl_v4(2)),
    ];
    for (i, (name, result)) in checks.into_iter().enumerate() {
        if (i + 1) % 5 == 0 {
            // inactivity-reaper rule: loops of >10 ops must emit output
            log::info!("udp_broadcast_multicast_unsupported: checked {} of 12", i + 1);
        }
        match result {
            Ok(()) => panic!("{} unexpectedly succeeded on xous (no smoltcp backing)", name),
            Err(e) => assert_eq!(
                e.kind(),
                ErrorKind::Unsupported,
                "{} surfaced as {:?} ({}), want Unsupported",
                name,
                e.kind(),
                e
            ),
        }
    }
}

/// No IPv6 stack on xous, so TcpListener::only_v6/set_only_v6 are stubbed to
/// fail Unsupported client-side rather than reach a nonexistent server opcode.
#[allow(deprecated)]
pub fn listener_only_v6_unsupported() {
    let port = next_port();
    let listener = check!(TcpListener::bind(SocketAddr::new(LOOPBACK, port)));

    let set_result = listener.set_only_v6(true);
    let get_result = listener.only_v6();

    discard(listener);

    match set_result {
        Ok(()) => panic!("set_only_v6 unexpectedly succeeded on xous (no IPv6 stack at all)"),
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::Unsupported,
            "set_only_v6 surfaced as {:?} ({}), want Unsupported",
            e.kind(),
            e
        ),
    }
    match get_result {
        Ok(v) => panic!("only_v6 unexpectedly succeeded on xous, returned {}", v),
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::Unsupported,
            "only_v6 surfaced as {:?} ({}), want Unsupported",
            e.kind(),
            e
        ),
    }
}

/// set_nonblocking(true) then (false) returns Ok on every socket kind — a
/// client-side flag only; the behavioral WouldBlock checks live in errors.
pub fn set_nonblocking_toggle_all_types() {
    let port = next_port();
    let addr = SocketAddr::new(LOOPBACK, port);
    let listener = check!(TcpListener::bind(addr));
    let client = check!(bounded("connect", 10, move || TcpStream::connect(addr)));
    let (accepted, listener) = bounded("accept", 10, move || {
        let r = listener.accept();
        (r, listener)
    });
    let (served, _peer) = check!(accepted);

    let listener_on = listener.set_nonblocking(true);
    let listener_off = listener.set_nonblocking(false);
    let client_on = client.set_nonblocking(true);
    let client_off = client.set_nonblocking(false);
    let served_on = served.set_nonblocking(true);
    let served_off = served.set_nonblocking(false);

    discard(client);
    discard(served);
    discard(listener);

    check!(listener_on);
    check!(listener_off);
    check!(client_on);
    check!(client_off);
    check!(served_on);
    check!(served_off);

    // Independent UDP leg — synchronous drop, no hazard, no discard() needed.
    let udp_port = next_port();
    let udp = check!(UdpSocket::bind(SocketAddr::new(self_ip(), udp_port)));
    check!(udp.set_nonblocking(true));
    check!(udp.set_nonblocking(false));
}

/// xous is IPv4-only, so only TcpListener::bind(v6) is asserted live: the
/// listen whitelist rejects it before any smoltcp socket exists, surfacing as
/// AddrNotAvailable. The connect/bind v6 cases panic the service (disabled below).
pub fn ipv6_surface_rejected() {
    let v6_listen_port = next_port();
    match TcpListener::bind(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), v6_listen_port)) {
        Ok(l) => {
            discard(l);
            panic!("TcpListener::bind on a v6 address unexpectedly succeeded");
        }
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::AddrNotAvailable,
            "v6 listen bind surfaced as {:?} ({}), want AddrNotAvailable",
            e.kind(),
            e
        ),
    }
}

/// DANGER — disabled, NOT registered: TcpStream::connect to an IPv6 address
/// panics the net service. std_tcp_connect applies no v4 whitelist, so the v6
/// remote reaches smoltcp and the next poll unwraps None during source
/// selection. Fix: a v4-only check mirroring the listen whitelist.
#[allow(dead_code)]
pub fn tcp_connect_v6_panics_net_service() {
    let v6_addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), next_port());
    match bounded("connect to a v6 address", 10, move || TcpStream::connect(v6_addr)) {
        Ok(s) => {
            discard(s);
            panic!(
                "TcpStream::connect(v6) returned Ok — the net service panics on the following poll (NTC-16)"
            );
        }
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::AddrNotAvailable,
            "v6 connect surfaced as {:?} ({}), want AddrNotAvailable",
            e.kind(),
            e
        ),
    }
}

/// DANGER — disabled, NOT registered: UdpSocket::bind on an IPv6 address
/// panics the net service the same way — std_udp_bind has no whitelist, so the
/// v6 endpoint reaches smoltcp and the next poll unwraps None. Fix: a v4-only
/// check in std_udp_bind.
#[allow(dead_code)]
pub fn udp_bind_v6_panics_net_service() {
    let v6_udp_addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), next_port());
    match UdpSocket::bind(v6_udp_addr) {
        Ok(sock) => {
            drop(sock);
            panic!("UdpSocket::bind(v6) returned Ok — the net service panics on the following poll (NTC-16)");
        }
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::AddrNotAvailable,
            "v6 UDP bind surfaced as {:?} ({}), want AddrNotAvailable",
            e.kind(),
            e
        ),
    }
}

/// This theme's registry (aggregated by tests::all_tests / all_xfails).
pub const TESTS: &[(&str, fn())] = &[
    ("sockopts::ttl_roundtrip_stream_listener_udp", ttl_roundtrip_stream_listener_udp as fn()),
    ("sockopts::ttl_zero_reads_back_default", ttl_zero_reads_back_default as fn()),
    ("sockopts::ttl_gt_255_invalid_input", ttl_gt_255_invalid_input as fn()),
    ("sockopts::nodelay_default_and_roundtrip", nodelay_default_and_roundtrip as fn()),
    ("sockopts::udp_broadcast_multicast_unsupported", udp_broadcast_multicast_unsupported as fn()),
    ("sockopts::listener_only_v6_unsupported", listener_only_v6_unsupported as fn()),
    ("sockopts::set_nonblocking_toggle_all_types", set_nonblocking_toggle_all_types as fn()),
    ("sockopts::ipv6_surface_rejected", ipv6_surface_rejected as fn()),
];

/// No XFAILs in this theme: every registered test is an as-is PASS or a
/// deviation-pin. The ipv6_surface_rejected UDP-v6 sub-case is an unregistered
/// PROBE, not an XFAIL entry.
pub const XFAILS: &[(&str, &str)] = &[];
