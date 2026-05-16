//! Pure scheduling helpers for the `services/net` main pump loop.
//!
//! These helpers encode two scheduling decisions whose pre-fix behaviour caused
//! per-call read/write timeouts (set via libstd's `set_{read,write}_timeout`)
//! to silently miss their configured deadlines on quiet sockets. They are
//! lifted into pure functions specifically so the decisions can be exercised
//! by `cargo test -p net-scheduling` without spinning up the full
//! Xous IPC + smoltcp + modals stack that `services/net` otherwise drags in.
//!
//! Refs: tunnell/xous-core PR #26; xas#16, xas#22.
//!
//! # The two decisions
//!
//! 1. **Pump deadline cap.** The main loop in `services/net/src/main.rs`
//!    computes how long to sleep on `core_rx.recv_timeout(...)` before
//!    issuing an internal `NetPump`. Pre-fix, that interval was driven
//!    entirely by smoltcp's `iface.poll_at(...)` (capped at
//!    `NET_DEFAULT_POLL_MS`). On a quiet established TCP socket
//!    `poll_at` can push the next wake far into the future — but the
//!    per-call user expiries stored in `tcp_{rx,tx,peek}_waiting` and
//!    `udp_rx_waiting` are not visible to smoltcp, so the next wake
//!    can overshoot the configured timeout by tens of seconds.
//!    [`next_wake_deadline_ms`] caps the smoltcp value by the soonest
//!    pending user expiry.
//!
//! 2. **Reaper gating.** The `Opcode::NetPump` arm pre-fix had a
//!    `if !iface.poll(...) { continue; }` short-circuit. On a quiet
//!    socket `iface.poll` returns `false`, so the per-call expiry
//!    reapers (which only live in this arm) never ran. The reapers
//!    are what produce the `NetError::TimedOut` for libstd reads/writes.
//!    [`should_run_reapers`] codifies the post-fix invariant that the
//!    reapers run unconditionally on every NetPump tick.

#![cfg_attr(target_os = "none", no_std)]

/// Decide how long the main net loop should sleep before issuing the
/// next internal NetPump.
///
/// Returns the duration in milliseconds. Caller passes this to
/// `core_rx.recv_timeout(Duration::from_millis(...))`.
///
/// # Arguments
///
/// * `smoltcp_poll_at_ms` — absolute timestamp (ms since boot) returned by
///   `iface.poll_at(timestamp, &sockets)`. `None` means smoltcp has no
///   pending work it cares about.
/// * `pending_expiries_ms` — iterator of absolute expiry timestamps
///   (ms since boot) for every `WaitingSocket` and `UdpStdState` slot
///   that currently carries a user-supplied per-call timeout. Slots
///   without an expiry should be filtered out by the caller.
/// * `now_ms` — current monotonic clock value (ms since boot).
/// * `default_poll_ms` — fallback when smoltcp has no opinion. Matches
///   `NET_DEFAULT_POLL_MS` in `services/net/src/main.rs` (900ms).
///
/// # Invariants exercised by the tests
///
/// - If `pending_expiries_ms` contains a value that is sooner than
///   smoltcp's `poll_at`, the deadline is capped to that expiry
///   (relative to `now_ms`).
/// - If any expiry is at or before `now_ms`, the deadline is 0
///   (wake immediately so the reaper can fire).
/// - With no pending expiries, behaviour matches the pre-fix code:
///   the smoltcp `poll_at` value, or `default_poll_ms` if `None`.
pub fn next_wake_deadline_ms<I: IntoIterator<Item = u64>>(
    smoltcp_poll_at_ms: Option<u64>,
    pending_expiries_ms: I,
    now_ms: u64,
    default_poll_ms: u64,
) -> u64 {
    let mut deadline = match smoltcp_poll_at_ms {
        Some(poll_at) if poll_at > now_ms => poll_at - now_ms,
        _ => default_poll_ms,
    };
    for expiry_ms in pending_expiries_ms {
        let until_expiry = if expiry_ms > now_ms { expiry_ms - now_ms } else { 0 };
        if until_expiry < deadline {
            deadline = until_expiry;
        }
    }
    deadline
}

/// Decide whether the per-call expiry reapers in the `Opcode::NetPump`
/// arm should run this tick.
///
/// Post-fix: always `true`. The reapers must run every time `NetPump`
/// fires, regardless of whether smoltcp's `iface.poll` reported any
/// socket-readiness change, because per-call timeouts encoded by
/// `set_{read,write}_timeout` are tracked outside of smoltcp's state
/// machine.
///
/// Pre-fix this returned `smoltcp_changed`, which caused the bug
/// described in the module doc: on a quiet socket smoltcp returns
/// `false` and the reapers were skipped indefinitely.
///
/// The `has_pending_expiries` parameter is accepted but deliberately
/// unused — it documents the invariant that even when there are no
/// pending expiries the reapers are cheap (just a Vec walk) and should
/// run unconditionally. A future caller may want to bypass the call
/// site entirely when the queues are empty, but the gating decision
/// itself does not consult them.
#[inline]
pub fn should_run_reapers(_smoltcp_changed: bool, _has_pending_expiries: bool) -> bool {
    // Post-fix invariant: always run the reapers on every NetPump tick.
    // See module doc for the bug history.
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================
    // Tests for `next_wake_deadline_ms`.
    //
    // Each test calls out which pre-fix branch it would have triggered,
    // so a future reader can map the assertion back to the bug.
    // ============================================================

    #[test]
    fn no_pending_expiries_uses_smoltcp_poll_at() {
        // Pre-fix and post-fix agree here: smoltcp's value drives the deadline
        // when there are no user expiries to cap by. This is the baseline.
        let deadline = next_wake_deadline_ms(
            Some(1_500),
            core::iter::empty::<u64>(),
            1_000,
            900,
        );
        assert_eq!(deadline, 500, "no expiries: should follow smoltcp poll_at - now");
    }

    #[test]
    fn no_smoltcp_value_falls_back_to_default() {
        let deadline = next_wake_deadline_ms(
            None,
            core::iter::empty::<u64>(),
            1_000,
            900,
        );
        assert_eq!(deadline, 900, "no smoltcp value: fall back to default poll interval");
    }

    #[test]
    fn smoltcp_poll_at_in_the_past_falls_back_to_default() {
        let deadline = next_wake_deadline_ms(
            Some(500), // already in the past
            core::iter::empty::<u64>(),
            1_000,
            900,
        );
        assert_eq!(deadline, 900, "stale smoltcp value: fall back to default poll interval");
    }

    // ----- Bug-regression tests: pending expiry must cap the deadline. ----

    #[test]
    fn pending_expiry_caps_far_off_smoltcp_value() {
        // This is the core bug. smoltcp wants to wait 5 seconds (e.g. a
        // half-closed peer with nothing in flight). The user set a 500ms
        // read_timeout on a quiet socket. Pre-fix returned 5000; the
        // reaper then didn't fire for ~5s and the read_exact eventually
        // returned WouldBlock late. Post-fix caps at the user expiry.
        let now = 1_000;
        let deadline = next_wake_deadline_ms(
            Some(6_000),       // smoltcp wants 5000ms
            [1_500u64],        // user expiry 500ms from now
            now,
            900,
        );
        assert_eq!(
            deadline, 500,
            "pending user expiry must cap deadline below smoltcp's poll_at \
             (pre-fix this returned smoltcp value, missing the timeout). \
             Refs PR #26, xas#16, xas#22."
        );
    }

    #[test]
    fn pending_expiry_caps_below_default() {
        // Same shape but with no smoltcp opinion — default would have been
        // 900ms but the user wants 200.
        let now = 1_000;
        let deadline = next_wake_deadline_ms(
            None,
            [1_200u64],
            now,
            900,
        );
        assert_eq!(deadline, 200, "user expiry must cap below default poll interval");
    }

    #[test]
    fn expired_pending_expiry_wakes_immediately() {
        // An expiry that's already past must produce a 0-deadline so the
        // reaper can fire on the next iteration. If we returned the
        // smoltcp value here (or, worse, did saturating-sub the other way)
        // the user would never get their timeout.
        let now = 5_000;
        let deadline = next_wake_deadline_ms(
            Some(10_000),
            [3_000u64], // already expired
            now,
            900,
        );
        assert_eq!(deadline, 0, "already-expired user expiry must produce a 0-ms wake");
    }

    #[test]
    fn soonest_among_many_expiries_wins() {
        // The real call site chains tcp_{rx,tx,peek}_waiting and
        // udp_rx_waiting. The helper must walk the whole iterator and
        // pick the minimum, not just the first.
        let now = 1_000;
        let deadline = next_wake_deadline_ms(
            Some(20_000),
            [5_000u64, 1_200u64, 3_000u64], // 200ms wins
            now,
            900,
        );
        assert_eq!(deadline, 200, "must pick the soonest expiry across the whole iterator");
    }

    #[test]
    fn smoltcp_value_wins_when_sooner_than_all_expiries() {
        // Sanity: the cap is one-sided. We only shrink the deadline.
        let now = 1_000;
        let deadline = next_wake_deadline_ms(
            Some(1_100),       // 100ms from now
            [1_500u64, 2_000u64],
            now,
            900,
        );
        assert_eq!(deadline, 100, "smoltcp's sooner deadline must win over later user expiries");
    }

    // ============================================================
    // Tests for `should_run_reapers`.
    // ============================================================

    #[test]
    fn reapers_run_even_when_smoltcp_unchanged() {
        // Pre-fix this was `if !iface.poll(...) { continue; }`, i.e. the
        // gate was `smoltcp_changed`. On a quiet socket smoltcp returns
        // false; pre-fix the reaper was skipped and the read_exact hung
        // until the TCP retransmit budget exhausted (~89s on PVT2 wlan).
        // Post-fix the reapers always run. Refs PR #26.
        assert!(
            should_run_reapers(false, true),
            "reapers must run on every NetPump tick even when smoltcp reports no change \
             (pre-fix this was gated on smoltcp_changed and the bug manifested). \
             Refs PR #26, xas#16, xas#22."
        );
    }

    #[test]
    fn reapers_run_when_smoltcp_changed() {
        assert!(should_run_reapers(true, true));
    }

    #[test]
    fn reapers_run_even_with_no_pending_expiries() {
        // Defensive: the helper does not pre-filter on queue contents.
        // The actual reaper loops are themselves no-ops on empty queues,
        // and they're cheap; we keep the gating decision simple.
        assert!(should_run_reapers(false, false));
        assert!(should_run_reapers(true, false));
    }
}
