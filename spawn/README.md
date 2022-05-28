# Spawn a process

This crate represents the initial program that will run when a process
starts up. It will listen on a Server and take orders from the parent
process to finish setting itself up.

## Initialization

The process starts with a stack and a text section but no heap, BSS,
or data section. The entrypoint looks like this:

```rust
pub extern "C" init(connection: xous::CID) -> ! {
    loop {}
}
```

This process is responsible for performing its own bootstrapping, after
which it should accept messages on the connection and copy data as necessary.

## API

The following messages are supported:

Mnemonic | Opcode | Type | Description
--- | ----- | ---- | ----
WriteMemory|1| M | Write memory into an area of memory. The `Offset` field is used to determine where the block will start.
WriteArgs | 2 | M | Reserved
WriteEnvironment | 3 | M | Reserved
FinishSetup | 255 | * | Terminate the loop, shutdown the server, and start the program.