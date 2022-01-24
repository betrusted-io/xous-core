# REPL

A simple demo application that provides a bare-bones REPL framework.

## Integration

1. Add a UX context by editing `services/gam/src/lib.rs/EXPECTED_BOOT_CONTEXTS`
2. Copy this demo application, and rename the relevant structures in its `Cargo.toml` and `main.rs`.
3. Add it to the Workspace `default-members` and `members` arrays by editing `./Cargo.toml`
4. Add it to the build by editing `xtask/src/main.rs` and inserting it into the relevant descriptor. Typically, you would insert your app into the `hw_pkgs` array, as this is what is built and targeted for full hardware builds. Most of the other trimmed-down descriptors are for debug, emulation, and benchmarking.
5. (optional) You may also need to run `cargo xtask generate-locales` if you modify/add any internationalization strings.
6. Add entries to the app switching menu. `services/status/src/appmenu.rs` to add the menu item (plus `locales/i18n.json` in the status directory if you want translated names for the app), and `services/status/src/main.rs` to add the Opcode (inside the `StatusOpcode` struct, around line 55) and the actual operation itself (in the main `loop`, around line 670).