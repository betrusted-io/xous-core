#  a crappy starter script to help with builds under Windows environments. ymmv.

# set RUSTFLAGS=--remap-path-prefix=F:\largework\rust-win\code\xous-core\=build
# $env:RUSTFLAGS="--remap-path-prefix=$(Get-Location)=build"

cargo xtask app-image ball repl
# cargo xtask ffi-test
# cargo xtask minimal precursors/soc.svd

CertUtil -hashfile precursors/bbram-test1.nky MD5
CertUtil -hashfile precursors/soc_csr.bin MD5

CertUtil -hashfile target/riscv32imac-unknown-xous-elf/release/loader.bin MD5
CertUtil -hashfile target/riscv32imac-unknown-xous-elf/release/xous.img MD5
scp -i c:/users/bunnie/.ssh/id_pi target/riscv32imac-unknown-xous-elf/release/xous.img target/riscv32imac-unknown-xous-elf/release/loader.bin precursors/soc_csr.bin pi@10.0.245.135:code/precursors/

# scp -i c:/users/bunnie/.ssh/id_ed25519 target/riscv32imac-unknown-xous-elf/release/xous.img target/riscv32imac-unknown-xous-elf/release/loader.bin precursors/soc_csr.bin bunnie@192.168.137.161:code/precursors/

# scp -i c:/users/bunnie/.ssh/id_ed25519 target/riscv32imac-unknown-xous-elf/release/xous.img target/riscv32imac-unknown-xous-elf/release/loader.bin precursors/soc_csr.bin bunnie@10.0.245.9:/mnt/c/Users/bunnie/

# CertUtil -hashfile target/riscv32imac-unknown-none-elf/release/loader.bin MD5
# CertUtil -hashfile target/riscv32imac-unknown-none-elf/release/xous.img MD5
# scp -i c:/users/bunnie/.ssh/id_pi target/riscv32imac-unknown-none-elf/release/xous.img target/riscv32imac-unknown-none-elf/release/loader.bin precursors/soc_csr.bin precursors/bbram-test1.nky pi@10.0.245.90:code/precursors/

# wishbone-tool --load-name target/riscv32imac-unknown-none-elf/release/xous.img  --load-address 0x980000 --load-flash
