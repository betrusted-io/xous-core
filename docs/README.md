# Xous Documentation

This directory is about to be deprecated. Please refer to the [Xous Book](https://betrusted.io/xous-book/) for up to date documentation.

## Baochip USB CCID (`usb-bao1x`)

CCID transport documentation for this PR lives under [`docs/ccid/`](ccid/):

| Document | Contents |
|----------|----------|
| [ccid/CCID_PROTOCOL_AND_HIL.md](ccid/CCID_PROTOCOL_AND_HIL.md) | Protocol, IPC handler guide, security, Raspberry Pi HIL |
| [ccid/DRIVER_CHANGES.md](ccid/DRIVER_CHANGES.md) | Rationale for `bao1x-hal` USB `driver.rs` changes |
| [ccid/code_map.md](ccid/code_map.md) | Symptom-to-source navigation |
| [ccid/CCID_TEST_REPORT.md](ccid/CCID_TEST_REPORT.md) | Hardware verification status |
| [ccid/CCID_USB_ENUMERATION_DEBUG.md](ccid/CCID_USB_ENUMERATION_DEBUG.md) | Community enumeration deep-dive (not official support) |
| [ccid/OPENPGP_APDU_BOOT_DEBUG.md](ccid/OPENPGP_APDU_BOOT_DEBUG.md) | dabao-ccid vs openpgp-apdu boot discrimination |
| [ccid/CCID_EP_BUDGET_AND_HIL_LOCAL.md](ccid/CCID_EP_BUDGET_AND_HIL_LOCAL.md) | Local EP-budget / HIL notes |
| [ccid/CCID_FUNCTIONAL_MAP.md](ccid/CCID_FUNCTIONAL_MAP.md) | Functional map |

Host tests live under `tools/ccid/`. Build images with `cargo xtask dabao-ccid`, `baosec-ccid`, or `ccid-hil`.
