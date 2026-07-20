# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Fork to Xous
## Changed
- Implement NIST.SP.800-230 initial public draft SLH-DSA for limited signature use cases (sign few, verify-many)
  - Hypetree is changed to a single layer. Signing is slow (one tall tree), verification is fast.
  - Winternitz parameter `w` is now 4 or 8, instead of fixed at 16.
  - Signature limit is 2**24. Publishing more than that number of signed binaries can lead to compromise of the private key.
- OIDs encoded are entirely fake. These need to be fixed once the NIST spec is ratified.
  - Changes in this fork are focused on *SHA2*. Shake changes are mainly side-effects where changes to types and structures that affect both have to be changed to facilitate compilation.
- Changes to utils.rs:
  - `base_2b` was a `u16`. This was OK because previously `a` would range from 6-14. It is now `u64` as `a` goes from 24-25 but `a+7` bits need to be stored and a 32-bit number would overflow.
  - `WotsParams` is generalized to handle `w`. The original spec always fixes it at 16, now, it can be either 4 or 8.
- Only `SLH-DSA-{SHA2,SHAKE}-128-24` is implemented by default. This is because `hybrid-array` currently only provides `U3856` types, and does not provide the `U7752`  / `U14944` types needed to implement the higher security modes. This is OK, because we only intend to use the -24 variant in this bootloader.

Note about `hybrid-array` for future use:

`hybrid_array::ArraySize` is implemented per concrete value by a macro. The
published `hybrid-array` 0.4 (with `extra-sizes`) **does** provide `U3856`, so
`SLH-DSA-{SHA2,SHAKE}-128-24` build and test against stock dependencies with no
changes. It does **not** provide `U7752` or `U14944`, so the L3/L5 sets are
feature-gated. To enable them you must build against a `hybrid-array` that adds
these two sizes (exactly as the FIPS-205 sig sizes such as `U7856` were added
upstream). In `hybrid-array/src/sizes.rs`, add to the `extra_sizes` module:

```rust
pub type U7752  = uint!(0 0 0 1 0 0 1 0 1 1 1 1 1);           // 7752
pub type U14944 = uint!(0 0 0 0 0 1 0 1 0 0 0 1 1 1 0 1 1);   // 14944
```

and the corresponding `impl_array_sizes! { 7752 => U7752, 14944 => U14944 }`
entries. Then build/test with `--features sp800-230-highsec`. (An orphan-rule
workaround inside `slh-dsa` is not possible: both `ArraySize` and `typenum::U<N>`
are foreign.)

## 0.1.0 (2024-08-18)
### Changed
- Implement changes from FIP 205 Initial Public Draft -> FIPS 205 Final ([#844])

### Fixed
- `no_std` support ([#845])
- Enable `derive` feature of `zerocopy` ([#847])

[#844]: https://github.com/RustCrypto/signatures/pull/844
[#845]: https://github.com/RustCrypto/signatures/pull/845
[#847]: https://github.com/RustCrypto/signatures/pull/847

## 0.0.3 (2025-05-10)
- Backport release with legacy `signature` v2 support

## 0.0.2 (2024-05-31) [YANKED]
- Initial release
