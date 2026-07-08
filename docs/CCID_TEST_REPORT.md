<!--
SPDX-License-Identifier: Apache-2.0
-->

# CCID OpenPGP test report

Date: 2026-05-10  
Toolchain: stable-x86_64-unknown-linux-gnu (default)  
Repository: betrusted-io/xous-core  
Branch: dev  
Reference crates / API: sibling firmware tree (see path deps in `libs/baochip-openpgp/Cargo.toml`)

Complete stdout/stderr for each cargo invocation was captured during this run
under `/tmp/ccid-test-out/` (`01-baochip-openpgp.txt` through
`06-workspace.txt`) and is not duplicated here.

## Results

| Command | Target | Result | Notes |
|---------|--------|--------|-------|
| `cargo check -p baochip-openpgp` | x86_64-unknown-linux-gnu (host) | PASS | Locales build-script warnings only |
| `cargo check -p usb-bao1x --features ccid-openpgp` | x86_64-unknown-linux-gnu (host) | PASS | Locales warnings only |
| `cargo check -p usb-bao1x --features ccid-openpgp-dev` | x86_64-unknown-linux-gnu (host) | PASS | Locales warnings only |
| `cargo check -p usb-bao1x --features board-dabao,ccid-openpgp --target riscv32imac-unknown-xous-elf` | riscv32imac-unknown-xous-elf | FAIL | First error: `error[E0463]: can't find crate for \`core\`` (target std not installed for this toolchain; use Xous-pinned toolchain or `-Z build-std`) |
| `cargo check -p usb-bao1x --features board-dabao,ccid-openpgp-dev --target riscv32imac-unknown-xous-elf` | riscv32imac-unknown-xous-elf | FAIL | Same first error as previous row |
| `cargo check --workspace --exclude xtask` | x86_64-unknown-linux-gnu (host) | FAIL | First error: `error: failed to run custom build command for \`hidapi v1.5.0\`` (`hidapi-hidraw` not found via pkg-config); environment/deps, not CCID code |

## Notes

- Host triple checks confirm new modules type-check and the OpenPGP / CCID
  dependency graph resolves correctly.
- board-dabao checks require the Xous-pinned toolchain used for normal
  Dabao image builds. Generic nightly + build-std failures on
  curve25519-dalek / utralib are environment limitations, not code errors.
- Here, stable did not provide `rust-std` for `riscv32imac-unknown-xous-elf`;
  the observed failure is missing `core`/target support, not curve25519.
  Full board-dabao verification must be run locally with the correct
  toolchain before merging.
- Workspace `cargo check` on this host failed early on `hidapi` system
  libraries; remaining `utralib` errors in the log follow from host-native
  crates and are unrelated to the CCID changes.

## Files changed

- `Cargo.toml` — Workspace member `libs/baochip-openpgp`; `[patch.crates-io]`
  comment for `subtle` (path deps vs patch).
- `Cargo.lock` — Dependency resolution updates for the OpenPGP / CCID graph.
- `libs/baochip-openpgp/Cargo.toml` — New shim crate: in-tree HAL/TRNG paths,
  `[lib]` points at the sibling firmware crate `src/lib.rs`.
- `services/usb-bao1x/Cargo.toml` — Optional `baochip-openpgp`, `usb-personality`,
  `trng`; features `ccid-openpgp`, `ccid-openpgp-dev`.
- `services/usb-bao1x/src/ccid.rs` — CCID stack: master key, provisioning
  branch, `CcidClass` construction.
- `services/usb-bao1x/src/provisioning.rs` — CDC provisioning IRQ loop and
  `ProvisioningCommit` wiring.
- `services/usb-bao1x/src/main.rs` — Feature-gated init calling `ccid` module.
- `services/usb-bao1x/src/hw.rs` — Composite poll includes CCID; reset and
  unplug paths call `ccid.reset()`.
