//! PDDB std::fs test runner for Renode CI.
//!
//! Waits for the PDDB to mount, runs every test registered in `tests::TESTS`
//! (each wrapped in `catch_unwind`), and emits machine-parsable sentinels on the
//! log console for the Renode-side driver (tools/pddb-fs-ci.py or
//! emulation/tests/pddb-fs.robot). See the `tests` module header
//! (services/pddb-fs-tests/src/tests/mod.rs) for the rules on adding tests, and
//! services/pddb-fs-tests/README.md for the sentinel grammar.

mod harness;
mod tests;

use std::panic::{self, AssertUnwindSafe};
use std::sync::Mutex;

/// The panic hook records the panic message here so the runner can report it as
/// the one-line FAIL reason. A custom hook is mandatory: the default hook prints
/// a `PANIC in PID ...` banner on the console, which the CI driver treats as a
/// hard failure even for panics that `catch_unwind` recovers from.
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
    let pddb = pddb::Pddb::new();
    log::info!("waiting for the PDDB to mount...");
    pddb.is_mounted_blocking();
    // let the rest of the boot sequence quiesce before hammering the PDDB server
    tt.sleep_ms(1000).unwrap();
    let all_tests = tests::all_tests();
    let all_xfails = tests::all_xfails();
    log::info!("PDDB mounted; running {} tests", all_tests.len());

    let mut pass = 0usize;
    let mut fail = 0usize;
    let mut xfail = 0usize;
    let mut xpass = 0usize;
    for &(name, test) in all_tests.iter() {
        PANIC_MSG.lock().unwrap().take();
        let result = panic::catch_unwind(AssertUnwindSafe(test));
        let expected_bug = all_xfails.iter().find_map(|&(n, bug)| if n == name { Some(bug) } else { None });
        match (result, expected_bug) {
            (Ok(()), None) => {
                pass += 1;
                log::info!("TEST {} PASS", name);
            }
            (Ok(()), Some(bug)) => {
                xpass += 1;
                log::info!("TEST {} XPASS {}", name, bug);
            }
            (Err(_), Some(bug)) => {
                xfail += 1;
                log::info!("TEST {} XFAIL {}", name, bug);
            }
            (Err(_), None) => {
                fail += 1;
                let reason = PANIC_MSG
                    .lock()
                    .unwrap()
                    .take()
                    .unwrap_or_else(|| "panicked without a recorded message".to_string());
                log::info!("TEST {} FAIL {}", name, one_line(&reason));
            }
        }
    }

    log::info!(
        "FS-TESTS DONE: pass={} fail={} xfail={} xpass={} total={}",
        pass,
        fail,
        xfail,
        xpass,
        all_tests.len()
    );
    log::info!("CI done");
    xous::terminate_process(0)
}
