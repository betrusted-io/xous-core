# Release Process for Bootloaders / Reproducible Builds

The final boot0/boot1 artifacts are built using guix. Here's the basic process:

- A release is tagged at a specific commit hash
- Rust is pinned to a specific version for the build
- boot0/boot1 are built using guix, and the hashes are checked against a third party verifier (Sylvain Bellemare) for reproducibility
- The artifacts are signed using the Baochip signing token
- They are uploaded to CI, and the release-CI is run
- Upon a full pass, the artifacts are transferred to chip probe for burning onto the chips
