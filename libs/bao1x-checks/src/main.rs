//! Host-side diagnostics for both slot maps in `bao1x-api`: the one-way counter map and the
//! per-target data slot map. The `const` assertions in the library are what gate the firmware
//! build; a const panic can only carry a static string, so this binary reads the same `pub`
//! tables on the host and reports exactly what is wrong. Run it after a build breaks, or in CI.
//!
//!     cargo run --bin bao1x-checks
//!
//! Exits non-zero on any inconsistency.
//!
//! Workspace note: if the root `.cargo/config.toml` pins a cross target, either add
//! `exclude = ["slot-check"]` to the root `Cargo.toml`, or pass `--target <host-triple>`, so
//! this builds for the host. If `bao1x-api` will not build for the host, pull the slot modules
//! in by path instead of depending on the crate (see the OWC note we discussed).

use bao1x_api::checks::data_slots::*;
use bao1x_api::checks::owc::*;

fn main() {
    let mut failures = 0usize;

    println!("==== one-way counter map ====");
    failures += owc_report_collisions();
    owc_print_occupancy();
    failures += owc_report_dupe_pairs();
    owc_report_headroom();

    for m in DATA_MAPS {
        println!("\n==== data slot map: {} ====", m.name);
        failures += data_report(m);
    }

    if failures == 0 {
        println!("\nOK: all slot maps consistent.");
    } else {
        eprintln!("\nFAIL: {} problem(s) found.", failures);
        std::process::exit(1);
    }
}

// ============================ one-way counter map ============================

fn owc_report_collisions() -> usize {
    let mut owner: Vec<Option<(&'static str, usize)>> = vec![None; OWC_TOTAL_SLOTS];
    let mut failures = 0usize;
    for c in OWC_MAP {
        if c.count == 0 {
            println!("ZERO-LENGTH claim `{}` at slot {}", c.name, c.first);
            failures += 1;
            continue;
        }
        for s in c.first..c.first + c.count {
            if s >= OWC_TOTAL_SLOTS {
                println!("OUT OF RANGE: `{}` claims slot {}", c.name, s);
                failures += 1;
                continue;
            }
            match owner[s] {
                Some((prev, pf)) => {
                    println!("COLLISION at slot {}: `{}` (@{}) and `{}` (@{})", s, prev, pf, c.name, c.first);
                    failures += 1;
                }
                None => owner[s] = Some((c.name, c.first)),
            }
        }
        if c.first + c.count > OWC_BOOT_SLOTS {
            println!("SPILL: `{}` reaches into the user region (>= {})", c.name, OWC_BOOT_SLOTS);
            failures += 1;
        }
    }
    failures
}

fn owc_print_occupancy() {
    let mut owner: Vec<Option<&'static str>> = vec![None; OWC_TOTAL_SLOTS];
    for c in OWC_MAP {
        for s in c.first..(c.first + c.count).min(OWC_TOTAL_SLOTS) {
            if owner[s].is_none() {
                owner[s] = Some(c.name);
            }
        }
    }
    println!("\n--- occupancy (boot region) ---");
    print_runs(&owner, OWC_BOOT_SLOTS);
}

fn owc_report_dupe_pairs() -> usize {
    let mut failures = 0usize;
    println!("\n--- revocation duplicate resolution ---");
    for p in OWC_DUPE_PAIRS {
        for i in 0..PUBKEY_SLOTS {
            let primary = p.primary + i;
            if p.distance > primary {
                println!("  {:<12} key {}: UNDERFLOW", p.name, i);
                failures += 1;
                continue;
            }
            let resolved = primary - p.distance;
            let expected = p.dupe + i;
            if resolved != expected {
                println!("  {:<12} key {}: -> {} expected {}  WRONG", p.name, i, resolved, expected);
                failures += 1;
            } else {
                println!("  {:<12} key {}: primary {:>3} -> dupe {:>3}  ok", p.name, i, primary, resolved);
            }
        }
    }
    failures
}

fn owc_report_headroom() {
    let mut claimed = vec![false; OWC_TOTAL_SLOTS];
    for c in OWC_MAP {
        for s in c.first..(c.first + c.count).min(OWC_TOTAL_SLOTS) {
            claimed[s] = true;
        }
    }
    println!("\n--- headroom above each revocation block ---");
    for c in OWC_MAP {
        if !c.name.contains("REVOCATION") {
            continue;
        }
        let mut headroom = 0usize;
        let mut s = c.first + c.count;
        while s < OWC_BOOT_SLOTS && !claimed[s] {
            headroom += 1;
            s += 1;
        }
        let flag = if headroom == 0 { "  <-- no room to grow" } else { "" };
        println!("  {:<28} headroom: {}{}", c.name, headroom, flag);
    }
}

// ============================ data slot map ============================

fn data_report(m: &DataMap) -> usize {
    let mut owner: Vec<Option<(&'static str, usize)>> = vec![None; MAX_DATA_SLOTS];
    let mut failures = 0usize;

    // primaries
    for c in m.common.iter().chain(m.target.iter()) {
        if !matches!(c.kind, SlotKind::Primary) {
            continue;
        }
        if c.count == 0 {
            println!("  ZERO-LENGTH `{}` at {}", c.name, c.first);
            failures += 1;
            continue;
        }
        for s in c.first..c.first + c.count {
            if s >= MAX_DATA_SLOTS {
                println!("  OUT OF RANGE `{}` slot {}", c.name, s);
                failures += 1;
                continue;
            }
            match owner[s] {
                Some((prev, pf)) => {
                    println!(
                        "  PRIMARY COLLISION at {}: `{}` (@{}) and `{}` (@{}) -- if intentional, mark one alias()",
                        s, prev, pf, c.name, c.first
                    );
                    failures += 1;
                }
                None => owner[s] = Some((c.name, c.first)),
            }
        }
    }

    // aliases
    for c in m.common.iter().chain(m.target.iter()) {
        if !matches!(c.kind, SlotKind::Alias) {
            continue;
        }
        for s in c.first..c.first + c.count {
            match owner.get(s).copied().flatten() {
                Some((backing, _)) => {
                    println!("  alias `{}` slot {} backs into `{}`  ok", c.name, s, backing)
                }
                None => {
                    println!("  UNBACKED alias `{}` slot {}", c.name, s);
                    failures += 1;
                }
            }
        }
    }

    // coverage cross-check happens in the library const asserts; here we just show the map
    let names: Vec<Option<&'static str>> = owner.iter().map(|o| o.map(|(n, _)| n)).collect();
    println!("  --- {} occupancy ---", m.name);
    print_runs_indented(&names, MAX_DATA_SLOTS);

    failures
}

// ============================ shared ============================

fn print_runs(owner: &[Option<&'static str>], upto: usize) {
    let mut s = 0;
    while s < upto {
        let here = owner[s];
        let start = s;
        while s + 1 < upto && owner[s + 1] == here {
            s += 1;
        }
        let label = here.unwrap_or("(free)");
        if start == s {
            println!("  {:>4}        {}", start, label);
        } else {
            println!("  {:>4}-{:<4}   {}", start, s, label);
        }
        s += 1;
    }
}

fn print_runs_indented(owner: &[Option<&'static str>], upto: usize) {
    let mut s = 0;
    while s < upto {
        let here = owner[s];
        let start = s;
        while s + 1 < upto && owner[s + 1] == here {
            s += 1;
        }
        let label = here.unwrap_or("(free)");
        if start == s {
            println!("    {:>4}        {}", start, label);
        } else {
            println!("    {:>4}-{:<4}   {}", start, s, label);
        }
        s += 1;
    }
}
