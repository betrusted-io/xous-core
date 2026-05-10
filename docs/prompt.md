# Cursor prompt: implement CCID OpenPGP support in `services/usb-bao1x`

## Context

You are working in a clone of `betrusted-io/xous-core`, on a topic branch based on `dev`.
The goal is to implement CCID OpenPGP support and first-boot PIN provisioning in
`services/usb-bao1x`, as specified in issue #875
(`https://github.com/betrusted-io/xous-core/issues/875`).

The reference implementation lives in the Galdralag-firmware repository
(`https://github.com/Supermagnum/Galdralag-firmware`). All types, traits, and
function signatures you need are defined there. Treat it as read-only reference — do
not copy code verbatim, but use it to understand the API surface.

---

## Read these files first before writing anything

From this repository (`xous-core`):

- `services/usb-bao1x/Cargo.toml` — existing dependencies and features
- `services/usb-bao1x/src/main_hw.rs` — how `UsbBusAllocator` and existing classes are constructed
- `services/usb-bao1x/src/hw.rs` — `composite_handler`, `device.poll`, USB reset and unplug handling
- `services/usb-bao1x/src/api.rs` — existing opcodes
- `libs/bao1x-hal/src/usb/driver.rs` — `CorigineWrapper`, `QUIRK_SET_ADDRESS_BEFORE_STATUS = true`

From Galdralag-firmware (read via raw URL or local clone):

- `crates/baochip-openpgp/src/xous_impl.rs` — `open_or_provision_backend`, `write_provisioning_pins`, `ccid_pin_hashes_unprovisioned`, `HalError::NeedsProvisioning`, `load_or_provision_ccid_user_pin_bytes`, `load_or_provision_ccid_admin_pin_bytes`, `ccid_pins_dev_from_env`, `load_or_derive_ccid_master_key`, `master_key_dev_from_env`
- `crates/baochip-openpgp/src/lib.rs` — public API surface
- `crates/usb-personality/src/provisioning/mod.rs` — `ProvisioningClass`, `ProvisioningCommit`, protocol
- `crates/usb-personality/src/ccid/usb_class.rs` — `CcidClass::new` signature
- `crates/usb-personality/src/ccid/mod.rs` — `USB_VID_GALDRALAG`, `USB_PID_GALDRALAG_TOKEN`, string constants
- `crates/usb-personality/src/openpgp/dispatch.rs` — `OpenPgpCcidDispatcher::new`

If any signature does not match what is described below, say so before writing code.

---

## What to implement

Prefer new files over editing existing ones. Keep concerns separated.

### New file: `services/usb-bao1x/src/ccid.rs`

Contains the CCID initialisation logic under `#[cfg(feature = "ccid-openpgp")]`:

- Master key loading — `load_or_derive_ccid_master_key` in production,
  `master_key_dev_from_env` in `ccid-openpgp-dev` builds
- First-boot detection — call `open_or_provision_backend` with empty PIN slices;
  if `HalError::NeedsProvisioning` is returned, delegate to the provisioning loop
  in `provisioning.rs`, then retry
- PIN loading — `load_or_provision_ccid_user_pin_bytes` /
  `load_or_provision_ccid_admin_pin_bytes` in production,
  `ccid_pins_dev_from_env` in dev builds
- Final backend construction — `open_or_provision_backend` with loaded PINs,
  wrapped in `OpenPgpCcidDispatcher`, passed to `CcidClass::new`
- Returns the constructed `CcidClass` to the caller in `main_hw.rs`

### New file: `services/usb-bao1x/src/provisioning.rs`

Contains the first-boot provisioning poll loop under `#[cfg(feature = "ccid-openpgp")]`:

- Builds a temporary USB device with `ProvisioningClass` (CDC-ACM,
  product string "Galdralag Provisioning", VID/PID from `usb-personality::ccid`)
- Polls until `ProvisioningClass::take_commit` returns a `ProvisioningCommit`
- Calls `write_provisioning_pins` with the committed User PIN and Admin PIN
- Returns on success; propagates `HalError::Denied` if PIN length exceeds
  `CCID_PIN_PROVISION_PAYLOAD_MAX_BYTES` (32 bytes)
- Uses `xous::yield_slice()` when idle

### Minimal edits to existing files

`services/usb-bao1x/Cargo.toml`:

- Add optional path dependencies: `baochip-openpgp`, `usb-personality`
  (with `provisioning-personality` feature), `trng`
- Add features: `ccid-openpgp` (enables all three), `ccid-openpgp-dev`
  (enables `ccid-openpgp` and `baochip-openpgp/dev-provisioning`)
- Paths follow the layout where Galdralag-firmware lives alongside xous-core;
  adjust to the actual relative path in your checkout

`services/usb-bao1x/src/main_hw.rs`:

- Add a single `#[cfg(feature = "ccid-openpgp")]` block that calls into `ccid.rs`
  to obtain the constructed `CcidClass` and passes it into `Bao1xUsb::new`

`services/usb-bao1x/src/hw.rs`:

- Inside `composite_handler`, add `&mut ccid` to the `device.poll` call under
  `#[cfg(feature = "ccid-openpgp")]`
- On USB reset, call `ccid.reset()` under the same feature gate
- On unplug and `force_reset`, also call `ccid.reset()`

---

## Constraints

- Provisioning personality is only reachable when `ccid_pin_hashes_unprovisioned()`
  returns true; subsequent boots go directly to CCID
- `trng-pin-fallback` must not be enabled alongside `board-dabao` — enforced by
  a `compile_error!` in `baochip-openpgp`; do not work around it
- `ProvisioningCommit` staging buffers are `Zeroizing` types; do not copy PIN
  bytes into unprotected storage
- `QUIRK_SET_ADDRESS_BEFORE_STATUS = true` in `CorigineWrapper` — the CCID poll
  loop must tolerate `WouldBlock` on bulk endpoints until enumeration completes
- CCID OUT reassembly is handled by `push_out_bytes` in `CcidClass::endpoint_out`;
  do not buffer or fragment outside the class
- App buffers in `CorigineWrapper` are 512 bytes per endpoint direction
- All new files must be GPL-3.0-only (match the existing service licence)

---

## Code style

Before writing any code, read three or four existing source files in
`services/usb-bao1x/src/` and `libs/bao1x-hal/src/usb/` to establish the
formatting conventions used in this codebase. Match them exactly:

- Indentation width and style (spaces vs tabs)
- Brace placement
- `use` import ordering and grouping
- Line length limit
- Comment style (`//` vs `///`, placement)
- Feature gate placement (`#[cfg(...)]` above or inline)
- No trailing whitespace on any line

Do not apply `rustfmt` defaults if they conflict with the existing style.
Follow what the existing code does.

---

## Testing

After implementing the new files and minimal edits, run these commands in order
and fix all errors and warnings before finishing. Report the output of each command.

```
cargo check -p usb-bao1x --features board-dabao,ccid-openpgp
cargo check -p usb-bao1x --features board-dabao,ccid-openpgp-dev
cargo test -p usb-bao1x --features board-dabao,ccid-openpgp 2>/dev/null || true
cargo check --workspace --exclude xtask
```

If a test binary cannot be built for the embedded target, `cargo check` passing
is sufficient. Do not proceed to the commit step until all four commands complete
without errors or warnings.

---

## Commit requirements per CONTRIBUTING.md

Each commit must have:

- Subject line: imperative mood, 50 characters or fewer, no trailing period
- Blank second line
- Body: what and why, wrapped at 72 characters, references `Closes #875`
- DCO sign-off: `Signed-off-by:email `
- AI disclosure trailer: `Assisted-by: cursor`

The pull request must:

- Be based on the `dev` branch
- Have the `AI` label applied
- Reference `Closes #875` in the PR description
- Cover only the `ccid-openpgp` feature — one concern per PR
