//! Shared plumbing for the net-tests suite: per-test port isolation, the
//! upstream `check!`/`error_contains!` assertion macros, a deterministic RNG
//! (OS randomness is unsupported on xous — do not use the `rand` crate), the
//! DUT's own IP, an echo-server helper, and hang containment for blocking
//! calls that known bugs can park forever. A superset toolkit: loopback uses all
//! of it, cross-host only a subset (it talks to a real peer, not a self-hosted echo
//! server), so unused helpers are tolerated in the cross-host build.
#![cfg_attr(feature = "cross-host", allow(dead_code))]

use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

static PORT_COUNTER: AtomicU16 = AtomicU16::new(20000);

/// Allocate a fresh port. Ports are NEVER reused across tests: `bounded` leaks
/// blocked threads that keep their sockets alive, and the net server has no
/// SO_REUSEADDR (re-bind fails SocketInUse). 20000+ avoids the ephemeral range.
pub fn next_port() -> u16 { PORT_COUNTER.fetch_add(1, Ordering::SeqCst) }

/// The DUT's own IP, queried from the net service once and cached. Under
/// renode-minimal the net service seeds a static config at boot (10.0.2.15/24);
/// the readiness gate guarantees it is present before any test runs.
pub fn self_ip() -> IpAddr {
    static SELF_IP: OnceLock<IpAddr> = OnceLock::new();
    *SELF_IP.get_or_init(|| {
        let cfg = net::NetManager::new()
            .get_ipv4_config()
            .expect("no IPv4 config (the readiness gate should have caught this)");
        IpAddr::from(cfg.addr)
    })
}

/// Run a blocking op on a throwaway thread, waiting up to `secs`. Converts a
/// known class of net-server hangs (#880: read timeouts never serviced on quiet
/// sockets) into a deterministic panic. On timeout the worker leaks with its
/// sockets — fine because ports are never reused (see `next_port`).
pub fn bounded<T: Send + 'static>(desc: &str, secs: u64, op: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        tx.send(op()).ok();
    });
    match rx.recv_timeout(Duration::from_secs(secs)) {
        Ok(v) => v,
        Err(RecvTimeoutError::Timeout) => {
            panic!("{} did not complete within {} s (worker thread leaked)", desc, secs)
        }
        Err(RecvTimeoutError::Disconnected) => panic!("{} worker thread panicked", desc),
    }
}

/// Hand a value (usually a TCP socket) to a detached thread to drop it there.
/// A TCP drop issues a blocking StdTcpClose the net server only services when
/// iface.poll() reports activity (NTC-5): a traffic-free close blocks forever.
/// Tests never drop TCP sockets inline; the worker leaks if it hangs. UDP is safe.
pub fn discard<T: Send + 'static>(x: T) { thread::spawn(move || drop(x)); }

/// Handle to a running `echo_server` thread; `wait` panics rather than
/// hanging the suite if the thread never sees EOF.
pub struct EchoServer {
    done: mpsc::Receiver<Result<usize, String>>,
}
impl EchoServer {
    /// Wait for the server thread to see EOF and exit; returns the byte count
    /// it echoed.
    pub fn wait(self) -> usize {
        match self.done.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(total)) => total,
            Ok(Err(e)) => panic!("echo server failed: {}", e),
            Err(_) => panic!("echo server did not exit within 10 s (EOF never delivered?)"),
        }
    }
}

/// Spawn a thread that accepts ONE connection on `listener` and echoes bytes
/// back until EOF. I/O errors are reported through `EchoServer::wait`, not by
/// panicking on the worker thread.
pub fn echo_server(listener: TcpListener) -> EchoServer {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = (|| {
            let (mut stream, _peer) = listener.accept().map_err(|e| format!("accept: {}", e))?;
            let mut total = 0usize;
            let mut chunks = 0usize;
            let mut buf = [0u8; 1024];
            loop {
                let n = stream.read(&mut buf).map_err(|e| format!("server read: {}", e))?;
                if n == 0 {
                    return Ok(total);
                }
                stream.write_all(&buf[..n]).map_err(|e| format!("server write: {}", e))?;
                total += n;
                chunks += 1;
                if chunks % 5 == 0 {
                    // inactivity-reaper rule: long op loops must emit output
                    log::info!("echo_server: {} chunks, {} bytes echoed", chunks, total);
                }
            }
        })();
        tx.send(result).ok();
    });
    EchoServer { done: rx }
}

/// Upstream idiom from rust's library/std tests: unwrap a Result with the
/// failing expression in the panic message.
macro_rules! check {
    ($e:expr) => {
        match $e {
            Ok(t) => t,
            Err(e) => panic!("{} failed with: {e}", stringify!($e)),
        }
    };
}
pub(crate) use check;

/// Upstream idiom from rust's library/std tests: assert that a Result is an
/// Err whose message contains `$s`.
#[allow(unused_macros)]
macro_rules! error_contains {
    ($e:expr, $s:expr) => {
        match $e {
            Ok(_) => panic!("Unexpected success. Should've been: {:?}", $s),
            Err(ref err) => {
                assert!(err.to_string().contains($s), "`{}` did not contain `{}`", err, $s)
            }
        }
    };
}
#[allow(unused_imports)]
pub(crate) use error_contains;

/// Deterministic xorshift32 RNG for generating test data.
pub struct XorShift(u32);
impl XorShift {
    pub fn new(seed: u32) -> Self { XorShift(if seed == 0 { 0xDEAD_BEEF } else { seed }) }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    pub fn fill(&mut self, buf: &mut [u8]) {
        for b in buf.iter_mut() {
            *b = self.next_u32() as u8;
        }
    }
}
