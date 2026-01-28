# Guix Quickstart for Baochip Firmware

## Prerequisites

[Install Guix][install guix] and configure channels in `~/.config/guix/channels.scm`:

```scheme
(cons*
 (channel
  (name 'rustup)
  (url "https://github.com/sbellem/guix-rustup")
  (branch "dev")
  (introduction
   (make-channel-introduction
    "d9bcf7f979506b880a5ba25674a606a824d9c890"
    (openpgp-fingerprint
     "E39D 2B3D 0564 BA43 7BD9  2756 C38A E0EC CAB7 D5C8"))))
 (channel
  (name 'rust-xous)
  (url "https://github.com/sbellem/rust-xous-guix")
  (branch "main")
  (introduction
   (make-channel-introduction
    "bcdb7bb2b220288545114b140f5079ba4f98a157"
    (openpgp-fingerprint
     "E39D 2B3D 0564 BA43 7BD9  2756 C38A E0EC CAB7 D5C8"))))
 %default-channels)
```

Then run `guix pull` to fetch the channels.

## Development Shell

Enter a shell with build dependencies:

```bash
guix shell --pure --development --file=guix.scm
```

Then build something, e.g.

```bash
cargo xtask dabao --no-verify
```

## Building Packages

From the xous-core directory:

```bash
# Build a specific target
guix build bao1x-boot0

# Build with output symlink
guix build bao1x-boot1 --root=target/guix
```

## Available Targets

| Package |
|---------|
| `bao1x-boot0` |
| `bao1x-boot1` |
| `bao1x-alt-boot1` |
| `bao1x-baremetal-dabao` |
| `dabao-helloworld` |
| `baosec` |
| `bootloader` |


[install guix]: https://guix.gnu.org/manual/1.5.0/en/html_node/Installation.html
