# Xous Documentation

Kernel architecture and syscalls are covered in the [Xous Book](https://betrusted.io/xous-book/).
Older notes in this directory (`arguments.md`, `memory.md`, `processes.md`,
`startup.md`, `syscalls.md`, `flash.md`) are historical; prefer the Book.

## Baochip USB CCID (`usb-bao1x`)

Current references for USB CCID transport (`ccid-openpgp`):

| Document | Contents |
|----------|----------|
| [CCID_PROTOCOL_AND_HIL.md](CCID_PROTOCOL_AND_HIL.md) | Protocol, IPC handler guide, security, Raspberry Pi HIL |
| [code_map.md](code_map.md) | Symptom-to-source navigation |
| [CCID_TEST_REPORT.md](CCID_TEST_REPORT.md) | Hardware verification status |
| [CCID_USB_ENUMERATION_DEBUG.md](CCID_USB_ENUMERATION_DEBUG.md) | Community enumeration deep-dive (not official support) |

Host tests live under `tools/ccid_hil/` and `tools/ccid_smoke.py`.
Build images with `cargo xtask dabao-ccid`, `baosec-ccid`, or `ccid-hil`.
