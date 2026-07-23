//! Host-side diagnostics for the one-way counter slot map.
//!
//! The `const` assertions in `bao1x-api` are what actually gate the firmware build. They
//! run on the cross target with no test harness, but a const panic can only carry a static
//! string. This binary reads the same `pub` tables on the host and says precisely what is
//! wrong, so it is the thing you run *after* the build breaks.
//!
//! Exits non-zero on any inconsistency, so it also works as a CI gate.
//!
//! Run with:
//!     cargo run -p owc-check
//!
//! If the workspace `.cargo/config.toml` pins `[build] target = "riscv32imac-..."`, that
//! applies to every cargo invocation in the tree and this will try to cross-compile a host
//! binary. Two ways out, pick whichever fits your layout:
//!   - `cargo run -p owc-check --target x86_64-unknown-linux-gnu`
//!   - keep `owc-check` out of the workspace (`exclude = ["owc-check"]` in the root Cargo.toml) so it gets
//!     its own config and builds for the host by default.
//!
//! If `bao1x-api` itself will not build for the host (utralib, target-specific deps), the
//! escape hatch is to skip the dependency and pull the definitions in directly:
//!
//!     #[path = "../../bao1x-api/src/owc_slots.rs"]
//!     mod owc_slots;
//!     use owc_slots::*;
//!
//! which needs the slot file to be free of target-specific imports. That is a good reason
//! to keep the OWC constants in their own leaf module with no `use` of hardware crates.

use bao1x_api::*;

fn main() {
    let mut failures = 0usize;

    failures += report_collisions();
    print_occupancy();
    failures += report_dupe_pairs();
    report_headroom();

    if failures == 0 {
        println!("\nOK: slot map is consistent.");
    } else {
        eprintln!("\nFAIL: {} problem(s) found.", failures);
        std::process::exit(1);
    }
}

/// Identify overlapping claims by name, which is the part the const assertion cannot say.
fn report_collisions() -> usize {
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
                println!("OUT OF RANGE: `{}` claims slot {} (max {})", c.name, s, OWC_TOTAL_SLOTS - 1);
                failures += 1;
                continue;
            }
            match owner[s] {
                Some((prev, prev_first)) => {
                    println!(
                        "COLLISION at slot {}: `{}` (starts {}) and `{}` (starts {})",
                        s, prev, prev_first, c.name, c.first
                    );
                    failures += 1;
                }
                None => owner[s] = Some((c.name, c.first)),
            }
        }
        if c.first + c.count > OWC_BOOT_SLOTS {
            println!(
                "SPILL: `{}` [{}..={}] reaches into the user application region (>= {})",
                c.name,
                c.first,
                c.first + c.count - 1,
                OWC_BOOT_SLOTS
            );
            failures += 1;
        }
    }
    failures
}

/// Print the boot region as a human-readable map, collapsing contiguous runs.
fn print_occupancy() {
    let mut owner: Vec<Option<&'static str>> = vec![None; OWC_TOTAL_SLOTS];
    for c in OWC_MAP {
        for s in c.first..(c.first + c.count).min(OWC_TOTAL_SLOTS) {
            if owner[s].is_none() {
                owner[s] = Some(c.name);
            }
        }
    }

    println!("\n--- one-way counter occupancy (boot region) ---");
    let mut s = 0usize;
    while s < OWC_BOOT_SLOTS {
        let here = owner[s];
        let start = s;
        while s + 1 < OWC_BOOT_SLOTS && owner[s + 1] == here {
            s += 1;
        }
        let label = here.unwrap_or("(free)");
        if start == s {
            println!("  {:>3}       {}", start, label);
        } else {
            println!("  {:>3}-{:<3}   {}", start, s, label);
        }
        s += 1;
    }
}

/// Resolve the duplicate index the way the signature checkers do, for every key slot.
/// This mirrors `hardened_get2(offset + i, offset + i - DISTANCE)` exactly.
fn report_dupe_pairs() -> usize {
    let mut failures = 0usize;
    println!("\n--- revocation duplicate resolution ---");
    for p in OWC_DUPE_PAIRS {
        for i in 0..PUBKEY_SLOTS {
            let primary = p.primary + i;
            if p.distance > primary {
                println!("  {:<12} key {}: UNDERFLOW ({} - {})", p.name, i, primary, p.distance);
                failures += 1;
                continue;
            }
            let resolved = primary - p.distance;
            let expected = p.dupe + i;
            if resolved != expected {
                println!(
                    "  {:<12} key {}: primary {} -> dupe {}, expected {}  <-- WRONG",
                    p.name, i, primary, resolved, expected
                );
                failures += 1;
            } else {
                println!("  {:<12} key {}: primary {:>3} -> dupe {:>3}  ok", p.name, i, primary, resolved);
            }
        }
    }
    failures
}

/// How many free slots sit above each revocation block before the next claim. Check this
/// before changing PUBKEY_SLOTS; several blocks currently have zero headroom.
fn report_headroom() {
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
        println!(
            "  {:<28} [{:>3}..={:<3}] headroom: {}{}",
            c.name,
            c.first,
            c.first + c.count - 1,
            headroom,
            flag
        );
    }
}
