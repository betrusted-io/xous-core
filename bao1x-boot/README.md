# Bao1x Bootloader

This directory contains the bootloader for the Bao1x. It is built using baremetal primitives.

- See [BOOTCHAIN.md](./BOOTCHAIN.md) for a description of the secure boot chain
- See [RELEASES.md](./RELEASES.md) for a description of the release process
- See (PQ.MD)(./PQ.md) for a discussin of post-quantum hardening

## Lot code to Bootloader Version

- T60M66.0 (A0) built with 5397e1b48 (v0.10.0)
- T6X960.06 (A1) built with 5397e1b48 (v0.10.0)

### Baobit Audit References

#### v0.10.0

```
boot0 partition: a0680a73106600c4ece34f9393fc5c8eed4885e844b1b78b58fb3a8a299a885                                                                     cd794c667a5e787083bce46bf7aff8d3f4affa6a93a38ba7b507bb0d47c70fa01
boot0 code only: 563802e7f10fd0ca0a1c700c6313eff624aa57d9cd88d3c33af4b720eff5480                                                                     432e01c116925a324a96e12d92953d7cb6076ea4557a22058e1a61be4c2b4dee2
boot0 baobit toolchain: 441559d7e1984623ec0a52f60f90240c740b6c41
boot1 partition: a12eb17f37c88e834bb7beef3f5163f232824e8044fcbab3bb37c6ac54d10f7                                                                     8d61ace96da9ebfc03e3cc9514312f22b18a9481747fc0526dc89733789db3927
boot1 code only: e42b5aba61d98990ca24f5fc3b6cf012b14ea4d121f9bbe494baf4d9ae854d3                                                                     ba88cdf26a288b88778826f816dc960c3ffe24b7c919108a3aca90f2dc4551a55
boot1 baobit toolchain: 441559d7e1984623ec0a52f60f90240c740b6c41
```
