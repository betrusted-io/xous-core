# Ball

A simple demo application that mainly demonstrates how to integrate into the Xous framework,
but without much emphasis on I/O.

## Integration

1. Add a UX context by editing `services/gam/src/lib.rs/EXPECTED_BOOT_CONTEXTS`
2. Copy this demo application, and rename the relevant structures in its `Cargo.toml` and `main.rs`.
3. Add it to the Workspace `default-members` and `members` arrays by editing `./Cargo.toml`
4. Add it to the build by editing `xtask/src/main.rs` and inserting it into the relevant descriptor. Typically, you would insert your app into the `hw_pkgs` array, as this is what is built and targeted for full hardware builds. Most of the other trimmed-down descriptors are for debug, emulation, and benchmarking.
5. (optional) You may also need to run `cargo xtask generate-locales` if you modify/add any internationalization strings.
