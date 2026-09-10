//! cross-host theme — cross-host tests against a REAL peer on the emulated switch.
//! These talk to a second Renode machine (emulation/linux-server.resc, a
//! busybox Linux) that the driver (tools/std-net-cross-host-ci.py) provisions
//! before the suite runs; compiled only under the `cross-host` feature. The peer
//! contract (addresses, ports, DNS records) is the consts below and MUST stay
//! in lockstep with the driver. Same harness rules as loopback (tests/mod.rs):
//! DUT-local ports via next_port(), blocking calls via bounded(), TCP via
//! discard(), log long loops; error KINDS pinned to (rustc, xous-core).

use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};

use crate::harness::{bounded, check, discard, next_port, self_ip};
use crate::tests::{TestEntry, XfailEntry};

/// The peer's static eth0 address (linux-server.resc rcS).
const PEER_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1));
/// Peer TCP echo server port (driver: `nc -lk -p <port> -e /bin/cat`).
const PEER_ECHO_TCP: u16 = 6001;
/// Peer UDP echo server port (driver: `nc -u -lk -p <port> -e /bin/cat`).
const PEER_ECHO_UDP: u16 = 6002;
/// Peer bulk-source port: serves 8192 bytes of 'A' then closes (driver).
const PEER_BULK_TCP: u16 = 6003;
/// A peer port with nothing listening — a SYN draws a real RST.
const PEER_DEAD_TCP: u16 = 6009;
/// Static A records the peer's dnsd serves (driver PEER_DNS_RECORDS); the DUT's
/// resolver (dns1, from the peer's udhcpd) points at the peer, so std lookups
/// of these names reach that dnsd.
const PEER_DNS: &[(&str, [u8; 4])] = &[("one.test", [10, 11, 12, 13]), ("two.test", [203, 0, 113, 7])];

/// cross-host canary: asserts the DUT's interface address is in the peer subnet
/// (192.168.0.0/24) and not the loopback static 10.0.2.15, proving a REAL DHCP
/// lease from the peer's udhcpd rather than a static seed.
pub fn dhcp_lease_from_peer() {
    let ip = self_ip();
    let v4 = match ip {
        IpAddr::V4(v4) => v4,
        IpAddr::V6(_) => panic!("expected an IPv4 lease, got {:?}", ip),
    };
    let o = v4.octets();
    assert_eq!(
        (o[0], o[1], o[2]),
        (192, 168, 0),
        "DUT address {} is not in the peer subnet 192.168.0.0/24 — real DHCP did not bind",
        v4
    );
    assert!(o[3] >= 20, "DUT address {} is outside the udhcpd lease range (.20-.254)", v4);
}

/// cross-host cross-host TCP: connect to the peer's echo server, send a payload,
/// read it back. Exercises a real remote stack (Linux) end to end rather than
/// loopback — SYN/SYN-ACK over the emulated wire, the peer's `nc` echoing.
pub fn tcp_echo_to_peer() {
    let addr = SocketAddr::new(PEER_IP, PEER_ECHO_TCP);
    let msg = b"cross-host-echo-probe";
    let got = bounded("cross-host echo roundtrip", 20, move || -> std::io::Result<Vec<u8>> {
        let mut s = TcpStream::connect(addr)?;
        s.write_all(msg)?;
        let mut buf = vec![0u8; msg.len()];
        s.read_exact(&mut buf)?;
        discard(s);
        Ok(buf)
    });
    assert_eq!(&check!(got), msg, "peer echo mismatch");
}

/// cross-host real connect-refused: a SYN to a peer port with no listener draws a
/// genuine RST, yet the client still surfaces AddrNotAvailable on the
/// (rustc, xous-core) pair. Asserts Err with that pinned kind.
pub fn connect_refused_from_peer() {
    let addr = SocketAddr::new(PEER_IP, PEER_DEAD_TCP);
    let result = bounded("connect to a dead peer port", 15, move || TcpStream::connect(addr));
    match result {
        Ok(s) => {
            discard(s);
            panic!("connect to the dead peer port {} unexpectedly succeeded", addr);
        }
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::AddrNotAvailable,
            "peer connect-refused surfaced as {:?} ({}), want the pinned AddrNotAvailable (NTC-6)",
            e.kind(),
            e
        ),
    }
}

/// cross-host cross-host UDP: send a datagram to the peer's UDP echo server and
/// read the echo back. The DUT socket binds its own DHCP address; the datagram
/// crosses the emulated wire to the peer's `nc -u` and returns.
pub fn udp_echo_to_peer() {
    let local = SocketAddr::new(self_ip(), next_port());
    let peer = SocketAddr::new(PEER_IP, PEER_ECHO_UDP);
    let msg = b"cross-host-udp-probe";
    let got = bounded("cross-host udp roundtrip", 25, move || -> std::io::Result<Vec<u8>> {
        let sock = UdpSocket::bind(local)?;
        // UDP is lossy and busybox `nc -u` echo is finicky, so burst several
        // datagrams and block for the first reply. A read timeout can't retry
        // (quiet-socket recv timeouts don't fire on dev, #880), so bounded() guards.
        for _ in 0..5 {
            sock.send_to(msg, peer)?;
        }
        let mut buf = vec![0u8; 64];
        let (n, from) = sock.recv_from(&mut buf)?;
        buf.truncate(n);
        drop(sock);
        Ok(if from.ip() == PEER_IP { buf } else { Vec::new() })
    });
    assert_eq!(&check!(got), msg, "peer udp echo mismatch (or wrong source addr)");
}

/// cross-host real DNS: resolve each `PEER_DNS` name against the peer's busybox
/// dnsd through the full std path (`ToSocketAddrs`), decoding a genuine
/// A-record response over the wire, and assert it maps to its configured address.
pub fn dns_resolve_peer_records() {
    for (i, &(name, ip)) in PEER_DNS.iter().enumerate() {
        let want = SocketAddr::new(IpAddr::V4(Ipv4Addr::from(ip)), 80);
        let hostport = format!("{}:80", name);
        // A quiet/parked resolver socket could hang the lookup (net #880), so
        // resolve off-thread with a deadline.
        let addrs =
            bounded("dns lookup", 20, move || hostport.to_socket_addrs().map(|it| it.collect::<Vec<_>>()));
        let addrs = check!(addrs);
        assert!(
            addrs.contains(&want),
            "lookup of {} resolved to {:?}, want {} (record {})",
            name,
            addrs,
            want,
            i
        );
    }
}

/// cross-host cross-host TCP windowing (receive side): the peer serves BULK_LEN
/// bytes of a known fill and the DUT reads to EOF. Crossing the 1530-byte rx
/// buffer ~5x exercises real over-the-wire segmentation and window updates.
pub fn tcp_bulk_receive_from_peer() {
    const BULK_LEN: usize = 8192;
    const FILL: u8 = b'A'; // driver serves BULK_LEN bytes of this (PEER_BULK_TCP)
    let addr = SocketAddr::new(PEER_IP, PEER_BULK_TCP);
    let got = bounded("cross-host bulk receive", 30, move || -> std::io::Result<Vec<u8>> {
        let mut stream = TcpStream::connect(addr)?;
        let mut buf = vec![0u8; 2048];
        let mut got = Vec::with_capacity(BULK_LEN);
        loop {
            let n = stream.read(&mut buf)?;
            if n == 0 {
                break;
            }
            got.extend_from_slice(&buf[..n]);
            if got.len() % 2048 < n {
                log::info!("cross-host bulk: {} bytes", got.len());
            }
            if got.len() > BULK_LEN * 2 {
                break; // guard against a misbehaving server streaming forever
            }
        }
        discard(stream);
        Ok(got)
    });
    let got = check!(got);
    assert_eq!(got.len(), BULK_LEN, "bulk receive got {} bytes, want {}", got.len(), BULK_LEN);
    assert!(got.iter().all(|&b| b == FILL), "bulk stream content corrupted over the wire");
}

/// cross-host real NXDOMAIN: a name the peer's dnsd does not serve must fail to
/// resolve. std maps the failed lookup to InvalidInput "DNS failure" (or an
/// empty address set); an unresolved name reading back as success fails here.
pub fn dns_unknown_name_errors() {
    let hostport = "no-such-host.test:80".to_string();
    let result = bounded("dns lookup of an unknown name", 20, move || {
        hostport.to_socket_addrs().map(|it| it.collect::<Vec<_>>())
    });
    match result {
        Ok(addrs) if addrs.is_empty() => {} // an empty success is also "no address"
        Ok(addrs) => panic!("unknown name resolved to {:?}, want a lookup failure", addrs),
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::InvalidInput,
            "unknown-name lookup failed with {:?} ({}), want InvalidInput",
            e.kind(),
            e
        ),
    }
}

pub const TESTS: &[TestEntry] = &[
    ("cross_host::dhcp_lease_from_peer", dhcp_lease_from_peer as fn()),
    ("cross_host::tcp_echo_to_peer", tcp_echo_to_peer as fn()),
    ("cross_host::tcp_bulk_receive_from_peer", tcp_bulk_receive_from_peer as fn()),
    ("cross_host::connect_refused_from_peer", connect_refused_from_peer as fn()),
    ("cross_host::udp_echo_to_peer", udp_echo_to_peer as fn()),
    ("cross_host::dns_resolve_peer_records", dns_resolve_peer_records as fn()),
    ("cross_host::dns_unknown_name_errors", dns_unknown_name_errors as fn()),
];

pub const XFAILS: &[XfailEntry] = &[];
