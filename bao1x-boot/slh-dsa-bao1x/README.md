# [RustCrypto]: SLH-DSA

[![crate][crate-image]][crate-link]
[![Docs][docs-image]][docs-link]
[![Build Status][build-image]][build-link]
![Apache2/MIT licensed][license-image]
![MSRV][rustc-image]
[![Project Chat][chat-image]][chat-link]

Pure Rust implementation of the SLH-DSA (aka SPHINCS+) signature scheme.

Implemented based on the [FIPS-205 Standard].

## ⚠️ Security Warning

The implementation contained in this crate has never been independently audited!

USE AT YOUR OWN RISK!

## License

All crates licensed under either of

* [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0)
* [MIT license](https://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

[crate-image]: https://img.shields.io/crates/v/slh-dsa?logo=rust
[crate-link]: https://crates.io/crates/slh-dsa
[docs-image]: https://docs.rs/slh-dsa/badge.svg
[docs-link]: https://docs.rs/slh-dsa/
[build-image]: https://github.com/RustCrypto/signatures/actions/workflows/slh-dsa.yml/badge.svg
[build-link]: https://github.com/RustCrypto/signatures/actions/workflows/slh-dsa.yml
[license-image]: https://img.shields.io/badge/license-Apache2.0/MIT-blue.svg
[rustc-image]: https://img.shields.io/badge/rustc-1.85+-blue.svg
[chat-image]: https://img.shields.io/badge/zulip-join_chat-blue.svg
[chat-link]: https://rustcrypto.zulipchat.com/#narrow/stream/260048-signatures

[//]: # (links)

[RustCrypto]: https://github.com/RustCrypto
[FIPS-205 Standard]: https://csrc.nist.gov/pubs/fips/205/final
