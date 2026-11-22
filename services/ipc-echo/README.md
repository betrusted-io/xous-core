# ipc-echo

Skeleton MutableBorrow echo server used by the `ipc-ci` Renode suite
(`services/ipc-tests`). It deserializes an `EchoPayload` lend and replaces it
with a response whose serialized length differs from the request (data
reversed, label dropped), which exercises the kernel -> client `used`-offset
return path in `Buffer::lend_mut` end to end. Nothing else should live here:
if a test needs behavior this doesn't provide, the suite should grow a new
opcode, not a dependency.

Built only as part of `cargo xtask ipc-ci`; see services/ipc-tests/README.md.
