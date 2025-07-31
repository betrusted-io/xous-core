# Release 0.9 notes

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
- Xous now targets `riscv32-imac-unknown-xous-elf` instead of `riscv32-imac-unknown-none-elf`.
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
- `modals` server for a simple "Pure Rust" API for creating dialog boxes and getting user input. See the `tests.rs` file for some examples how to use the application calls.

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
  - A second problem was identified where the virtual page number was being stored as a fully expanded address in the page
  table, and not as a page number. Due to the compressed encoding of the page table entry, this means that the virtual address space would be shrunk by ~4000x. This is now fixed, so we have the full as-designed virtual memory space once again.
  - A migration routine was created to go from v1 -> v2 databases. It automatically detects the older version and attempts to guide the user through a migration. Although we don't have many users and databases today, this is a "best practice" for breaking revisions and this serves as a basis for forward-looking changes that are migrateable.
- Various fixes and improvements to the USB update scripts to improve reliability.
- Graphical panic outputs: when there is a panic, you get a "Guru Meditation" error box plus the panic message.
  - Currently all panics are hard crashes
  - Most of the time the system will reboot itself within a few seconds of displaying the panic
  - There will be occasions where you will need to insert a paperclip into the reset port on the lower right hand corner to recover from the panic.
  - If you get a panic, please snap a photo of it and drop it in a new issue in the `xous-core` repo, along with a description of what you were doing at the time.

## New in 0.9.9
This release requires a new SoC. It is highly recommended to first upload the SoC and install the update, and then
perform the Xous firmware upgrade. This requires running manual update commands, instead of the all-in-one updater script.

- `modals` text entry has been refactored to allow multi-field text entries with defaults! Thanks to gsora for PR #140.
- fix issue #141: bug fix in `log-server` where max-length buffers were not being printed + refactor of method to use `send` vs scalars
- PR #149: index support for modals. Lists can be submitted as a `Vec` now, with an array index returned as the selection result. Thanks to nworbnhoj for the PR.
- PR #150 & #153: QR codes can now display a separate QR code from the actual text in the box via a Some/None specifier. Thanks to nworbnhoj for the PR & refinements.
- PR #151: message forwarding standardized as part of the messaging API. See https://betrusted.io/xous-book/ch07-07-forwarding.html
- fixed locking bug in dlmalloc (stdlib version 1.60.0.7)
- update smoltcp to 0.8.1
- refactor wait threads in net crate - use statically allocated pool of waiters + better filtering of requests for less churn
- defer Drop on TcpStream until all written packets have been transmitted
- scheduler quantum is now a tuning parameter in xous definitions.rs (`BASE_QUANTA_MS`). it is now set to 10ms, down from 20ms.
- USB device core with keyboard HID emulation demo and FIDO2 HID support; fix issue #170
- Issue #162 and #159: fix bugs with condvar support. condvar IDs are now serial, so re-allocations are not a problem, and the routine to remove old ones from the notification table now looks at the correct sender ID.
- Add `ceil`, `floor`, and `trunc` (f32 and f64) variants to the built-ins list (this is a `std` lib update, in 1.61.0.2)
- Add CI test automation facilities - CI infra now drives actual hardware through `expect` scripts, instead of just doing simulation checks
- Vendor in `getrandom` so we can support a Xous API for the crate, allowing us access some of the more modern rustcrypto APIs. This is necessary for `randcore` 0.6 compatibility. `randcore` 0.5 APIs are retained by integrating them directly into the TRNG object.
- Update AES API level to 0.8, and cipher dependency to 0.4 (on rootkeys). This was necessary to get CBC support for AES, which is needed for FIDO2. This *should* have no user-facing impact.
- `vault` app: a user authentication management app is now ready for Alpha testing. It aims to provide FIDO2, TOTP, and stored password DB functions.
  - U2F/FIDO2 are a vendor-in of Google OpenSK's FIDO2 implementation. The code basically passes the CTAP2 test suite (https://github.com/google/CTAP2-test-tool) ("basically passes" in that the timeouts tests fail because they aren't automated to not "press the button" when timeouts are being tested).
  - U2F functions are best supported with UX flow. FIDO2 transactions still trying to figure out what the UX flow is even supposed to be:
    we're lacking actual FIDO2 applications to test against. Some prompts are still just stand-in text.
  - TOTP integration is thanks to blakesmiths's PoC TOTP code!
  - Password vault functionality in place
  - Test routine for adding dozens of TOTP/FIDO records and a hundred passwords added. System tested up to 300 passwords -- no crash observed, but query performance of the PDDB is slow (~8 seconds)
- Strip out gratuitous use of floating point -- this trims about 100k overall from the memory footprint.
- Autobacklight feature: selectable feature to automatically raise the backlight when keys are pressed. Thanks to gsora for the feature!
  - More importantly, this adds `crossbeam` into the kernel. This is a somewhat heavyweight item and is not recommended for use in core
    services, but by including it to drive the auto-backlight feature, we get regular code coverage of the condvar pathway.
- Fix PDDB FSCB generation problems
  - some issue were found in free space cache generation, these are now fixed
  - fuzzer added to push many more corner cases than before -- disk is filled to ~75% capacity with dozens of Bases,
    triggering roughly 4 FSCB ops, and then all Bases and keys are verified to be correct.
- Optimize GAM performance
  - context switches that revert to a prior app screen are restored from a stashed bitmap instead of a full redraw. This
    prevents old menus, contexts, and defacements from "piling up" when an app is blocked due to an exceptional condition
    being raised (such as the PDDB requesting a full basis unlock to generate the FSCB)
- PDDB support in libstd
  - Directories are implemented as dicts in the flat namespace
  - The `:` separator is used between path components
  - A path that begins with a `:` specifies a basis. That is, the path `example` specifies a dict in the Union basis, while the path `:.System:example` specifies a dict `example` in the basis `.System`.
  - A path that is only `:` specifies `all bases`, and is used to list the open bases
  - The path `::` specifies the "default basis", which is the most recently-opened one
  - Because the path delimiter is `:`, this cannot be used as a string in filenames
    - Note, however, that the underlying API allows for `:` in filenames, which will confuse the libstd API
  - It is possible for a "file" and a "directory" with the same name to exist in the same "directory". In this case, the entity will return `true` for both `.is_file()` and `.is_directory()`. In practice this doesn't matter, since there are different calls for working with files and directories.
  - When deleting a file, it is immediately deleted and all open file handles on the system are now invalid. This is in contrast to other platforms where open file handles still work, or where a file that is open cannot be deleted.
  - It is not yet possible to create or delete bases using this API.
  - Libstd support for PDDB is not possible with hosted mode. File operations will write to your local filesystem.
- BIP39 wordlist handling
  - Display of BIP39 words from a `[u8]`
  - Input of BIP39 words into a `[u8]`, with validation and dynamic lookup/autocomplete of words
- Improved kernel revisioning
  - A readable semantic version tag is appended to loader and kernel images
  - In addition to the current tag, a tag for "minimum xous version" to read a PDDB backup generated by the current Xous image is included.
- Backup and restore of the PDDB
  - PDDB can now be backed up via USB. An encryption key, coded in BIP-39 words, is displayed on the screen.
  - Likewise, the PDDB can be restored onto any device, so long as you have the encryption key.
  - Restoration of the PDDB to a brand new device will require a "rekey" step that migrates the PDDB to the device-specific DNA code.
  - Most deniability is lost in the event of a rekey migration (only about 10% of the disk free space is fake-rekeyed for performance reasons). If full deniability is desired, one can run `pddb churn` after the migration to fully re-cycle all the ciphertext in the PDDB, and regain full deniability (but it can take up to 10x longer to run than a simple rekey).
- Auto-update flow
  - Staged updates are now automatically applied, or at least the user is prompted to enter passwords and apply them.
  - Devices with no passwords prompt users to create them, at least the very first time.
- Clarifications to terminology
  - Various menus and items clarified to be easier to read/understand. In particular:
    - The term "FPGA key" is now "backup key"
    - The term "update password" is now "root password"
    - The term "root keys" (as in "init root keys") is now simply referred to as "passwords" (as in "setup passwords")
    - The new terms are less pedantic but hopefully more intuitive

## New in 0.9.10
- Host-side updates are now simplified.
  - There is just one script to run on all platforms: `precursorupdater`
  - The old factory_reset.[sh,ps1] and update_ci scripts are now deprecated, and will be permanently removed next release.
  - `precursorupdater` is [published to PyPi](https://pypi.org/project/precursorupdater/) with the help of neutralinsomniac to sort out the packaging issues via #204. Thank you!
- PDDB fixes:
  - root structure journal age bug fixed & pushed as hotfix to v0.9.9
  - periodic flush of FSCB SpaceUpdate records added to restore deniability
  - Shellchat commands for flush, sync, and write added to the pddb subcommand
  - `backalyzer` script added to analyze backups. Useful for debug & recovery of data.
  - Fixed aes-keywrap bug - OG library was faulty; swapped in one that passes NIST test vectors & added transparent migration.
- Net fixes (`std` testbench compliance):
  - added loopback interface by "faking it" over the COM as smoltcp does not yet have the feature (loops back packets and ARP packets)
  - `net test` command added when the nettest feature is turned on
  - get error codes into compliance with library expectations
  - add nonblocking feature
  - fix Peek
  - fix short reads -- no more discarding of excess data when the read buffer is shorter than the data in the buffer
  - fix TcpClose -- waits until the close handshake finishes before removing the socket
  - handle "unattended" closes -- this is when someone connects to a server and immediately disconnects, so the application layer never has time to issue a "formal" close request. Now it is automatically issued.
  - fix timeouts
- `ditherpunk` improvements (PR #207):
  - iterator form for PNG decoding (thanks to nworbnhoj for a ton of work to get that together)
  - memory usage is well-constrained now, and suitable for everyday use
  - primary limit to PNG decode speed is read speed over e.g. TCP
- Feature #211 (change unlock PIN) implemented
  - The menu item is in the PDDB Submenu->Change unlock PIN
  - The redundant boot PIN setting during first power on setup is also removed. Only the one you set when you init the PDDB matters.
- vault backup restore (PR #205):
  - backup and restore of TOTP + password records to a host device via USB implemented by gsora. Huge thanks, that was a massive effort! lots of refactoring of the internal data structures used by the vault, making them more idiomatic.
  - separate host-based tool currently located at `apps/vault/tools/vaultbackup-rs` must be run on the host side to perform the backup
  - data is stored on the host as JSON; users can format their own JSON records to import existing TOTP and password data to the device in a bulk command
  - BitWarden export format is also supported by `vaultbackup-rs`.
  - backup and restore can only be run with user approval, accessed via the `vault` context menu and then selecting `Enable host readout`. Note that in this mode, any host can read vault secrets; therefore, the mode locks out the UI and when it is active.
- highly experimental TLS support when the "tls" feature is enabled. Relies on a pure-Rust implementation of `ring` located in the `ring-xous` fork.
- performance monitoring framework added. see `tools/perflib` and `services/shellchat/net_cmd.rs` for examples of how to use it.
- HOTP support added to `vault`. HOTPs display in the TOTP window with the notation "HOTP" next to them, and they auto-increment on autotype. When creating new items in TOTP you are given the option to make an HOTP record, and you can convert between the two by editing the record and changing the bottom line from totp to hotp and vice-versa. The `timestep` field is re-used from the TOTP record to store the `count` for HOTP.
- `ball` app demo removed from default build
- WLAN submenu added, thanks to a huge effort by @gsora.
- fix/close various old issues (in particular, RTC interrupts stripped out and suspend lock fails now trigger a notification)
- move RTC resume handler to the secure/private server - hopefully resolves a susres failure case
- `xtask` cleanup
  - old/unused targets removed
  - code refactored to use builder patterns and to be more modular
  - better support for remote packages and pre-built artifacts in the build system
- Cleanup to facilitate more targets that Precursor:
  - `api` directory created.
    - `ticktimer`, `log-server`, `xous-names`, and `susres` split into api/implementation pairs
  - `xous-kernel`, `xous-ipc`, `xous-rs` updated, packaged, and pushed to crates.io; kernel has gdb-stub removed since it doesn't work and creates a crates.io incompatible dependency.
  - `utralib` refactored to be the domicile for platform-specific artifacts:
    - SVD files are now located here
    - `renode`, `hosted`, `precusor` targets now added
    - targets are selected by a feature flag passed through the build system
- Added a sanity check to the backup.py to make sure that the backup preparation was run: you must prepare backups every time before doing a backup. The exported key headers are erased after the backup is run, because you don't want the exported keys just laying around where anyone can read it out by plugging in a USB cable. Unfortunately, a user lost data because this check was missing :(
- Added crates.io package verification to `xtask`. Note: Cargo.toml files are munged by `cargo publish`, so it's difficult to prove equivalence of your source file and what you're sent from crates.io. Instead, there's now a check in CI to confirm that Cargo.lock did not change, which should change with a tampered Cargo.toml.
- Fixed cache coherence issue in FLASH. Writes to flash memory need to also invalidate D$ because they are out of band to the regular memory hierarchy.
- Add checksums to backups
  - every 1MiB block of the PDDB is checksummed with a SHA-512 hash, of which the first 128 bits are stored in a table
  - checksums are included in the backup header
  - `backalyzer.py`, `backup.py`, and `restore.py` now parse the header and if checksums are there, it will flag if there is a checksum error
  - backups now take about an extra minute to run, due to the inclusion of the extra checksums
  - backups are no longer abortable, because in order to do the checksums, we have to unmount the PDDB and put the system into a semi-shutdown state. A confirmation screen now gates backups to avoid users accidentally triggering backups with a fat-fingered menu selection.

## New in 0.9.11
- Various infrastructure fixes contributed by @eupn and @jeandudey
- "Lock device" feature added; PDDB unmount before reboots
- Successive failed PIN attempts will re-suspend the device if it is suspendable, or reboot if not
- Fix bug in device auto-shutdown; COM/LLIO method deprecated as susres method does the correct sequencing. This should help with some of the "insert paperclip" scenarios after updating SoC, hopefully.
- Updated VexRiscv core to the latest version. STATIC branch prediction enabled and slightly faster I$ gives a small performance bump. Also fixes a bug with cache flushing that was causing coherence problems with the PDDB.
- Fix tricky loader bug that was causing subtle issues with various build configurations
- Suppress main menu from popping up before the PDDB is mounted (resolves race conditions based on PDDB-stored keys)
- Optimize PDDB bulk key listing performance
- Add French language locale (thanks @tmarble!)
- Add `mtxcli` application, a basic Matrix chat interface (currently just https-secured, not E2EE). Thanks again @tmarble for the contribution!
- Several infrastructure changes/improvements to how utralib and crating works
- Add some UX cues on boot asking the user to wait for various operations.
- Fix context switching in GAM. Now, when relinquishing a context, the context is switched before the response is fired back to the caller. This means that it is much less likely that the caller will start drawing prematurely and have the draw ops missed.
- Rework main menu & preferences to use a dedicated "preferences" submenu (thanks @gsora!). This change will cause your system to prompt you to set the time again, because the location of the time zone record changes.
- Notes in `vault` are now editable without deleting the existing text
- `vault` password import from CSV using `vaultbackup-rs` (a host-based Rust program found in `apps/vault/tools`)
- eFuse burning now in Beta.
  - Burn an indelible backup key into your device without any third-party hardware
  - Loss of the backup key will brick your device permanently. Bugs in the burning process could also brick your device.
  - Testers need to build with `--feature efuse` in the command line. The process is not yet high confidence, so, proceed at your own risk!
  - Please contact bunnie if you plan to try this feature. Because it is permanent, and there is limited production due to supply chain issues, only minimal testing could be performed.
- Fixed GAM issue where canvases previously defaced would be re-defaced every time the canvas order is computed.
- Wifi signal is now rendered as bars, instead of as a number (thanks @gsora for PR#283!).

## New in 0.9.12
- Basis priority order is displayed in the status bar (issue #269). The left-most basis is the default basis. When no secret bases are open, no notification is displayed (the `.System` basis is assumed).
- Various bug fixes in `mtxcli`
- Fixed issue #109, where PDDB can panic after a memory cache prune due to missing keys.
- Reduced kernel code size by about 737kiB (10%) by restoring lto=`fat` and pushing FFT test code onto the tester. Note that any users who wish to write code that relies on built-in floating point transcendental functions will have to restore lto=`thin`, at least until https://github.com/rust-lang/rust/issues/105734 is resolved.
- `bip-utils` dependency removed from Python packages. This allows `backalyzer` and `precursorupdater` to run on older platforms that don't have the latest-greatest Python. A hand-rolled BIP-39 word-to-bits converter is used instead.
- More optimizations to `vault` passwords path. Records are re-used instead of re-allocated if they don't change. This should speedup switching to `vault` passwords by about 2x after the very first time the records are loaded (the first time will take longer because the records have to be built up).
- Extend watchdog reset time to ~30s from 7s, to enable easier guru meditation reporting.
- Add USB mass-storage drivers (thanks @gsora for all the help there!). Currently able to emulate a blank USB drive in RAM; more to come soon.
- Performance fixes to xous scalar and memory messages. A subtle bug was uncovered and fixed in the way scalar messages were being returned, and memory messages now initialize memory using an unrolled loop that takes better advantage of the 32-bit architecture and cache line size. A corresponding `std` fix to an unnecessary `yield_slice()` inside `dl_malloc` improves `vault` PDDB large-key readout performance by 30%.
- Find and fix some edge cases in key deletion.
  - Keys that were supposed to be deleted were being re-fetched from RAM cache, which leads to them re-appearing in the UX and when one attempts to re-delete the key it triggers a double-free error.
  - Small pool key packing was incorrectly using an old key offset when repacking keys, leading to data corruption/loss after deletion events
- `mtxcli` has a message filter and async message updates! (thanks @tmarble)
- OpenSK FIDO code upgraded to handle FIDO2.1, which means among other things Precursor now supports residential SSH keys (e.g. you can use it with ssh to log in, sign commits, etc.). Upgrading to the latest version will trigger a migration to the new database format. If there is a bug or compatibility issue, don't fear: the previous database is not affected, and you can downgrade to the previous version and continue using your original keys.
- `transientdisk` PoC demo app by @gsora: a RAM-based 1.44MiB USB disk that one can use to transfer sensitive materials (such as private keys) between computers. The disk is de-allocated by just backgrounding the app, and you can sleep well at night knowing RAM is completely erased on a reboot.
- Lots of kernel upgrades & fixes by @xobs:
  - emulation: the ticktimer is now correctly able to handle delays of more than 49 days
  - emulation: timer0 is now correctly modeled, and the system timer works correctly
  - kernel: thread selection is now massively improved, and should be faster
  - kernel: thread selection now correctly selects and parks threads
  - kernel: the main loop is now simpler, though there is more room for improvement
  - kernel: execution now immediately transfers to spawned threads
  - kernel: fixed a bug where a thread immediately exited and the parent joined it
  - kernel: when returning a message twice, the error DoubleFree is now returned instead of ProcessNotFound
  - kernel: all `scalar` return calls are now unified
  - kernel: servers get to use their full scheduled quantum
  - libxous: syscalls now use asm! rather than external object files
  - ticktimer: the condvar implementation has been completely overhauled
  - ticktimer: implemented FreeMutex and FreeCondition api calls
  - ticktimer: only respond to RecalculateSleep when sent internally
  - ticktimer: use new .pop_first() function on BTreeHeap
  - ticktimer: fix a potential panic in the interrupt handler
- Fixed some edge cases in the I2C driver, and improved sleep/resume stability.

## New in 0.9.13
- Loader refactor & optimization:
  - Better portability to different RAM sizes
  - Assembly stubs eliminated and absorbed into Rust files
  - Code modularized and re-organized
  - Phase 1 cleaned up and optimized
- VexRiscv core patch to D$ virtual memory flush bug
  - Hopefully resolves issue #321
- SoC yield bugs fixed
  - Work-around Vivado tool issue where >10% of compilations were failing on some units
  - Strip out unused logic to decongest the design
  - Requires an update to usb_update.py, precursorupdater to work with the new SoC as the CPU debug port is replaced with a simple reset-halt mechanism for preventing bus traffic during USB updates
  - Metastability harden I2C & TRNG
  - Handle I2C timeouts. The I2C block is sensitive to hardware-specific thresholds, and on some devices it can fail to come up cleanly on boot. This code recovers from that more gracefully.
  - Move I/O blocks into always-on domain to avoid clock stoppage during fire and forget I/O operation
- I2C fixes:
  - Ensure that the RTC does not interpret line noise during shutdown as garbage by having the very last command issued be a read to the RTC.
  - Shut down I2C block after this read happens by disabling it
  - Harden the RTC handler such that if junk corrupts the RTC it doesn't loop forever being confused about the junk data.
- More multi-platform support work
  - Preliminary Cramium SoC and FPGA targets incorporated
  - atsama5d27 target support via PRs from Foundation Devices (thank you!!). Xous is now booting on the ATSAMA5D27-SOM1-EK1 dev board!
- Fix edge case in phase 1 loader (thanks to @southpawflow for reporting it and providing the test case files)
- Implementations removed from crates.io (API crates still published) -- nobody is using the implementation crates it seems, and they are very hard to maintain.
- (EC) WF200 firmware bumped to 3.16.0 (will trigger an EC update). EC Litex design also brought into compliance with deprecated Litex APIs.
- Various improvements:
  - MAC address now added to wifi preferences status screen
  - All-0 app_id in U2F no longer prompts for a save record
  - PDDB shellchat writeall command via @pakl
- Add Force Update option for the EC. This required migrating the time server runtime into the DNS process space, due to exhaustion of connection IDs in the status process space.
- @xobs has added the feature `--gdb-stub` to the build. When selected, the kernel is built with GDB support over serial port. This works well in Renode. To try it in hardware, one must first run `console app` inside `shellchat` to activate the GDB UART (otherwise the sole serial port is connected to the console log).
- @xobs added graphical panic output to the kernel. This is different from a guru meditation in that it only happens when the kernel itself panics.
- Pulled in `ebreak` fix for VexRiscv CPU
- In `vault`, deleting a password and saving the record with the blank password triggers a password generation dialog box (useful for re-doing passwords)
- Add USB serial support
  - console logs can now be viewed via USB serial with the shellchat command `usb console` (`usb noconsole` to turn off). You will need a terminal client that is capable of CRLF translations.
  - TRNG can be set to emit raw binary data over USB serial with `usb trng` (`usb notrng` to turn off). This should be compatible with existing methods to extract randomness from USB dongles such as the OneRNG (looking for an existing HW RNG dongle user to test and confirm compatibility with their existing system!).
- "Lock device" now sleeps after reboot (thanks to patches by @gsora)
- Hosted mode now runs more smoothly, with less lag (thanks @yvt for the patch!)

## New in 0.9.14
- `gdb-stub` on hardware is now quite reliable, thanks to some performance improvements by @xobs. Just remember that single stepping does not work on XIP images (and by default now, apps are compiled as `xip`)
- "Notes" fields in `vault` app that start with the keyword 'bip39' will trigger a BIP39 password entry box (if the password field is blank), or BIP39 rendering of the hex data. Password generation prompt also uses the password 'bip39' to trigger the BIP39 generation sequence. It does mean that you can't use just 'bip39' as a regular password but...
- Add multi-line text editing to `vault` password fields, as well as left/right cursor movement to call up an insertion point that can be used to insert and delete characters in the middle of a field.
- Extra characters added to arrow keys and Fn keys from MacOS hosted mode cleaned up (thanks @wizzard0)
- Device will not try to suspend on lock if it is plugged in
- Autosleep timer option added. By default it is 0 (disabled). It is adjustable in increments of minutes. Note that if you try to lock the device while plugged in, and then unplug it without unlocking it, the system will stay "awake" because it can't consult the PDDB to know what the autosleep setting is.
- Autosleep can also optionally lock the device on sleep. However, by default it just sleeps.
- Add "lefty mode" option for `vault` (flips deny key from F1 to F4)
- Add "autotype delay" setting. The default is 30ms, but some computers can't handle key strokes that fast (password characters will be dropped). This allows users to tune this down, to a maximum of 500ms (at which point you may actually get multiple key entries depending on your hold-to-repeat delay). Probably a setting of 80ms should be fine to deal with deeply virtualized USB stacks running on slower computers.
- Fix #325, u2f authentication requests now sunset even when the host stops polling
- #388 via @eupn adds physical address resolution for a remote virtual memory process, allowing for coordination with remote DMA initiators. Does introduce some potential security problems, so gated behind the `v2p` flag.
- Fix #339 by adjusting the shellchat API call to match what is done in status bar
- Improved wifi scanning (fixes #336) - scans are now sorted by strength; old APs are retired; and the UX will pause while the scan occurs. Repeated scans still require going through the entire menu tree again; this is because modal radio-box lists aren't dynamically updateable.
- Improve performance of filtering operations in `vault` by ~100x by refactoring the item cache to work on `Vec` that is repeatedly sorted, instead of on a `BTreeMap` that is referenced. Turns out that sorting is a far cheaper operation than reference-counted shared references, or copying data to the heap (to avoid the shared reference). See PR#389 for details.
- @jeandudey contributed #390 and #391 which improve interoperability of svd2utra across build hosts and modularizes the language to the locales crate. Unused assembly code was also stripped out of xous-rs.
- @samblenny contributed a Tall font. NOTE: if you are doing kernel development, you *must* update the loader with this change, otherwise some fonts will not render correctly.
- The rendering subsystem has been updated to handle this and the default system font is now Tall, which should improve readability without sacrificing user prompts (i.e. dialog boxes should still fit on the screen). Please create issues for dialog boxes that are broken by this change. Also, a deliberate choice was made to leave selection lists in the original more dense font, as selection lists benefit from vertical density.
- A proper TLS library for incorporation into apps that require it has been created by @nhoj. You can find it in `libs/tls`.
- Issue #341 closed (support open wifi networks) - requires EC update.
- Rust 1.71.0 support - @xobs discovered that Rust 1.71.0 now shifts the address around of hardware management structures such that when we bind them to interrupt handlers, the final address of the interrupt handler is different from the address we see inside the `new()` function. It's actually allowed to do this. @xobs has created the "deferred-init + Box" pattern to harden against this. Deferred-init refers to binding the address of the hardware structure to the interrupt handler *after* `new()` has returned. This ensures that at least all the optimization that may happen inside `new()` are finalized. Additionally, wrapping the result of `new()` inside `Box()` ensures that the resulting structure is put on the heap. Because `Box` also implements `Pin`, the compiler won't muck with the address any further and should prevent future incompatibilities with Rust. The only place this pattern could not be applied is in the USB stack, because the USB crate we use expects to have an un-Box'd hardware management structure. However, deferred-init alone seems to be good enough, at least for now, to allow this to work.
- Fix (hopefully for real this time?) a bug in the "flush cache" instruction for Vex that affected specifically virtual memory configurations like ours. This will require a firmware update.
- More cramium SoC target integration and libraries; support multi-SVD targets in `utralib`
- Add menu option in `vault` to type usernames (in addition to passwords)
- Fix minor issue in precursorupdater where `--config` argument would not quit and run an update instead
- Fix issue in `ProcessStartup` where `repr(C)` was missing, thanks to @vihaanjim for finding that subtle bug!

## New in v0.9.15
- refresh item cache after leaving vault host readout mode
- Regressions have bee found in Rust 1.72. We suspect the llvm back-end for RV32 is emitting invalid code, see issue #416 and #417 for more details. For now, the optimization level has been set to `s` as a work-around.
- Network stack is updated to work with the `main` branch of `smoltcp`. This is not intended as a release configuration -- this note is here so that when a release is snapshotted out, we remember to pin to a particular commit (or ideally an official release, if one happens in time).
  - This fixes a number of long-standing issues, including #210 and #407.
  - Fairly major overhaul to the network stack. We now use mspc primitives to implement the wait/poll loop, which should make the net stack much more efficient and robust.
- mtxchat project kicked off by @nohj - see https://github.com/orgs/betrusted-io/projects/3/views/1 for project tracker
- mutex & condvar refactor in ticktimer thanks to @xobs - improves performance and stability
- App loader example code added thanks to @vihaanjim
- Add "busy spinner" primitive to text boxes to the GAM. This allows UIs to show that something is happening without having to explicitly implement that.
- sigchat skeleton added thanks to @nhoj
- usbd-human-interface-device bumped to latest in preparation for a fork
- Fix #446 - add support for numeric-only passwords in `vault`
- Fork usbd-human-interface-device to xous-usb-hid
- Harden EC against power glitches during updates (requires EC update - do not plug or unplug USB during an update until this patch is finished updating)
- Some housekeeping on the Xous wiki. Much more work is needed.
- Delay PDDB mount dialogs until all update checks are completed. This is done with a call named `is_ec_ready` and `set_ec_ready` in the `llio`. Nominally, it can be set/unset to indicate if the EC is going into an update state. Initially it is unset, and it is set by the status loop once all the update activities are completed. But it could be set later on by a routine that manually initiates EC updates (although there is no sanctioned way to do that at the moment).
- Fix validator to accept `tz_offset` == 0 as valid.
- Rust 1.74 rolled out. Xous now has much more native `std` support in mainline Rust, thanks to a huge effort by @xobs
  - Fix regressions in libstd `lend_impl` and PDDB `WriteKeyStd`
  - Fix bug in `Seek`
- Fix issue with `mpsc::channel` implementation that would lead to threads hanging under a race condition where `unpark()` is called in the middle of a `park_timeout()` call
- Cleanup areas where `net` could block on a hung `connection_manager`
- Stop running CI on `betrusted-soc` commits: there have been no changes to the design, all of the recent commits are fixing breaking changes in CI tooling. We should not force users to update their `soc` for a design that hasn't actually changed. Thus, users may note that the `soc` commit is not parallel with HEAD on `main`. CI & release staging will from now on be manually triggered only when an actual design change is made.
- `smoltcp` is pinned to a branch. A lot of significant fixes have occurred since their last release in June, and we'd like to get this baked into 0.9.15. It's unclear when they will tag out a release, so this is a temporary fix to allow us to move ahead. Will revert to an officially blessed release once it's available!
- fix #227 (make wlan status sends fail gracefully)
- big push by @xobs to make significant fixes to the `std` synchronization primitives (pushed in Rust 1.74.1.4, absorbed into this release):
  - fix bug in FFI memory free
  - improved soundness of `condvar`
  - refactor `rwlock` with more robust error handling
  - make `once` thread-safe
  - fix `thread_local_key`, `thread_parking`, `senres`
- work around codec/rtc conflict on I2C bus with a delay on codec init (defer full fix to resolution of #467 on future release)
- Fix an issue where uninitialized memory is being returned from the kernel due to a pointer assumed to be pointing to `usize` actually pointing to `u8`, thus having its length incorrectly divided by the size of a `usize`

## New in 0.9.16
- Incorporate more stability fixes and formatting changes from @xobs
- Clean up warning and breakages due to Rust 1.75.0. Note that cleaning up one warning forces the code to be
  backward-incompatible with earlier versions of Rust. 😖
- Fixed a long-dormant bug in a kernel asm.S that came to light with the clean-up pass of the kernel (see #474)
- A kernel patch was introduced by @xobs which ensures that every page is owned by only one process. As a result,
  it was discovered that XIP processes have been broken, because XIP processes map their code space into their own
  process space, but their code space was also mapped into `root-keys` so that it can inspect the code for signing.
  This has been fixed, see [#472](https://github.com/betrusted-io/xous-core/issues/472) for an extensive discussion,
  but the TL;DR is that the kernel is now signed using ed25519ph (the pre-hash version of the algorithm), and the
  hash is computed by the loader at boot time. This also closes a TOCTOU between when the loader verifies the
  filesystem signature before jumping to the kernel, at the expense of being more vulnerable to any breaks in SHA-512,
  or a runtime break in the `root-keys` process that gains an arbitrary write primitive and is able to remodel
  the stored hash value before it is signed.
- An update to `precursorupdater` is needed to recognize v2 kernel signatures (needs to be <= v0.1.5). Upgrade using
  `python3 -m pip install --upgrade precursorupdater`
- Environment variables are now a feature of programs. They are stashed just beyond the top of stack by the loader.
  They are mostly intended for test & debug tooling, where the loader is in the host environment, but this mechanism
  is also used to resolve the above issue.
- `sigchat` can now link up to an existing account - thanks to huge PR by @nhoj
- `xtask` can accept out-of-tree apps using a syntax like `cargo xtask app-image sigchat:path-to-elf-file` thanks to PR by @kotval. This allows apps like `sigchat` to have AGPL licensing without contaminating `xous-core`
- `sigchat` moved to its own repo with AGPL licensing: https://github.com/betrusted-io/sigchat
- Formatting and contribution standards have been modified. Formatting with `rustfmt` and trailing white space removal is now mandatory for all Xous contributions, see [#477](https://github.com/betrusted-io/xous-core/pull/477) for a discussion of how we got there and why.
- The repo has gone through a "flag day" where all the crates have been formatted, which means commits before the flag day may be more difficult to undo. The changes are committed on a crate-by-crate basis, so if something is really broken we can undo the formatting for the crate and add an exception to the rustfmt rules.
- Implement #478: backlight should turn on automatically when a U2F/FIDO packet comes in from the host, allowing users in dark conditions to see the screen and know what they are approving.
- the `sha2` API has been upgraded from 0.9.9 to 0.10.8. In the process of upgrading this, the `sha2` code is now domiciled in a fork of the `RustCrypto/hashes` repo. This should hopefully make tracking changes on RustCrypto somewhat easier, at the price of some difficulty in maintaining external crate pins (but I think that can be solved with some scripting). In the process of conversion, crates that depend on the 0.9.9 API for acceleration are now not accelerated.
- upgrade all other crypto APIs to latest version, with the exception of `p256` inside `vault` (this code is vendored from OpenSK), and the curve25519 implementations inside the loader (because they have been tightly optimized for size, we'll have to revisit the patch set later; but also, only verification steps are done in the loader so there is less risk of leaking secret key material - not as worried about ct_eq and zeroize bugs)
- a number of other crates and pins were upgraded in the process due to a cargo `update` run; the `build.rs` changes were reviewed and nothing nefarious was found, so at least this process did not introduce any obvious attacks against build hosts through supply chain contamination.
- @gsora has added the `hidapi` - apps can now register a HID descriptor for custom interactions over USB. See `apps/hidv2` for democumentation.
- change kernel and loader targets to riscv-unknown-elf-none because `xous` is now a proper target (required for Rust 1.76 compatibility)
- `curve25519-dalek` API is now at 4.1.2, thanks to @kotval for pulling it together. The new API removes `engine-25519` and rolls hardware allocate/release into the forked crate, similar to how sha2 was ported. `engine-25519` crate now removed from source tree, as it is now depracted since all the functionality was pulled into `curve25519-dalek`.
- keymap is checked on every call to send a key to the USB keyboard. This allows us to toggle the keymap temporarily to allow typing into hosts with a different keymap (instead of requiring a reboot)
- Encrypted swap: encrypted swap will allow Xous to run on microcontrollers that have small internal memory footprints, and rely on external SPI RAM for backing storage. This should create a step function in physical security when running on a microcontroller with sufficient internal protected RAM to hold the working set and core OS, as it will complicated attacks that attempt to read out off-chip RAM.

# New in 0.10.0
- Fix panic reporting in userspace panics. There was an API incompatibility between `std` and the panic handler where we instantiated the panic handler as a "well known service" but actually it needed to be registered with xous-names.
- Added "device RAM allocation". A region of memory requested using the `map_memory` API with a physical address of `None` and a flag of `xous::MemoryFlags::DEV` will be allocated as contiguous physical pages of memory. It returns `OutOfMemory` if a contiguous block cannot be found; it is up to the userspace to de-allocate or swap out memory to create a large enough block. This API is useful for creating regions of RAM to be passed on to e.g. DMA devices or hardware coprocessors.
- cleaned up swap API; removed elements that are no longer needed (e.g. SID/CID for userspace calls from kernel)
- Cramium target:
  - USB core able to enumerate, communicate to Linux devices. Windows compat still WIP.
  - Mailbox protocol to other devices has been tested, working.
  - TRNG has been tuned, partially validated.
  - BIO-BDMA test cases added
  - Full USB mass storage stack added in loader mode
  - Interactive USB updates via loader mode
  - Camera driver with live preview
  - QR code decoding (up to version 8)
- mini-gfx:
  - New graphics crate for small screen targets that does not require windows or borders. Drops the GAM, condenses APIs together for smaller memory footprint devices.
  - Very much a WIP
- Various fixes to track changes in Rust 1.80/1.81/1.82/1.83
- Add documentation to the `modals` library (thanks @rowr111)
- Due to a breaking change in Renode, this release is only compatible with Renode equal to or later than 1.15.2.7965（e6e79aad-202408180425) (see issue #570 / PR #572)
- Migrate to `rkyv` v0.8+.
  - This is a breaking change: `xous-ipc` is now at 0.10
  - Xous minor version incremented to 0.10.0 to note the breakage
  - All prior `chat-test` sessions need to have their PDDB keys manually deleted due to changes in serialization format
    - No other user-facing backward compatibility issues other than that noted above!
  - `xous-ipc::String<N>` API is now deprecated. We can now serialize `String` natively between crates (and `Vec` too)
    - Note that this pushes length-checking of `String` onto the caller: if the `String` is too large, you will get a run-time crash reported as a failure in `rancor-0.1.0` with a form like "created a new `Panic` from: overflowed buffer while writing ### bytes into buffer of length 0 (capacity is 4096)".
    - The resolution to this is to either ensure that all elements can fit within a single page of memory (if that's the desired behavior)
    - Or, check the total runtime size of the dynamic allocations and allocate more pages prior to sending the data
  - Number of pages to allocate could be automated inside the `xous-ipc` crate, but this will be delegated to a future time with a new API.
  - Most applications were forward-ported, except for `app-loader` which has already bit-rotted for other reasons and may be deprecated because we can use "swap" space to effectively do app loading (to be made available in future hardware revs)
  - Serialization is a bit easier now with the new `rkyv`, we don't have to track a `pos` explicitly; all of the archival metadata is now stuck at the end of the archive, so all you need to know is the final length of the serialized record and you're done.
- Refactor `blitstr2` to be in its own `libs` crate, allowing it to be re-used across multiple configurations
- Clean up the board vs soc abstraction. There are still places that don't adhere to this distinction, but:
  - A `soc` flag specifies dependencies that are generic to a system-on-chip (SoC). For example, the locations of registers, or the extents of memory regions contained in the `soc`.
  - A `board` flag specifies dependencies that are specific to a board. For example, the resolution of displays, pin mappings to peripherals, and sizes of external memory. Typically, a board assumes a `soc`, so both a `board-*` and `*-soc` set of flags are required to fully specify a build.
  - Precursor (the first target for Xous) did not hold to this abstraction and conflated the two, so it is a special case in the build system.
  - "Hosted" mode emulations are considered to be a `board` target; the `soc` is assumed to be the host (linux, windows, etc.)
- Since all builds require a `board` specifier, documentation now requires a `doc-deps` flag to be passed. This effectively specifies a set of dummy board dependencies so that the documentation can build. Here is the recommended command line for building docs: `cargo doc --no-deps --feature doc-deps`
- Move graphics api to `ux-api` crate. This enables multi-platform support for the existing graphics libraries.
  - graphics-server is deprecated as a dependency. Clients should now use `ux-api::minigfx` for drawing primitive dependencies.
  - This change now hides the GAM abstraction in most cases. The GAM is used to implement windowing; for lightweight platforms that don't have the screen real estate for windows, the `canvas`/`Gid` abstractions still exist but are ignored, and applications can draw directly to the screen.
  - `baosec` target in particular supports no windowing as the screen is 128x128: the UI is purely modal, where every user interaction occupies the entire screen.
  - Modify the core message loop to use reply_and_receive_next() API
- Improve message passing time by ~15% in tight scalar message benchmark
  - `activate_process_thread` now takes a `lazy_arg` which is evaluated after the thread is activated. When `lazy_arg` is set, it can save a context switch, as it allows pre-switch context setup to be deferred until the final context switch to activate the target process.
- Vexii SoC target support. This is primarily for testing the Vexii core for future implementations.

## Roadmap
- Lots of testing and bug fixes
- Fixing performance issues in `pddb`
- Refactoring `modals` to not have N^2 space growth with feature set
