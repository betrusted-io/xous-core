target\debug\create-image.exe `
    target\riscv32imac-unknown-xous-elf\release\xous_presign.img `
    --kernel target/riscv32imac-unknown-xous-elf/release/xous-kernel `
    --init target/riscv32imac-unknown-xous-elf/release/xous-ticktimer `
    --init target/riscv32imac-unknown-xous-elf/release/xous-log `
    --init fake-root-keys\target\riscv32imac-unknown-xous-elf\debug\fake-root-keys `
    --init pddb-raw\target\riscv32imac-unknown-xous-elf\debug\pddb-raw `
    --init target/riscv32imac-unknown-xous-elf/release/xous-names `
    --init target/riscv32imac-unknown-xous-elf/release/xous-susres `
    --init target\riscv32imac-unknown-xous-elf\release\pddb `
    --init target\riscv32imac-unknown-xous-elf\release\trng `
    --init target\riscv32imac-unknown-xous-elf\release\llio `
    --init target\riscv32imac-unknown-xous-elf\release\spinor `
    --svd utralib/renode/renode.svd

target\debug\copy-object.exe `
    target/riscv32imac-unknown-xous-elf/release/loader `
    target/riscv32imac-unknown-xous-elf/release\loader_presign.bin

target\debug\sign-image.exe `
    --loader-image target/riscv32imac-unknown-xous-elf/release\loader_presign.bin `
    --loader-key devkey/dev.key `
    --loader-output target/riscv32imac-unknown-xous-elf/release\loader.bin `
    --min-xous-ver v0.9.8-791

target\debug\sign-image.exe `
    --kernel-image target/riscv32imac-unknown-xous-elf/release\xous_presign.img `
    --kernel-key devkey/dev.key `
    --kernel-output target/riscv32imac-unknown-xous-elf/release\xous.img `
    --min-xous-ver v0.9.8-791
    