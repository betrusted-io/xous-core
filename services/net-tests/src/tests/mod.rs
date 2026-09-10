//! Test registry, plus the rules for adding a test.
//!
//! Each theme module owns its `TESTS` and `XFAILS` tables; this module only
//! aggregates them. A test is a `pub fn` that panics to fail and returns to
//! pass; register it in its theme's `TESTS`, and if it reproduces a known bug
//! add it to `XFAILS` against an NTC id. Never weaken an assertion to make a
//! test pass — assert the correct behavior and register the XFAIL; every XFAIL
//! fn carries a doc stating the symptom, mechanism, and suspected code site.
//!
//! xous/net rules that trip up std::net habits:
//! - Port isolation: allocate every port through `harness::next_port` and
//!   never reuse one — the server has no SO_REUSEADDR and leaked blocked
//!   threads keep old ports bound (a re-bind fails with SocketInUse).
//! - HANG HAZARD: blocking calls a known bug can park forever go through
//!   `harness::bounded`, which converts a hang into a deterministic panic.
//! - CLOSE HAZARD: dropping a TCP socket issues a blocking close that can hang
//!   forever, so never drop TCP sockets on the test thread — release them with
//!   `harness::discard` (UDP drops are synchronous and safe inline). Inside a
//!   `bounded` worker, return sockets to the test thread with the result and
//!   discard there, in collect-discard-assert order.
//! - A loop of >~10 socket ops must `log::info!` every ~5 iterations, or the
//!   driver's inactivity reaper treats console silence as a dead server.
//! - Deterministic data only: `harness::XorShift`, never the `rand` crate.
//! - Never call `NetManager` wifi-stats/SSID-list APIs: under renode-minimal
//!   the connection manager thread is not spawned and those requests hang.

// the loopback suite and cross-host (cross-host) are mutually exclusive suites; each
// image compiles only its own themes so the other's tests don't warn as unused.
#[cfg(not(feature = "cross-host"))]
pub mod concur;
#[cfg(not(feature = "cross-host"))]
pub mod dns;
#[cfg(not(feature = "cross-host"))]
pub mod errors;
#[cfg(not(feature = "cross-host"))]
pub mod smoke;
#[cfg(not(feature = "cross-host"))]
pub mod sockopts;
#[cfg(not(feature = "cross-host"))]
pub mod tcp;
#[cfg(feature = "cross-host")]
pub mod cross_host;
#[cfg(not(feature = "cross-host"))]
pub mod timeouts;
#[cfg(not(feature = "cross-host"))]
pub mod udp;

/// (unique `theme::snake_case` name, test fn). A test panics to fail and
/// returns normally to pass.
pub type TestEntry = (&'static str, fn());
/// (test name, NTC bug ID). A listed test that fails reports XFAIL; one that
/// passes reports XPASS (bug apparently fixed — update the theme's table!).
pub type XfailEntry = (&'static str, &'static str);

/// Every registered test, aggregated from the theme modules. the loopback suite
/// and cross-host (cross-host) are mutually exclusive suites sharing this harness
/// and runner. Within loopback, `timeouts` runs LAST because it ends with the
/// quarantined blackhole-connect tests that leak SynSent sockets toward
/// unreachable addresses, containing any future connect-jam to the tail.
pub fn all_tests() -> Vec<TestEntry> {
    let mut v: Vec<TestEntry> = Vec::new();
    #[cfg(not(feature = "cross-host"))]
    {
        v.extend_from_slice(smoke::TESTS);
        v.extend_from_slice(tcp::TESTS);
        v.extend_from_slice(udp::TESTS);
        v.extend_from_slice(errors::TESTS);
        v.extend_from_slice(sockopts::TESTS);
        v.extend_from_slice(dns::TESTS);
        v.extend_from_slice(concur::TESTS);
        v.extend_from_slice(timeouts::TESTS);
    }
    #[cfg(feature = "cross-host")]
    v.extend_from_slice(cross_host::TESTS);
    v
}

/// The known-bug registry, aggregated from the theme modules (same suite
/// split as all_tests).
pub fn all_xfails() -> Vec<XfailEntry> {
    let mut v: Vec<XfailEntry> = Vec::new();
    #[cfg(not(feature = "cross-host"))]
    {
        v.extend_from_slice(smoke::XFAILS);
        v.extend_from_slice(tcp::XFAILS);
        v.extend_from_slice(udp::XFAILS);
        v.extend_from_slice(errors::XFAILS);
        v.extend_from_slice(sockopts::XFAILS);
        v.extend_from_slice(dns::XFAILS);
        v.extend_from_slice(concur::XFAILS);
        v.extend_from_slice(timeouts::XFAILS);
    }
    #[cfg(feature = "cross-host")]
    v.extend_from_slice(cross_host::XFAILS);
    v
}
