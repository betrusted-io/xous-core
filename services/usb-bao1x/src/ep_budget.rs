//! Corigine unidirectional non-EP0 endpoint budget ledger.
//!
//! `CRG_EP_NUM` (8) limits composite class endpoints. Per-class asserts alone are
//! insufficient: each class can be ≤8 while their **sum** overflows. This ledger
//! tracks the **cumulative** total across every class reserved for a build.
//!
//! Const `CRG_EP_NUM` must match `bao1x_hal::usb::driver::CRG_EP_NUM`.

#![allow(dead_code)]

/// Must match `libs/bao1x-hal/src/usb/driver.rs` `CRG_EP_NUM`.
pub const CRG_EP_NUM: usize = 8;

const MAX_PARTS: usize = 8;

/// Fail if a **single** class claims more than the hardware budget (kept as a
/// per-class sanity check alongside cumulative verification).
pub fn assert_class_ep_budget(class: &str, claimed: usize) {
    assert!(
        claimed <= CRG_EP_NUM,
        "USB endpoint budget: class '{}' alone claims {} unidirectional EPs, Corigine CRG_EP_NUM={}",
        class,
        claimed,
        CRG_EP_NUM
    );
}

/// Cumulative endpoint budget across composite USB classes.
#[derive(Clone, Debug)]
pub struct EpBudgetLedger {
    label: &'static str,
    total: usize,
    len: usize,
    parts: [(&'static str, usize); MAX_PARTS],
}

impl EpBudgetLedger {
    pub fn new(label: &'static str) -> Self { Self { label, total: 0, len: 0, parts: [("", 0); MAX_PARTS] } }

    pub fn total(&self) -> usize { self.total }

    pub fn parts_snapshot(&self) -> [(&'static str, usize); MAX_PARTS] { self.parts }

    pub fn parts_len(&self) -> usize { self.len }

    fn push_part(&mut self, class: &'static str, n: usize) {
        assert!(self.len < MAX_PARTS, "EpBudgetLedger overflow: too many class parts");
        self.parts[self.len] = (class, n);
        self.len += 1;
        self.total = self.total.saturating_add(n);
    }

    fn assert_within_budget(&self) {
        assert!(
            self.total <= CRG_EP_NUM,
            "USB endpoint CUMULATIVE budget exceeded for '{}': running total {} unidirectional EPs > CRG_EP_NUM={} (parts={:?})",
            self.label,
            self.total,
            CRG_EP_NUM,
            &self.parts[..self.len]
        );
    }

    /// Account for a class that is about to call `alloc_*`, asserting the new
    /// **running total** still fits. Call **before** the allocating constructor.
    pub fn reserve_before_alloc(&mut self, class: &'static str, n: usize) {
        assert_class_ep_budget(class, n);
        self.push_part(class, n);
        self.assert_within_budget();
    }

    /// Final check after every class for this image has been reserved (and ideally
    /// constructed). Names all classes and the total vs `CRG_EP_NUM`.
    pub fn finalize(&self) { self.assert_within_budget(); }

    /// Compare planned cumulative total to a live occupancy count (e.g.
    /// `CorigineWrapper::allocated_non_ep0_count()` after constructions).
    pub fn assert_matches_live(&self, live_allocated: usize) {
        assert_eq!(
            self.total,
            live_allocated,
            "USB EP ledger total {} for '{}' does not match live allocated_non_ep0={} (parts={:?})",
            self.total,
            self.label,
            live_allocated,
            &self.parts[..self.len]
        );
        self.finalize();
    }
}

/// Old (broken-for-gap) semantics: each subtotal checked independently.
/// Used only in tests to prove the cumulative gap.
#[cfg(test)]
pub fn old_independent_subtotal_ok(subtotals: &[usize]) -> bool { subtotals.iter().all(|&n| n <= CRG_EP_NUM) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persona_a_ccid_fits_cumulative() {
        let mut ledger = EpBudgetLedger::new("baosec-ccid");
        ledger.reserve_before_alloc("CCID", 2);
        ledger.reserve_before_alloc("FIDO", 2);
        ledger.reserve_before_alloc("NKRO", 2);
        ledger.finalize();
        assert_eq!(ledger.total(), 6);
    }

    #[test]
    fn stock_baosec_fits_cumulative() {
        let mut ledger = EpBudgetLedger::new("baosec");
        ledger.reserve_before_alloc("FIDO", 2);
        ledger.reserve_before_alloc("NKRO", 2);
        ledger.reserve_before_alloc("debug CDC", 3);
        ledger.finalize();
        assert_eq!(ledger.total(), 7);
    }

    /// Regression for the cumulative gap: a fake extra class on a 6/8 stack.
    /// Independent per-class checks all pass; cumulative reserve must panic.
    #[test]
    fn fake_extra_class_caught_by_cumulative_not_by_independent() {
        // OLD: CCID(2), HID(4), FAKE(3) each ≤8 — would all "pass"
        assert!(old_independent_subtotal_ok(&[2, 4, 3]));

        let mut ledger = EpBudgetLedger::new("baosec-ccid + fake");
        ledger.reserve_before_alloc("CCID", 2);
        ledger.reserve_before_alloc("FIDO", 2);
        ledger.reserve_before_alloc("NKRO", 2);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut l = ledger.clone();
            l.reserve_before_alloc("FAKE_EXTRA", 3);
        }));
        assert!(
            r.is_err(),
            "cumulative ledger must fire when adding a class that pushes 6+3 over CRG_EP_NUM"
        );
    }

    #[test]
    fn live_mismatch_detected() {
        let mut ledger = EpBudgetLedger::new("mismatch");
        ledger.reserve_before_alloc("CCID", 2);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ledger.assert_matches_live(1);
        }));
        assert!(r.is_err());
    }
}
