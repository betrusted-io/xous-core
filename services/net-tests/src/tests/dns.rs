//! dns theme: the resolver/decoder chain behind `ToSocketAddrs`, exercised
//! hermetically with a fake DNS resolver bound inside the DUT. Lookups run the
//! full std path (to_socket_addrs -> LookupHost -> dns RawLookup IPC ->
//! resolver UDP query). Port 53 is the one deliberately REUSED port (the
//! resolver always queries <dns1>:53); each test binds it to self_ip on the
//! test thread before the lookup and drops it inline before asserting, always
//! answers its one query (an unanswered query wedges the single-threaded dns
//! loop, #880), and uses a unique hostname (lookups cache forever).

use std::io::{self, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::harness::{bounded, check, discard, next_port, self_ip};
use crate::tests::{TestEntry, XfailEntry};

// ---------------------------------------------------------------------------
// DNS wire-format response builders.
// ---------------------------------------------------------------------------

/// Build a wire-format DNS response header.
/// id: 2 bytes; flags: 0x8180 (QR=1, RD=1, RA=1, RCODE=0);
/// qdcount=1, `ancount`, nscount=0, arcount=0.
fn header(id: u16, ancount: u16) -> Vec<u8> { header_with_flags(id, 0x8180, ancount) }

/// `header` with caller-chosen flags, for rcode != 0 shapes (e.g. NXDOMAIN).
fn header_with_flags(id: u16, flags: u16, ancount: u16) -> Vec<u8> {
    let mut h = Vec::new();
    h.extend_from_slice(&id.to_be_bytes());
    h.extend_from_slice(&flags.to_be_bytes());
    h.extend_from_slice(&1u16.to_be_bytes()); // qdcount
    h.extend_from_slice(&ancount.to_be_bytes());
    h.extend_from_slice(&0u16.to_be_bytes()); // nscount
    h.extend_from_slice(&0u16.to_be_bytes()); // arcount
    h
}

/// Encode a domain name as DNS wire labels terminated by a null byte.
fn encode_name(name: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    for label in name.split('.') {
        buf.push(label.len() as u8);
        buf.extend_from_slice(label.as_bytes());
    }
    buf.push(0);
    buf
}

/// Build the question section: name + qtype + qclass.
fn question(name: &str) -> Vec<u8> {
    let mut q = encode_name(name);
    q.extend_from_slice(&1u16.to_be_bytes()); // qtype = A
    q.extend_from_slice(&1u16.to_be_bytes()); // qclass = IN
    q
}

/// An A record with a compressed-pointer name.
/// `name_offset` is the absolute offset in the datagram of the name to
/// point to (typically 12 for the qname, but can be any valid offset).
fn a_record(name_offset: u16, ttl: u32, ip: [u8; 4]) -> Vec<u8> {
    let mut rr = Vec::new();
    // Compressed pointer: high 2 bits = 11, remaining 14 bits = offset
    let ptr = 0xc000 | (name_offset & 0x3fff);
    rr.extend_from_slice(&ptr.to_be_bytes());
    rr.extend_from_slice(&1u16.to_be_bytes()); // type A
    rr.extend_from_slice(&1u16.to_be_bytes()); // class IN
    rr.extend_from_slice(&ttl.to_be_bytes());
    rr.extend_from_slice(&4u16.to_be_bytes()); // rdlength
    rr.extend_from_slice(&ip);
    rr
}

/// A CNAME record with a compressed-pointer name and an inline canonical
/// name (no further compression in the RDATA, for test simplicity).
fn cname_record(name_offset: u16, ttl: u32, canonical: &str) -> Vec<u8> {
    let mut rr = Vec::new();
    let ptr = 0xc000 | (name_offset & 0x3fff);
    rr.extend_from_slice(&ptr.to_be_bytes());
    rr.extend_from_slice(&5u16.to_be_bytes()); // type CNAME
    rr.extend_from_slice(&1u16.to_be_bytes()); // class IN
    rr.extend_from_slice(&ttl.to_be_bytes());
    let rdata = encode_name(canonical);
    rr.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
    rr.extend_from_slice(&rdata);
    rr
}

/// An NS record (type 2) — exercises "skip unknown type via rdlength".
fn ns_record(name_offset: u16, ttl: u32, ns_name: &str) -> Vec<u8> {
    let mut rr = Vec::new();
    let ptr = 0xc000 | (name_offset & 0x3fff);
    rr.extend_from_slice(&ptr.to_be_bytes());
    rr.extend_from_slice(&2u16.to_be_bytes()); // type NS
    rr.extend_from_slice(&1u16.to_be_bytes()); // class IN
    rr.extend_from_slice(&ttl.to_be_bytes());
    let rdata = encode_name(ns_name);
    rr.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
    rr.extend_from_slice(&rdata);
    rr
}

// ---------------------------------------------------------------------------
// Fake-resolver harness
// ---------------------------------------------------------------------------

/// Handle to a one-shot fake-resolver worker (mirrors `harness::EchoServer`).
struct FakeResolver {
    done: mpsc::Receiver<(UdpSocket, Result<(), String>)>,
}

impl FakeResolver {
    /// Reap the worker: drops the port-53 socket inline (UDP closes are
    /// synchronous — this frees the port for the next dns test), then returns
    /// the worker's outcome. On reap timeout the worker, and port 53 with it,
    /// leak.
    fn finish(self) -> Result<(), String> {
        match self.done.recv_timeout(Duration::from_secs(10)) {
            Ok((sock, outcome)) => {
                drop(sock);
                outcome
            }
            Err(_) => Err("fake resolver did not finish within 10 s \
                 (worker and port 53 leaked; later dns tests may fail to bind it)"
                .to_string()),
        }
    }
}

/// Reap a fake-resolver worker, panicking if it failed — its diagnostics beat
/// the lookup error it usually causes. Call between collecting the lookup
/// result and asserting on it (collect-discard-assert).
fn reap(resolver: FakeResolver) {
    if let Err(e) = resolver.finish() {
        panic!("fake resolver: {}", e);
    }
}

/// Bind (self_ip, 53) ON THE TEST THREAD — the resolver must be listening
/// before the lookup, or the query lands on an unbound port and the dns
/// resolver recv parks forever (#880) — then serve ONE query on a worker
/// thread: parse the 2-byte transaction id, hand it to `shapes`, and send each
/// returned datagram back in order. The socket rides back through the channel
/// so a worker-side drop cannot race the reap.
fn fake_resolver(shapes: impl FnOnce(u16) -> Vec<Vec<u8>> + Send + 'static) -> FakeResolver {
    let sock = match UdpSocket::bind(SocketAddr::new(self_ip(), 53)) {
        Ok(s) => s,
        Err(e) => panic!(
            "bind of the fake resolver on {}:53 failed ({}); an earlier dns test likely leaked the port",
            self_ip(),
            e
        ),
    };
    // Post-#880-fix safety net so a query-less worker can exit on its own; on
    // dev a quiet socket never fires this timeout (NTC-1) and the worker leaks.
    check!(sock.set_read_timeout(Some(Duration::from_secs(15))));
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let outcome = (|| {
            let mut buf = [0u8; 512]; // DNS_PKT_MAX_LEN; queries are far smaller
            let (len, peer) = sock.recv_from(&mut buf).map_err(|e| format!("recv_from(query): {}", e))?;
            if len < 2 {
                return Err(format!("runt query: {} bytes", len));
            }
            let id = u16::from_be_bytes([buf[0], buf[1]]);
            log::info!("fake resolver: query id={:#06x} len={} from {}", id, len, peer);
            for (seq, dgram) in shapes(id).into_iter().enumerate() {
                sock.send_to(&dgram, peer).map_err(|e| format!("send_to(response {}): {}", seq, e))?;
            }
            Ok(())
        })();
        tx.send((sock, outcome)).ok();
    });
    FakeResolver { done: rx }
}

/// Resolve "<name>:<port>" through the full std chain, collected to a Vec.
/// Bounded: on dev an unanswerable query parks the dns service's resolver
/// recv forever (#880), which parks this lookup with it.
fn std_lookup(name: &'static str, port: u16) -> io::Result<Vec<SocketAddr>> {
    let desc = format!("std lookup of {}", name);
    bounded(&desc, 20, move || {
        format!("{}:{}", name, port).to_socket_addrs().map(|iter| iter.collect::<Vec<_>>())
    })
}

/// Assert a lookup succeeded and returned exactly `ips` (as an unordered set:
/// the dns service stores answers in a HashMap, so order is unspecified) with
/// `port` propagated onto every address.
fn assert_resolves_to(result: io::Result<Vec<SocketAddr>>, ips: &[[u8; 4]], port: u16) {
    let addrs = match result {
        Ok(addrs) => addrs,
        Err(e) => {
            panic!("lookup failed with {} (kind {:?}), want {} address(es)", e, e.kind(), ips.len())
        }
    };
    assert_eq!(addrs.len(), ips.len(), "expected {} address(es), got {:?}", ips.len(), addrs);
    for &ip in ips {
        let want = SocketAddr::new(IpAddr::V4(Ipv4Addr::from(ip)), port);
        assert!(addrs.contains(&want), "lookup result {:?} is missing {}", addrs, want);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// "localhost:<port>" resolves to 127.0.0.1:<port> with no network traffic —
/// the dns service special-cases the name ahead of cache and resolver. Also the
/// end-to-end canary for the ToSocketAddrs -> LookupHost -> RawLookup path.
pub fn dns_localhost_builtin() {
    let port = next_port(); // never bound — only propagated through the lookup
    let result = bounded("localhost lookup", 20, move || {
        format!("localhost:{}", port).to_socket_addrs().map(|iter| iter.collect::<Vec<_>>())
    });
    let addrs = check!(result);
    assert_eq!(
        addrs,
        vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)],
        "localhost must resolve to exactly 127.0.0.1 with the port propagated"
    );
}

/// A numeric "ip:port" string parses entirely client-side (std tries a
/// SocketAddr parse before any resolution), so it works even with the dns
/// service wedged — deliberately unguarded, registered ahead of resolver tests.
pub fn dns_numeric_addr_bypasses_resolver() {
    let port = next_port();
    let want = SocketAddr::new(self_ip(), port);
    let addrs: Vec<SocketAddr> = check!(format!("{}:{}", self_ip(), port).to_socket_addrs()).collect();
    assert_eq!(addrs, vec![want], "numeric addr string must parse verbatim, no DNS involved");
}

/// A plain all-A response with every answer name compressed to the qname
/// (offset 0x0c) resolves to all records, port propagated — the positive
/// control proving the fake-resolver rig before the parser tests run.
pub fn dns_a_record_response_ok() {
    const NAME: &str = "t3.a.test";
    const IPS: [[u8; 4]; 3] = [[192, 0, 2, 1], [192, 0, 2, 2], [192, 0, 2, 3]];
    let port = next_port();
    let resolver = fake_resolver(move |id| {
        let mut dgram = header(id, IPS.len() as u16);
        dgram.extend_from_slice(&question(NAME));
        for &ip in &IPS {
            dgram.extend_from_slice(&a_record(12, 255, ip));
        }
        vec![dgram]
    });
    let result = std_lookup(NAME, port);
    reap(resolver);
    assert_resolves_to(result, &IPS, port);
}

/// A CNAME + A response whose A records compress to a mid-message offset must
/// resolve to the A records: the resolver chases the CNAME server-side, so the
/// client only skips the CNAME and follows the non-qname pointer.
/// XFAIL: dev's answer walker rejects non-{A,AAAA} types and offsets != 0x0c as FormatError, services/dns/src/main.rs:140-159.
pub fn dns_cname_chain_response() {
    const NAME: &str = "t4.cn.test";
    const IPS: [[u8; 4]; 2] = [[198, 51, 100, 7], [198, 51, 100, 8]];
    let port = next_port();
    let resolver = fake_resolver(move |id| {
        let mut dgram = header(id, 3);
        dgram.extend_from_slice(&question(NAME));
        // The A records' names point at the canonical name stored inside the
        // CNAME's RDATA — a non-qname offset. The RDATA starts 12 bytes into
        // the CNAME record (2 ptr + 2 type + 2 class + 4 ttl + 2 rdlength),
        // and the answer section starts at the current end of the datagram.
        let canonical_offset = (dgram.len() + 12) as u16;
        dgram.extend_from_slice(&cname_record(12, 296, "canon.t4.cn.test"));
        for &ip in &IPS {
            dgram.extend_from_slice(&a_record(canonical_offset, 296, ip));
        }
        vec![dgram]
    });
    let result = std_lookup(NAME, port);
    reap(resolver);
    assert_resolves_to(result, &IPS, port);
}

/// A non-address record (NS=2) in the answer section must be skipped via its
/// rdlength, not treated as fatal; the A record after it still resolves.
/// XFAIL: dev's walker rejects any type not in {A,AAAA} as FormatError, services/dns/src/main.rs:156-159.
pub fn dns_ns_in_answer_tolerated() {
    const NAME: &str = "t5.ns.test";
    const IPS: [[u8; 4]; 1] = [[192, 0, 2, 53]];
    let port = next_port();
    let resolver = fake_resolver(move |id| {
        let mut dgram = header(id, 2);
        dgram.extend_from_slice(&question(NAME));
        dgram.extend_from_slice(&ns_record(12, 60, "ns1.t5.ns.test"));
        dgram.extend_from_slice(&a_record(12, 60, IPS[0]));
        vec![dgram]
    });
    let result = std_lookup(NAME, port);
    reap(resolver);
    assert_resolves_to(result, &IPS, port);
}

/// A response whose question section carries a bogus qclass (99) must be
/// rejected as malformed; through std the failure surfaces as InvalidInput
/// "DNS failure".
pub fn dns_bad_qclass_rejected() {
    const NAME: &str = "t6.qc.test";
    let port = next_port();
    let resolver = fake_resolver(move |id| {
        // Ported from 4012f51bd's rejects_bad_qclass; the question section is
        // built inline because question() hardcodes the valid qclass.
        let mut dgram = header(id, 0);
        dgram.extend_from_slice(&encode_name(NAME));
        dgram.extend_from_slice(&1u16.to_be_bytes()); // qtype = A
        dgram.extend_from_slice(&99u16.to_be_bytes()); // qclass = bogus
        vec![dgram]
    });
    let result = std_lookup(NAME, port);
    reap(resolver);
    match result {
        Ok(addrs) => panic!("bad-qclass response unexpectedly resolved to {:?}", addrs),
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::InvalidInput,
            "DNS failure surfaced as {:?} ({}), want InvalidInput (E-14 pin)",
            e.kind(),
            e
        ),
    }
}

/// DANGER — NOT REGISTERED: a response whose rdata is shorter than its rdlength
/// must be rejected as a clean FormatError. Disabled because on dev the answer
/// walker slices past the datagram end (unchecked indexing) and PANICS the dns
/// service, which the CI driver treats as a hard failure and wedges later lookups.
#[allow(dead_code)]
pub fn dns_truncated_rdata_disabled() {
    const NAME: &str = "t7.tr.test";
    let port = next_port();
    let resolver = fake_resolver(move |id| {
        let mut dgram = header(id, 1);
        dgram.extend_from_slice(&question(NAME));
        // An A record truncated mid-rdata: rdlength promises 4 bytes, the
        // datagram ends after 2.
        dgram.push(0xc0);
        dgram.push(0x0c);
        dgram.extend_from_slice(&1u16.to_be_bytes()); // type A
        dgram.extend_from_slice(&1u16.to_be_bytes()); // class IN
        dgram.extend_from_slice(&60u32.to_be_bytes()); // ttl
        dgram.extend_from_slice(&4u16.to_be_bytes()); // rdlength = 4
        dgram.extend_from_slice(&[1, 2]); // ...but only 2 bytes of rdata
        vec![dgram]
    });
    let result = std_lookup(NAME, port);
    reap(resolver);
    match result {
        Ok(addrs) => panic!("truncated response unexpectedly resolved to {:?}", addrs),
        Err(e) => log::info!("truncated response rejected cleanly: {} (kind {:?})", e, e.kind()),
    }
}

/// An A record whose rdlength is not 4 violates the RR contract, so the lookup
/// must fail — both dev and the fix reject it (all 8 bytes present, no
/// truncation in play).
pub fn dns_a_rdlength_not_4() {
    const NAME: &str = "t8.len.test";
    let port = next_port();
    let resolver = fake_resolver(move |id| {
        // Ported from 4012f51bd's rejects_length_mismatched_a_record.
        let mut dgram = header(id, 1);
        dgram.extend_from_slice(&question(NAME));
        dgram.push(0xc0);
        dgram.push(0x0c);
        dgram.extend_from_slice(&1u16.to_be_bytes()); // type A
        dgram.extend_from_slice(&1u16.to_be_bytes()); // class IN
        dgram.extend_from_slice(&60u32.to_be_bytes()); // ttl
        dgram.extend_from_slice(&8u16.to_be_bytes()); // rdlength = 8 (wrong)
        dgram.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        vec![dgram]
    });
    let result = std_lookup(NAME, port);
    reap(resolver);
    match result {
        Ok(addrs) => panic!("length-mismatched A record unexpectedly resolved to {:?}", addrs),
        Err(e) => log::info!("rdlength != 4 rejected as the contract requires: {} (kind {:?})", e, e.kind()),
    }
}

/// A NOERROR response with zero answers must surface as a lookup error
/// (getaddrinfo EAI_NODATA analog), never as a success carrying no addresses;
/// the error kind is left unconstrained (platform-defined).
/// XFAIL: dev encodes an empty resolve as SUCCESS entry_count=0 and caches it, so the lookup returns Ok([]), services/dns/src/main.rs:401-444.
pub fn dns_zero_answers_is_error() {
    const NAME: &str = "t10.zero.test";
    let port = next_port();
    let resolver = fake_resolver(move |id| {
        let mut dgram = header(id, 0); // NOERROR flags, ancount = 0
        dgram.extend_from_slice(&question(NAME));
        vec![dgram]
    });
    let result = std_lookup(NAME, port);
    reap(resolver);
    match result {
        Ok(addrs) => {
            panic!("zero-answer NOERROR response must be a lookup error, got Ok({:?})", addrs)
        }
        Err(e) => {
            log::info!("zero-answer response errored as the contract requires: {} (kind {:?})", e, e.kind())
        }
    }
}

/// An unresolvable name reached through TcpStream::connect("<name>:<port>")
/// fails with kind InvalidInput — the xous pin for DNS failures. The kind is
/// asserted, not the message; either funnel yields no address so connect never dials.
pub fn dns_error_kind_via_std() {
    const NAME: &str = "t11.nx.test";
    let port = next_port();
    let resolver = fake_resolver(move |id| {
        // NXDOMAIN shape: QR|RD|RA with rcode=3, zero answers, question echoed.
        let mut dgram = header_with_flags(id, 0x8183, 0);
        dgram.extend_from_slice(&question(NAME));
        vec![dgram]
    });
    let result = bounded("connect to an unresolvable name", 20, move || {
        TcpStream::connect(format!("{}:{}", NAME, port))
    });
    reap(resolver);
    match result {
        Ok(stream) => {
            discard(stream);
            panic!("connect to an NXDOMAIN-answered name unexpectedly succeeded");
        }
        Err(e) => assert_eq!(
            e.kind(),
            ErrorKind::InvalidInput,
            "DNS failure through connect surfaced as {:?} ({}), want InvalidInput (E-14 pin)",
            e.kind(),
            e
        ),
    }
}

/// A response with a mismatched transaction id must not satisfy the query: the
/// resolver keeps listening and returns the right-id answer. The worker sends a
/// wrong-id decoy then the right-id answer; registered LAST (see the drain below).
/// XFAIL: dev does a single recv per lookup, so an id mismatch errors immediately, services/dns/src/main.rs:351-367.
pub fn dns_wrong_txn_id_ignored() {
    const NAME: &str = "t9.id.test";
    const DECOY_IP: [u8; 4] = [192, 0, 2, 66];
    const REAL_IP: [u8; 4] = [192, 0, 2, 99];
    let port = next_port();
    let resolver = fake_resolver(move |id| {
        let wrong_id = id ^ 0xa5a5; // xor with a nonzero constant: never equal to id
        let mut wrong = header(wrong_id, 1);
        wrong.extend_from_slice(&question(NAME));
        wrong.extend_from_slice(&a_record(12, 60, DECOY_IP));
        let mut right = header(id, 1);
        right.extend_from_slice(&question(NAME));
        right.extend_from_slice(&a_record(12, 60, REAL_IP));
        vec![wrong, right]
    });
    let result = std_lookup(NAME, port);
    let worker = resolver.finish();
    if result.is_err() && worker.is_ok() {
        // Dev deviation path: exactly one stale (right-id) datagram is queued
        // on the resolver socket — consume it before any assert can panic.
        let drained = bounded("drain of the stale right-id datagram", 20, move || {
            "t9drain.id.test:80".to_socket_addrs().map(|iter| iter.count())
        });
        log::info!("drain lookup returned {:?} (outcome irrelevant; the queue is clean either way)", drained);
    }
    if let Err(e) = worker {
        panic!("fake resolver: {}", e);
    }
    assert_resolves_to(result, &[REAL_IP], port);
}

/// This theme's registry (aggregated by tests::all_tests / all_xfails).
/// Ordering is load-bearing: the resolver-free canaries come first, then the
/// positive-control rig test, and dns_wrong_txn_id_ignored stays LAST;
/// dns_truncated_rdata_disabled is DANGER-disabled and deliberately absent.
pub const TESTS: &[TestEntry] = &[
    ("dns::dns_localhost_builtin", dns_localhost_builtin as fn()),
    ("dns::dns_numeric_addr_bypasses_resolver", dns_numeric_addr_bypasses_resolver as fn()),
    ("dns::dns_a_record_response_ok", dns_a_record_response_ok as fn()),
    ("dns::dns_cname_chain_response", dns_cname_chain_response as fn()),
    ("dns::dns_ns_in_answer_tolerated", dns_ns_in_answer_tolerated as fn()),
    ("dns::dns_bad_qclass_rejected", dns_bad_qclass_rejected as fn()),
    ("dns::dns_a_rdlength_not_4", dns_a_rdlength_not_4 as fn()),
    ("dns::dns_zero_answers_is_error", dns_zero_answers_is_error as fn()),
    ("dns::dns_error_kind_via_std", dns_error_kind_via_std as fn()),
    ("dns::dns_wrong_txn_id_ignored", dns_wrong_txn_id_ignored as fn()),
];

pub const XFAILS: &[XfailEntry] = &[
    ("dns::dns_cname_chain_response", "NTC-12"),
    ("dns::dns_ns_in_answer_tolerated", "NTC-12"),
    ("dns::dns_zero_answers_is_error", "NTC-17"),
    ("dns::dns_wrong_txn_id_ignored", "NTC-18"),
];
