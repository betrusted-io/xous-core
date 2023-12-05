# Spawn a process & Read an ELF File

This crate is based on the one in services/test_spawn. The main
difference is that this one has the ability to read, load, and
execute an ELF file through the opcode `LoadElf`.

## API

The following messages are supported:

| Mnemonic     | Opcode | Type | Description                                                                                              |
|--------------|--------|------|----------------------------------------------------------------------------------------------------------|
| LoadElf      | 1      | M    | Reads and loads an ELF file sent as a MemoryMessage. `Offset` is used to determine where the file starts |
| PingResponse | 2      | S    | Returns the scalar sent except that arg1 += 1                                                            |

## A Note on Building

In case you need to recompile this program for use in `app-loader`,
use the following commands:
```
$ cargo build --package spawn --target riscv32imac-unknown-xous-elf --release
$ cargo run --package tools --bin copy-object target/riscv32imac-unknown-xous-elf/release/spawn
$ cp target/riscv32imac-unknown-xous-elf/release/spawn.bin apps/app-loader/src
```
