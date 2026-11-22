//! xous-ipc buffer regression runner for Renode CI.
//!
//! Runs every test in `tests::all_tests()` under `catch_unwind` and emits the
//! machine-parsable sentinel stream shared by the pddb-fs/net suites:
//! `TEST <name> START|PASS|FAIL|XFAIL|XPASS`, then `IPC-TESTS DONE: pass=..
//! fail=.. xfail=.. xpass=.. total=..`, then `CI done` (`CI done` terminates
//! the driver's watch loop; per-suite grammar keeps drivers from
//! cross-matching). Authoring rules live in tests.rs.

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

/// Raise our heap ceiling: the largest tests hold a 1 MiB `Vec` plus a
/// multi-page serialized buffer and a response copy at once. Same idiom as
/// swaptest1 / ipc-echo.
fn bump_heap() {
    const HEAP_LIMIT: usize = 1024 * 1024 * 8;
    let result =
        xous::rsyscall(xous::SysCall::AdjustProcessLimit(xous::Limits::HeapMaximum as usize, 0, HEAP_LIMIT));
    if let Ok(xous::Result::Scalar2(1, current_limit)) = result {
        xous::rsyscall(xous::SysCall::AdjustProcessLimit(
            xous::Limits::HeapMaximum as usize,
            current_limit,
            HEAP_LIMIT,
        ))
        .expect("couldn't raise heap limit");
    } else {
        log::warn!("heap limit bump unsupported; relying on default");
    }
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());
    bump_heap();

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
    // message receipt; a large serialization can run longer than that.
    // Feed it for the run's life.
    std::thread::spawn(|| {
        let tt = ticktimer_server::Ticktimer::new().unwrap();
        loop {
            tt.ping_wdt();
            tt.sleep_ms(5000).unwrap();
        }
    });

    // Let the rest of the boot sequence quiesce; the echo server must have
    // registered before the lend tests run.
    tt.sleep_ms(2000).unwrap();

    // Readiness gate: block until the echo server is reachable (xous-names
    // returns ServerNotFound until it registers). Both failure paths emit a
    // deterministic failing DONE line instead of hanging the run.
    let xns = xous_names::XousNames::new().unwrap();
    let mut echo_ready = false;
    for poll in 1..=60u32 {
        match xns.request_connection(ipc_echo::SERVER_NAME) {
            Ok(_cid) => {
                echo_ready = true;
                break;
            }
            Err(_) => {
                if poll % 10 == 0 {
                    log::info!("still waiting for {} ({} polls)", ipc_echo::SERVER_NAME, poll);
                }
                tt.sleep_ms(500).unwrap();
            }
        }
    }
    if !echo_ready {
        log::info!("TEST readiness::echo_server FAIL not registered within 30 s");
        log::info!("IPC-TESTS DONE: pass=0 fail=1 xfail=0 xpass=0 total=1");
        log::info!("CI done");
        xous::terminate_process(0);
    }

    let all_tests = tests::all_tests();
    log::info!("echo server up; running {} tests", all_tests.len());

    let mut pass = 0usize;
    let mut fail = 0usize;
    let mut xfail = 0usize;
    let mut xpass = 0usize;
    for &(name, test) in all_tests.iter() {
        PANIC_MSG.lock().unwrap().take();
        log::info!("TEST {} START", name);
        // Per-test verdict deadline: these tests are in-process, but the echo
        // tests lend into a second process, so a wedged server would otherwise
        // park the suite forever. On timeout the worker leaks and the run
        // still reaches `IPC-TESTS DONE`.
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            tx.send(panic::catch_unwind(AssertUnwindSafe(test)).is_ok()).ok();
        });
        let verdict = rx.recv_timeout(Duration::from_secs(120)).ok();
        match verdict {
            Some(true) => {
                pass += 1;
                log::info!("TEST {} PASS", name);
            }
            Some(false) => {
                fail += 1;
                let reason = PANIC_MSG
                    .lock()
                    .unwrap()
                    .take()
                    .unwrap_or_else(|| "panicked without a recorded message".to_string());
                log::info!("TEST {} FAIL {}", name, one_line(&reason));
            }
            None => {
                fail += 1;
                log::info!("TEST {} FAIL wedged: no verdict within 120 s (worker thread leaked)", name);
            }
        }
    }

    log::info!(
        "IPC-TESTS DONE: pass={} fail={} xfail={} xpass={} total={}",
        pass,
        fail,
        xfail,
        xpass,
        all_tests.len()
    );
    log::info!("CI done");
    xous::terminate_process(0)
}
