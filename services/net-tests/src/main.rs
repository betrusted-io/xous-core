//! std::net test runner for Renode CI.
//!
//! Joins the (emulated) wlan, waits for the net service to report an IPv4
//! config (renode-minimal seeds a static one at boot, since no DHCP peer
//! exists), then runs every test in `tests::TESTS` under `catch_unwind` and
//! emits machine-parsable sentinels on the log console for the Renode driver.
//! Sentinel grammar mirrors pddb-fs-tests, plus a `NET-TESTS DONE:` totals
//! line so drivers cannot cross-match suites. Authoring rules: tests/mod.rs.

mod harness;
mod tests;

use std::panic::{self, AssertUnwindSafe};
use std::sync::Mutex;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// The panic hook records the message here so the runner can report it as the
/// one-line FAIL reason. Mandatory: the default hook prints a `PANIC in PID`
/// banner the CI driver treats as a hard failure even for recovered panics.
static PANIC_MSG: Mutex<Option<String>> = Mutex::new(None);

/// Flatten a panic message to its first line, truncated to ~120 chars.
fn one_line(reason: &str) -> String {
    let first = reason.lines().next().unwrap_or("").replace('\r', " ");
    let mut flat: String = first.chars().take(120).collect();
    if first.chars().count() > 120 {
        flat.push_str("...");
    }
    flat
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    // Record panics instead of printing the default banner (see PANIC_MSG).
    panic::set_hook(Box::new(|info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "non-string panic payload".to_string()
        };
        let msg = match info.location() {
            Some(loc) => format!("{} (at {}:{})", msg, loc.file(), loc.line()),
            None => msg,
        };
        *PANIC_MSG.lock().unwrap() = Some(msg);
    }));

    let tt = ticktimer_server::Ticktimer::new().unwrap();

    // The ticktimer arms a ~30 s hardware watchdog and feeds it only on
    // message receipt; this minimal image can go quiet longer than that when a
    // test parks in a long guard, resetting the SoC. Feed it for the run's life.
    std::thread::spawn(|| {
        let tt = ticktimer_server::Ticktimer::new().unwrap();
        loop {
            tt.ping_wdt();
            tt.sleep_ms(5000).unwrap();
        }
    });

    // let the rest of the boot sequence quiesce before driving the COM
    tt.sleep_ms(1000).unwrap();
    let all_tests = tests::all_tests();
    let all_xfails = tests::all_xfails();

    // Readiness: issue the wlan join (cf. services/libstd-test), then poll for
    // the IPv4 config the net service acquires. Both failure paths emit a
    // deterministic failing DONE line instead of hanging the run.
    let xns = xous_names::XousNames::new().unwrap();
    let mut com = com::Com::new(&xns).unwrap();
    if let Err(e) = com.wlan_join() {
        log::info!("TEST readiness::wlan_join FAIL couldn't issue join command: {:?}", e);
        log::info!("NET-TESTS DONE: pass=0 fail=1 xfail=0 xpass=0 total=1");
        log::info!("CI done");
        xous::terminate_process(0);
    }
    log::info!("wlan join issued; waiting for an IPv4 config...");
    let net_mgr = net::NetManager::new();
    let mut net_cfg = None;
    for poll in 1..=240u32 {
        // 240 polls x 500 ms = 120 s budget
        if let Some(cfg) = net_mgr.get_ipv4_config() {
            net_cfg = Some(cfg);
            break;
        }
        if poll % 5 == 0 {
            // inactivity-reaper safety: show signs of life while waiting
            log::info!("still waiting for an IPv4 config ({} polls)", poll);
        }
        tt.sleep_ms(500).unwrap();
    }
    let cfg = match net_cfg {
        Some(cfg) => cfg,
        None => {
            log::info!("TEST readiness::ipv4_config FAIL no IPv4 config within 120 s");
            log::info!("NET-TESTS DONE: pass=0 fail=1 xfail=0 xpass=0 total=1");
            log::info!("CI done");
            xous::terminate_process(0);
        }
    };
    log::info!("net is up at {:?}; running {} tests", std::net::IpAddr::from(cfg.addr), all_tests.len());

    let mut pass = 0usize;
    let mut fail = 0usize;
    let mut xfail = 0usize;
    let mut xpass = 0usize;
    for &(name, test) in all_tests.iter() {
        PANIC_MSG.lock().unwrap().take();
        log::info!("TEST {} START", name);
        // Each test runs on a worker thread with a per-test verdict deadline:
        // std::net calls lend blockingly into the net server and known bugs
        // (NTC-1, NTC-5) can park them forever. On timeout the worker leaks,
        // the test is a hard FAIL, and the run still reaches `NET-TESTS DONE`.
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            tx.send(panic::catch_unwind(AssertUnwindSafe(test)).is_ok()).ok();
        });
        let verdict = rx.recv_timeout(Duration::from_secs(60)).ok();
        let expected_bug = all_xfails.iter().find_map(|&(n, bug)| if n == name { Some(bug) } else { None });
        match (verdict, expected_bug) {
            (Some(true), None) => {
                pass += 1;
                log::info!("TEST {} PASS", name);
            }
            (Some(true), Some(bug)) => {
                xpass += 1;
                log::info!("TEST {} XPASS {}", name, bug);
            }
            (Some(false), Some(bug)) => {
                xfail += 1;
                log::info!("TEST {} XFAIL {}", name, bug);
            }
            (Some(false), None) => {
                fail += 1;
                let reason = PANIC_MSG
                    .lock()
                    .unwrap()
                    .take()
                    .unwrap_or_else(|| "panicked without a recorded message".to_string());
                log::info!("TEST {} FAIL {}", name, one_line(&reason));
            }
            (None, _) => {
                // a wedge is never an expected failure: XFAIL tests are
                // structured to convert their bug's hang into a panic
                fail += 1;
                log::info!("TEST {} FAIL wedged: no verdict within 60 s (worker thread leaked)", name);
            }
        }
    }

    log::info!(
        "NET-TESTS DONE: pass={} fail={} xfail={} xpass={} total={}",
        pass,
        fail,
        xfail,
        xpass,
        all_tests.len()
    );
    log::info!("CI done");
    xous::terminate_process(0)
}
