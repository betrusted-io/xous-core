# Post-Quantum Hardening

The Baochip boot chain is hardened for post-quantum using the SPHINCS+ (SLH-DSA) NIST SP 800-230 (ipd).
The "ipd" means "initial public draft" - as of the writing of this documentation (July 2026), this PQ
scheme, which is suitable for use on small embedded targets, is still in draft status and thus could
be subject to change.

The standard encodes six signature types, optimized for firmware applications as follows:

| Parameter set               | n   | h   | d   | h′  | a   | k   | lg_w | w   | m   | pk  | **sig bytes** | cat |
| --------------------------- | --- | --- | --- | --- | --- | --- | ---- | --- | --- | --- | ------------- | --- |
| SLH-DSA-{SHA2,SHAKE}-128-24 | 16  | 22  | 1   | 22  | 24  | 6   | 2    | 4   | 21  | 32  | **3856**      | 1   |
| SLH-DSA-{SHA2,SHAKE}-192-24 | 24  | 21  | 1   | 21  | 25  | 9   | 3    | 8   | 32  | 48  | **7752**      | 3   |
| SLH-DSA-{SHA2,SHAKE}-256-24 | 32  | 21  | 1   | 21  | 25  | 12  | 2    | 4   | 41  | 64  | **14944**     | 5   |

"Optimization for firmware signing" implies the following:
- Sign-few, verify many: the signatures can be broken if more than 2^24 firmwares are signed
- Signing is an expensive operation: on a desktop class machine, takes a few seconds
- Verification is a cheap operation: on the bao1x, verifies in a few milliseconds
- Signature is **relatively** compact (3856 bytes)

## Implementation

The RustCrypto slh-dsa crate is forked at commit `eda9d85840d` into [slh-dsa-bao1x](./slh-dsa-bao1x/).
On top of this the following things have been implemented:

- Verified support for SLH-DSA-SHA2-128-24 - this is the scheme we're using for Baochip
- Unverified support for the five other signature variants in NIST SP 800-230
- Signing acceleration through a combination of parallelization and caching of the xmss tree
- Verification acceleration through hardware hashing of the incoming message
- KAT (known answer test) derived from the `sphincs/sphincsplus` C reference implementation

The KAT can be found in [slh-dsa-kat](./slh-dsa-kat/).

The hardware interface for the Baochip-1x hasher had to be upgraded to `digest` API 0.11.0 to work
with the PQ crate. This version is in [sha2-bao1x-pq](./sha2-bao1x-pq/).

See the slh-dsa [CHANGELOG](./slh-dsa-bao1x/CHANGELOG.md) for technical details of the changes laid into
the crate. However, one thing to be aware of is that the category 3 and 5 signatures are impossible
to use with the current Rust ecosystem because `hybridarray::ArraySize` v0.4 with `extra-sizes` only
provides a `U3856` type, but it does **not** provide `U7752` or `U14944` types, which are needed
to encode teh signature sizes of those schemes. Thus those signatures are gated off with the
`sp800-230-highsec` feature by default.

## Compatibility Policy

The PQ hardening is applied as a layer in addition to the existing ed25519 signature scheme. Thus Baochip
uses a "hybrid" verification scheme.

PQ enabled bootloaders by default will accept non-PQ signed next stages. This is to facilitate backward
compatibility.

However, a one-way counter maybe incremented, and if its value is not 0, the bootloader will **require**
a PQ signature on the next stage to be considered trusted.

This means by default, chips coming from the factory will permissively allow non-PQ code. However,
users may "flavor" their chips by incrementing the "PQ required" flag and opt into a mandatory-PQ
ecosystem.
