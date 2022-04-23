# Relase 0.9 notes

This release signals the stabilization of most Xous APIs; however, we reserve
the right to make breaking changes, but with some justification. You could call
this a "beta" release.

Notably, in 0.9, we have `libstd` integrated into Xous, and solidly intergated
into several servers; thus this release is not backward compatible with 0.8 (and
was never intended to be).

Aside from the introduction of `libstd`, few other breaking changes were implemented
in the APIs. Thus, we refer you to the [0.8 release notes](https://github.com/betrusted-io/xous-core/blob/main/RELEASE-v0.8.md#xous-08-messaging-api-in-practice)
for examples of idiomatic ways to write code for Xous.

## Major new features of 0.9
- Xous now targets `riscv32-imac-uknown-xous-elf` instead of `riscv32-imac-unknown-none-elf`.
  - `cargo xtask`, when run interactively, will offer to install the `xous` target.
  - Kernel debug primitives are accessible via the kernel console
  - GDB improvements
  - Improved `MemoryRange` API
  - Various stability improvements and major bug fixes (ELF loader bugs, stack pointer alignment, etc.)
  - Elimination of `heapless` crate
  - Transition from `master`->`main` for the default branch
- All hardware drivers are available in some form:
  - AES (using VexRiscV CPU extensions)
  - Audio Codec (low-level interface only, 8kHz rec/playback)
  - Curve25519 accelerator
  - SHA512 (based on Google's OpenTitan core)
  - JTAG self-introspection drivers
  - Real time Clock
  - FLASH memory writing
  - Suspend/Resume
  - Clock throttling when idle
  - TRNG upgraded with health monitoring and CSPRNG whitener
  - Upgrades to previous drivers (keyboard, graphics, etc.)
  - Wifi enhancements including SSID scanning and AP join commands
- Secured boot flow:
  - Signed gateware, loader and kernel
  - Compatible with BBRAM and eFuse keyed gateware images
  - Semi-automated BBRAM key burning
  - Self-provisioning of root keys (update and boot unlock keys)
  - Local re-encryption of gateware updates to a secret key known only to your device
  - Re-signing of loader/OS updates to protect code-at-rest
  - Detection of image "downgrade" attacks that use developer keys
  - Indication of downgrade through hardware-enforced hash marks on the status bar
- Enhanced graphical user interface primitives:
  - Modal dialog boxes
  - Menus
  - Progress bars
  - Text and password entry forms with input validators
  - Radio buttons
  - Check boxes
  - Notifications
  - Defacement of less secure items: random hashes on background areas; complicates phishing attacks that present fake UI elements
- Enforcement of delimited attack surfaces
  - `xous-names` can optionally enforce a fixed number of connections to a server
    - Once all the connections are occupied after boot, new connections are no longer possible
    - Developers must increment the connection count, specified inside the target server itself, when adding new resources that require server access
  - `gam` enforces a registry of all UX elements
    - Rogue processes cannot create new trusted UX resources
    - Developers must register their UX tokens in the `gam` inside `tokens.rs`
- Extended simulation support
  - Hosted mode performance improvements
  - Hosted mode now supports thread creation with `std::thread::spawn()`
- Improved emulation in Renode
  - Full emulation of ENGINE
  - Full emulation of SHA512 block
  - Initial support of emulation of EC
    - No COM-based inter-chip communication yet

## New in 0.9.5
- `modals` server for a simple "Pure Rust" API for creating dialog boxes and getting user input. See the `tests.rs` file for some examples how to use the applcation calls.

## New in 0.9.6
- Networking: DNS, UDP, Ping and TCP
  -  Basic demo of ping, rudimentary http get/serve
  -  EC offload of ARP and DHCP – thanks to samblenny for adding that, along with a solid refactor of the EC code base! The EC now also has the capability to act as a coarse packet filter for the core CPU.
- Connection manager
  -  Maintains a list of known SSID/password combos
  -  Manages passive SSID scanning and re-connection
  -  Enrolling in and saving a network profile is done via shellchat's wlan series of commands
  -  Only WPA2 is supported
  -  Still a lot of corner cases to work through
- PDDB: Plausibly Deniable DataBase for system config and user data storage.
  -  Uniform filesystem for all user-specific Xous data
  -  Fully encrypted database
  -  Plausibly deniable secret overlays
  -  See this blog post for more details
  -  Rust API bindings available, but as of now, few command line tools
- EC events – asynchronous callbacks triggered by network and battery events
- CPU load monitor in status bar
- Improved fonts
  -  B/W display optimized emoji
  -  Chinese-native Hanzi glyphs
  -  Japanese-native Kanji glyphs
  -  Hangul glyph set
  -  Thanks again to samblenny for all the hard work to make a slick new set of glyphs and a smooth new API!
- Japanese locale
- AZERTY and QWERTZ layouts
- Auto word-wrapping in the TextView objects
- Simplified graphical abstractions
  -  Modal server: create notifications, checkboxes, progress bars, radio boxes, and text entry boxes with validators using a single API call
  -  Simplified menu API
- Modular application programs
  -  manifest.json file to specify application integration parameters, such as the localized name and app launcher menu name
  -  Build-time command line selection of which apps you want baked into the Xous image
- Many bug fixes and improvements
- Improved libstd support
- Improved Renode and Hosted mode support

## New in 0.9.7
- `pddb` has salamanders fixed (https://eprint.iacr.org/2020/1456.pdf). This changes the root basis record storage, causing all prior versions to be unrecognized.
- `betrusted-soc` was updated to the latest Litex in prep for some work optimizing CPU performance and USB cores

## New in 0.9.8
- `TcpStream` is now part of `libstd`. Legacy TcpStream has been removed.
- `TcpListener` is now part of `libstd`. Legacy TcpListener has been removed.
- `net server` demo program has been upgraded to use multiple worker threads + MPSC for communications. Users can now attempt to access `buzz/` to cause the vibration motor to run, and there is now a 404 response for pages not found.
- `UdpSocket` is now part of `libstd`. DNS and all test routines switched over to `libstd`, and all prior scaffolding implementations have been removed.
- `NetPump` call added after Tx (UDP/TCP) and connect (TCP) events. This should improve the transmit latency.
- `Duration` and `Instant` are now part of `libstd`
- Timezone and time setting has been refactored. The HW RTC is now simply treated as a seconds counter since arbitrary time in the past; the BCD data that the hardware device tries to return is mapped into seconds since epoch. On first boot or invalid RTC detection, a random time is picked since epoch as the offset, between 1-10 years, to provide some deniability about how long the device has been used.
- One can now switch the local time by just updating the Timezone offset. The actual timezone offsets are not dynamically updated with DST; one has to explicitly program in the offset from UTC upon daylight savings.
- NTP option for time setting introduced with fallback to manual option
- CPU load meter has been shifting around in the `status` bar to accommodate worst-case proportional font layouts. Maybe we're there?
- Focus events refactored for the GAM to only send to apps (and not to menus and dialogs); it is strongly encouraged that all apps take advantage of them.
- Sleep screen is now blanked of all prior content and just the sentinel message is held
- Sleep/suspend lightly refactored to fix some bugs. Ticktimer is now the sole `Last` event.
- Preliminary text to speech (TTS) support added; compile with `cargo xtask tts` or set the LOCALE to `en-tts` to try it out.
- QR code rendering option added to `modals` (thank you nworbnhoj!)
- Introduce deferred-response pattern into modals, pddb, and susres.
- `pddb` critical bug fixed where page table entries were not having their journal numbers synchronized with the free space table when read off of the disk. This would cause inconsistency glitches in the `pddb`. This release fixes the problem, but, it may require you to reformat the `pddb` once the patch is in place. Because this is a genuine bug, if you're unlucky to be hit by this, there is no effective migration path. :(
- `pddb` major bug fixed where zero-length file allocations were not being committed to disk.
- `pddb` revision bumped to v2.1:
  - A first problem was identified where a key was being re-used between the ECB page table cipher and the AES-GCM-SIV data cipher
  - HKDF is now used to derive two separate keys for each
  - A second problem was identified where the virtual page number was being stored as a fully expanded address in the paget able, and not as a page number. Due to the compressed encoding of the page table entry, this means that the virtual address space would be shrunk by ~4000x. This is now fixed, so we have the full as-designed virtual memory space once again.
  - A migration routine was created to go from v1 -> v2 databases. It automatically detects the older version and attempts to guide the user through a migration. Although we don't have many users and databases today, this is a "best practice" for breaking revisions and this serves as a basis for forward-looking changes that are migrateable.
- Various fixes and improvements to the USB update scripts to improve reliability.
- Graphical panic outputs: when there is a panic, you get a "Guru Meditation" error box plus the panic message.
  - Currently all panics are hard crashes
  - Most of the time the system will reboot itself within a few seconds of displaying the panic
  - There will be occassions where you will need to insert a paperclip into the reset port on the lower right hand corner to recover from the panic.
  - If you get a panic, please snap a photo of it and drop it in a new issue in the `xous-core` repo, along with a description of what you were doing at the time.

## New in 0.9.9 (currently in development)
- `modals` text entry has been refactored to allow multi-field text entries with defaults! Thanks to gsora for PR #140.
- fix issue #141: bug fix in `log-server` where max-length buffers were not being printed + refactor of method to use `send` vs scalars

## Roadmap to 1.0

The items that are still missing before we can hit a 1.0 release include:
- [x] PDDB (plausibly deniable database): a key/value store that is the equivalent of a "filesystem" for Xous
- [x] Networking capabilities: a simple (think UDP-only) network stack
- Password changes in the `rootkey` context
- Post-boot loadable applications. We currently have modularized integrated applications, but no notion of a disk-loadable application. Still not sure if this is a feature we want, though, given that Xous is supposed to be a single-purpose tool and not a general OS.
- Further integration of drivers into `libstd`
- Maybe a functional USB device stack??
- Lots of testing and bug fixes
