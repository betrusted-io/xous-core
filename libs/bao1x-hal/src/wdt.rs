use utralib::*;

// This file mainly machine generated using Claude Sonnet 4.6 Adaptive, by
// feeding in the RTC driver as a template, the Crossbar WDG C driver as a reference,
// and the verilog source files for the IP block.
//
// So far, the only tested aspects are enabling and feeding the watchdog; the
// interrupt feature set is not yet tested.

/*
    This is the ARM "CMSDK APB Watchdog" IP (PrimeCell part number 0x824,
    designer 0x3B / ARM).

    The full APB address window is 4 KB (PADDR[11:2]), so a single
    xous::map_memory call of 0x1000 bytes covers every register including
    LOCK at offset 0xC00.

    Main register block (word indices 0–5, offsets 0x000–0x014)
    -----------------------------------------------------------
    LOAD        0x000   32-bit down-counter reload value.
                        Writing here also immediately reloads the running
                        counter (the hardware does a synchronised load on
                        the next WDOGCLK edge).

    VALUE       0x004   Current counter value (read-only).
                        Read-back of wdog_load is returned immediately after
                        a write before the first WDOGCLK edge, so the freshly
                        written value is always visible.

    CONTROL     0x008   [1] RESEN – assert WDOGRES (hardware reset) when the
                             counter expires while the interrupt is already
                             active (i.e. the dog was not fed after the first
                             timeout).
                        [0] INTEN – enable the counter AND the WDOGINT output.
                             The counter only decrements while this bit is set.
                             Clearing it freezes the counter; the reload value
                             is NOT lost.

    INTCLR      0x00C   Write 0x5A to feed the dog (reload counter + clear
                        the interrupt). Any other value is ignored by the
                        hardware.

    RAWINTSTAT  0x010   [0] RIS – raw (unmasked) interrupt flag. Set when the
                             counter reaches zero; cleared by writing INTCLR.

    MASKINTSTAT 0x014   [0] MIS – masked interrupt status (RIS & INTEN).
                             This is the signal that actually reaches the interrupt subsystem.

    Lock register (word index 768, offset 0xC00)
    --------------------------------------------
    LOCK        0xC00   Write LOCK_MAGIC (0x1ACCE551) to unlock all writable
                        registers. Any other write re-locks them. Reads back
                        the current lock state (0 = unlocked, 1 = locked).
                        Reset state is unlocked (wdog_lock = 0).

    Interrupt / reset behaviour summary
    ------------------------------------
    First timeout  → RAWINTSTAT[0] set, WDOGINT asserted (if INTEN=1).
    Feed dog       → write 0x5A to INTCLR; counter reloads, interrupt clears.
    Second timeout → WDOGRES asserted (if RESEN=1), counter freezes until
                     LOAD is written again.

    Typical "reset-only, no CPU interrupt" configuration
    ------------------------------------------------------
    Set CONTROL = 0b11 (RESEN|INTEN). Do NOT wire the interrupt.
    Feed the dog periodically by writing INTCLR. If the firmware hangs the
    counter will expire twice and assert the hardware reset line.
    This matches the usage in the C HAL (daric_hal_wdg.c).
*/

// -- Main registers ------------------------------------------------------------

/// Down-counter reload value.  Write to set; also reloads the running counter.
pub const LOAD: Register = Register::new(0, 0xFFFF_FFFF);

/// Current counter value (read-only).
pub const VALUE: Register = Register::new(1, 0xFFFF_FFFF);

/// Control register.
pub const CONTROL: Register = Register::new(2, 0x3);
/// CONTROL[0]: counter enable + interrupt output enable.
/// The counter only runs while this bit is set.
pub const CONTROL_INTEN: Field = Field::new(1, 0, CONTROL);
/// CONTROL[1]: hardware reset output enable.
pub const CONTROL_RESEN: Field = Field::new(1, 1, CONTROL);

/// Interrupt clear / dog-feed register.  Write [`INTCLR_FEED`] to feed.
pub const INTCLR: Register = Register::new(3, 0xFF);

/// Raw (unmasked) interrupt status.
pub const RAWINTSTAT: Register = Register::new(4, 0x1);
/// RAWINTSTAT[0]: set when counter reaches zero; cleared by feeding.
pub const RAWINTSTAT_RIS: Field = Field::new(1, 0, RAWINTSTAT);

/// Masked interrupt status (RAWINTSTAT & INTEN).
pub const MASKINTSTAT: Register = Register::new(5, 0x1);
/// MASKINTSTAT[0]: the interrupt signal seen by the hardware interrupt subsystem.
pub const MASKINTSTAT_MIS: Field = Field::new(1, 0, MASKINTSTAT);

// -- Lock register (offset 0xC00 = word index 768) ----------------------------

/// Software lock register.  Protects all writable registers.
/// Word index 768 = byte offset 0xC00, still within the 4 KB APB window.
pub const LOCK: Register = Register::new(768, 0xFFFF_FFFF);

// -- Constants -----------------------------------------------------------------

/// Write to [`LOCK`] to unlock all writable registers.
pub const LOCK_MAGIC: u32 = 0x1ACCE551;
/// Write to [`LOCK`] to re-lock all writable registers.
pub const LOCK_VALUE: u32 = 0x0000_0000;
/// Write to [`INTCLR`] to feed the dog and clear the interrupt.
pub const INTCLR_FEED: u32 = 0x5A;

/// Physical base address of the WDT peripheral.
pub const HW_WDT_BASE: usize = 0x40041000;
/// Number of 32-bit words needed to cover LOAD … LOCK (inclusive).
/// 769 = indices 0–5 (main regs) + index 768 (LOCK).
pub const WDT_NUMREGS: usize = 769;

// -- Op enum (for future message-passing integration) -------------------------

/// Operations that can be sent to a WDT service.
///
/// Variants 0–127 are for externally-visible APIs; 1024+ are for
/// implementation-private operations (mirroring the TimeOp convention).
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub enum WdgOp {
    /// Enable the watchdog.
    /// arg1: load value (ticks); arg2: enable_reset (bool); arg3: enable_irq (bool)
    Enable = 0,

    /// Disable the watchdog (freeze counter, clear CONTROL).
    Disable = 1,

    /// Feed the dog — reset the counter and clear any pending interrupt.
    Feed = 2,

    /// Returns the current counter value in arg1.
    GetValue = 3,

    /// Returns 1 in arg1 if the watchdog is currently running (CONTROL != 0).
    IsEnabled = 4,

    /// Returns raw interrupt status in arg1 and masked status in arg2.
    GetInterruptStatus = 5,

    // -- Implementation-private ops ----------------------------------------
    /// Enable the interrupt output (sets CONTROL[INTEN]; preserves RESEN).
    /// Does NOT connect the interrupt — use xous::claim_interrupt separately.
    UnmaskInterrupt = 1024,

    /// Disable the interrupt output (clears CONTROL[INTEN]; preserves RESEN).
    /// Leaves the counter running if RESEN is still set.
    MaskInterrupt = 1025,
}

// -- Driver -------------------------------------------------------------------

/// Single-owner driver for the ARM CMSDK APB Watchdog.
///
/// # Memory mapping
/// When compiled with the `std` feature (Xous userspace), `new()` calls
/// `xous::map_memory` to obtain a virtual mapping of the 4 KB APB window.
/// Without `std` (bare-metal / privileged context) it uses the physical
/// address directly.
///
/// # Lock protocol
/// Every write to a protected register must be bracketed by `unlock` /
/// `lock`.  Reads do not require unlocking.
///
/// # Typical "reset-only" setup
/// ```ignore
/// let mut wdt = Wdt::new();
/// // Feed interval (e.g. 500 ms) expressed in watchdog clock ticks.
/// // Use the WDG_TICKS_PER_MS macro / constant from the BSP.
/// wdt.enable(500 * TICKS_PER_MS, /*enable_reset=*/ true, /*enable_irq=*/ false);
///
/// loop {
///     do_work();
///     wdt.feed();
/// }
/// ```
pub struct Wdt {
    csr: CSR<u32>,
}

impl Wdt {
    /// Construct a new `Wdt` driver, mapping hardware memory as required.
    pub fn new() -> Self {
        #[cfg(feature = "std")]
        {
            let wdt_range = xous::map_memory(
                xous::MemoryAddress::new(HW_WDT_BASE),
                None,
                // 4 KB covers the full APB window including LOCK at 0xC00
                // and the ID registers at 0xFE0–0xFFC.
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map WDT range");
            Wdt { csr: CSR::new(wdt_range.as_mut_ptr() as *mut u32) }
        }
        #[cfg(not(feature = "std"))]
        {
            Wdt { csr: CSR::new(HW_WDT_BASE as *mut u32) }
        }
    }

    /// reveals the internal CSR address for the WDT. The recipient of this address must promise
    /// not to do any concurrent access with the donor; the primary purpose of this method is to
    /// provide a method for moving the CSR between threads
    pub unsafe fn to_raw(&self) -> usize { self.csr.base() as usize }

    /// create a WDT from an internal CSR address. The caller must guarantee that the donor
    /// of the address has released the address, and also that the address is in fact a valid
    /// mapping for the WDT.
    pub unsafe fn from_raw(mem: usize) -> Self { Wdt { csr: CSR::new(mem as *mut u32) } }

    // -- Lock helpers ---------------------------------------------------------

    #[inline]
    fn unlock(&mut self) { self.csr.wo(LOCK, LOCK_MAGIC); }

    #[inline]
    fn lock(&mut self) { self.csr.wo(LOCK, LOCK_VALUE); }

    // -- Public API -----------------------------------------------------------

    /// Enable the watchdog and start the counter.
    ///
    /// - `load_value`: initial (and reload) counter value in watchdog clock ticks.  The counter counts down
    ///   from this value. The clock runs at (pclk / 2)
    /// - `enable_reset`: assert the hardware reset output on the *second* consecutive timeout (i.e. when the
    ///   dog was not fed after the first expiry).  Set this `true` for a normal watchdog reset.
    /// - `enable_irq`: set CONTROL[INTEN], which both starts the counter *and* drives the WDOGINT output to
    ///   the interrupt system.  For reset-only usage (no CPU interrupt desired) pass `false` here — the
    ///   counter will still run because INTEN is always forced set internally; see note.
    ///
    /// # Note on INTEN and the counter
    /// The ARM CMSDK watchdog only decrements the counter while INTEN=1
    /// (CONTROL[0]).  To use the watchdog *purely* for reset (no interrupt
    /// servicing) you still need the counter running, so this function always
    /// sets INTEN regardless of whether the interrupt is registered or not
    /// with the rest of the system.
    pub fn enable(&mut self, load_value: u32, enable_reset: bool) {
        self.unlock();
        self.csr.wo(LOAD, load_value);
        // INTEN (bit 0) must always be set to start the counter.
        // RESEN (bit 1) gates the hardware reset output.
        let ctrl = 0b01 | if enable_reset { 0b10 } else { 0 };
        self.csr.wo(CONTROL, ctrl);
        self.lock();
    }

    /// Disable the watchdog.
    ///
    /// Clears CONTROL (freezes the counter and de-asserts outputs) and
    /// resets LOAD to the all-ones sentinel so that if the watchdog is
    /// inadvertently re-enabled it will take the maximum possible time
    /// before expiring.
    pub fn disable(&mut self) {
        self.unlock();
        self.csr.wo(CONTROL, 0x0);
        self.csr.wo(LOAD, 0xFFFF_FFFF);
        self.lock();
    }

    /// Feed the dog — reload the counter and clear any pending interrupt.
    ///
    /// Writes the magic byte `0x5A` to INTCLR.  The hardware ignores any
    /// other value, so there is no need to check the current state first.
    pub fn feed(&mut self) {
        self.unlock();
        self.csr.wo(INTCLR, INTCLR_FEED);
        self.lock();
    }

    /// Returns `true` if the watchdog counter is currently running
    /// (CONTROL != 0).
    pub fn is_enabled(&self) -> bool { self.csr.r(CONTROL) != 0 }

    /// Returns the current value of the down-counter.
    ///
    /// Immediately after writing [`enable`], this reflects the freshly loaded
    /// value before the first WDOGCLK edge.
    pub fn current_value(&self) -> u32 { self.csr.r(VALUE) }

    /// Returns the raw (unmasked) interrupt flag.
    ///
    /// Set when the counter reaches zero.  Cleared by [`feed`].
    /// This flag is independent of INTEN, so it is visible even when
    /// the counter-enable bit is cleared.
    pub fn raw_interrupt_pending(&self) -> bool { self.csr.rf(RAWINTSTAT_RIS) != 0 }

    /// Returns the masked interrupt status (RAWINTSTAT & INTEN).
    ///
    /// This is the signal that would be presented to the CPU if the
    /// interrupt line were connected via `xous::claim_interrupt`.
    pub fn masked_interrupt_pending(&self) -> bool { self.csr.rf(MASKINTSTAT_MIS) != 0 }

    /// Enable the WDOGINT interrupt output (set CONTROL[INTEN]).
    ///
    /// Preserves the current RESEN setting.  Does NOT connect the CPU —
    /// call `xous::claim_interrupt` separately when you are ready to handle
    /// the interrupt in software.
    pub fn unmask_interrupt(&mut self) {
        self.unlock();
        let ctrl = self.csr.r(CONTROL) | 0b01;
        self.csr.wo(CONTROL, ctrl);
        self.lock();
    }

    /// Disable the WDOGINT interrupt output (clear CONTROL[INTEN]).
    ///
    /// Preserves the current RESEN setting.  Note that clearing INTEN also
    /// freezes the counter — call [`unmask_interrupt`] or [`enable`] to
    /// resume it.
    pub fn mask_interrupt(&mut self) {
        self.unlock();
        let ctrl = self.csr.r(CONTROL) & !0b01;
        self.csr.wo(CONTROL, ctrl);
        self.lock();
    }
}
