# `bao1x-hal` USB `driver.rs` changes (CCID PR)

This note is for reviewers of the Corigine USB driver diff in
`libs/bao1x-hal/src/usb/driver.rs`. Most of the line count is optional
telemetry; the behavior changes that affect every USB client are smaller and
listed first.

## Default builds (no `irq-pending-trace`)

These changes always apply:

### 1. PEI-indexed application buffers

`app_enq_index` / `app_deq_index` are indexed by PEI (endpoint number +
direction), not by endpoint number alone. IN and OUT of the same endpoint
number previously shared one ring index pair, which could alias buffers when
both directions were active (CCID bulk IN + bulk OUT). Each direction now has
its own slot bookkeeping.

### 2. Full buffer returns `WouldBlock` (no wrap/overwrite)

The old path could wrap or ignore the dequeue pointer when a new transfer did
not fit, which risked overwriting a buffer still owned by DMA. Allocation now
treats `enq + mps > CRG_UDC_APP_BUF_LEN` as full and returns `None`, so
`UsbBus::write` surfaces `WouldBlock` instead of clobbering in-flight data.
This also matters for high-speed 512-byte CCID slots where the old wrap
predicate rejected even a valid first slot.

### 3. `disable_interrupts` / restore masks `EV_ENABLE` only

`disable_interrupts` saves the previous `EV_ENABLE`, writes zero to mask, and
returns the saved value for a matching restore. It does **not** clear
`EV_PENDING`. An IRQ that latches while masked stays pending and runs after
restore (same contract as IRQ-safe serial writes in `usb-bao1x`).

### 4. Shared `allocated_non_ep0` counter

`CorigineWrapper` clones (including the `UsbBusAllocator`-owned clone) share an
`Arc<AtomicUsize>` counting non-EP0 endpoint allocations. Callers such as the
CCID EP-budget ledger can observe composite layout after `UsbDeviceBuilder`
runs.

### 5. No synthetic bulk-OUT retire

An earlier `force_prime_bulk_out` path that retired outstanding OUT slots
without a hardware completion was removed. Re-arm must not pretend DMA finished;
that pattern was associated with host-visible bulk errors. Priming after address
assignment still arms OUT when the ring is idle, without fake retire.

## With `irq-pending-trace` (observation only)

When the feature is **off**, the telemetry symbols compile to no-ops / zero
snapshots and do not change control flow.

When **on**, the driver adds lock-free counters and optional EP0 vendor
control-IN responses (`bRequest` `0x42` / `0x43`) plus a small flight ring for
host-side polls. These paths only read/update atomics and must not take the
Corigine `hw` mutex from EP0. They exist to gather lost-wakeup / TRB lifecycle
evidence on hardware; they are not required for CCID transport.

## Provisioning / PDDB (out of scope)

PDDB-backed CCID provisioning was removed from this PR: dabao has no SPI gen2
PDDB path, and that code was untested. A future, hardware-tested design could
use an application provisioning hook/callback instead of pulling PDDB into
`usb-bao1x`; that abstraction is intentionally not implemented here.
