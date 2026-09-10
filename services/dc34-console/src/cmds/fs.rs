use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use bao1x_hal_service::trng::dabao::Trng;

const N: usize = 10;
const DIR: &str = "dc34";

fn path(suffix: usize) -> String { format!("{}/f{}", DIR, suffix) }

fn counter_path(label: &str) -> String { format!("{}/{}_calls", DIR, label) }

pub fn is_first_call() -> bool { fs::metadata(path(0)).is_err() }

pub fn first_call_setup() {
    fs::create_dir_all(DIR).unwrap();

    let xns = xous_names::XousNames::new().unwrap();
    let r = Trng::new(&xns).unwrap();
    for i in 0..N {
        if r.get_u32().unwrap() & 1 == 1 {
            log::info!("setting {} to 0", i);
            fs::write(path(i), b"0").unwrap();
        } else {
            log::info!("setting {} to nil", i);
            fs::write(path(i), b"").unwrap();
        }
    }
}

pub fn run_parity(parity: usize) {
    for i in (parity..N).step_by(2) {
        let p = path(i);
        let t = fs::read_to_string(&p).unwrap().trim().to_string();

        if t.is_empty() {
            log::info!("updating {} to {}", i, i);
            fs::write(&p, i.to_string()).unwrap();
        } else {
            let n: u32 = t.parse().unwrap();
            log::info!("updating {} to {}", i, n + 1);
            fs::write(&p, (n + 1).to_string()).unwrap();
        }
    }
}

pub fn bump_counter(label: &str) {
    let p = counter_path(label);
    let cur = fs::read_to_string(&p).unwrap_or_default();
    let n: u32 = cur.trim().parse().unwrap_or(0);
    fs::write(&p, (n + 1).to_string()).unwrap();
}

/// Verify that every file's content is consistent with the recorded
/// odd_calls / even_calls counters.
///
/// Because we don't persist the per-file initial state, each file can
/// legitimately hold one of two values:
///
///   * started empty  →  i + calls - 1   (after ≥ 1 call)
///   * started "0"    →  calls           (after ≥ 1 call)
///
/// If calls == 0 the file must still be "" or "0".
pub fn verify() {
    let odd_calls: u32 =
        fs::read_to_string(counter_path("odd")).unwrap_or_default().trim().parse().unwrap_or(0);
    let even_calls: u32 =
        fs::read_to_string(counter_path("even")).unwrap_or_default().trim().parse().unwrap_or(0);

    let mut errors: Vec<String> = Vec::new();

    for i in 0..N {
        let content = fs::read_to_string(path(i)).unwrap_or_default().trim().to_string();

        let calls: u32 = if i % 2 == 0 { even_calls } else { odd_calls };

        if calls == 0 {
            // No writes should have happened to this parity group yet.
            if content != "" && content != "0" {
                errors.push(format!("f{}: zero calls yet, expected \"\" or \"0\", got \"{}\"", i, content));
            }
        } else {
            // Two valid outcomes depending on initial state.
            let exp_empty_init = i as u32 + calls - 1;
            let exp_zero_init = calls;
            let actual = content.parse::<u32>().unwrap();

            if actual != exp_empty_init && actual != exp_zero_init {
                errors.push(format!(
                    "f{}: expected {} (init=\"\") or {} (init=\"0\"), got \"{}\"",
                    i, exp_empty_init, exp_zero_init, content
                ));
            }
        }
    }

    if errors.is_empty() {
        log::info!("OK: all {} files consistent (odd_calls={}, even_calls={})", N, odd_calls, even_calls);
    } else {
        log::error!(
            "FAIL: {} inconsistent file(s) (odd_calls={}, even_calls={}):",
            errors.len(),
            odd_calls,
            even_calls
        );
        for e in &errors {
            log::error!("  {}", e);
        }
    }
}

pub fn reset() {
    let mut removed = 0;

    // data files f0..f{N-1}
    for i in 0..N {
        match fs::remove_file(path(i)) {
            Ok(_) => removed += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::error!("reset: failed to remove {}: {}", path(i), e),
        }
    }

    // counters
    for label in ["odd", "even"] {
        match fs::remove_file(counter_path(label)) {
            Ok(_) => removed += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::error!("reset: failed to remove {}: {}", counter_path(label), e),
        }
    }

    log::info!("reset: removed {} file(s) from {}", removed, DIR);
}
/*
fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--verify" || a == "-v") {
        verify();
        return;
    }

    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let odd = secs % 2 == 1;

    if is_first_call() {
        first_call_setup();
    }

    if odd {
        run_parity(1);
        bump_counter("odd");
    } else {
        run_parity(0);
        bump_counter("even");
    }
}
*/
