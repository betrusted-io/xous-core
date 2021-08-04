REM  a crappy starter script to help with builds under Windows environments. ymmv.

cargo xtask hw-image precursors/soc.svd
REM cargo xtask minimal precursors/soc.svd

CertUtil -hashfile precursors/bbram-test1.nky MD5
CertUtil -hashfile precursors/soc_csr.bin MD5

CertUtil -hashfile target/riscv32imac-unknown-xous-elf/release/loader.bin MD5
CertUtil -hashfile target/riscv32imac-unknown-xous-elf/release/xous.img MD5
scp -i c:/users/bunnie/.ssh/id_pi target/riscv32imac-unknown-xous-elf/release/xous.img target/riscv32imac-unknown-xous-elf/release/loader.bin precursors/soc_csr.bin precursors/bbram-test1.nky pi@10.0.245.90:code/precursors/

REM CertUtil -hashfile target/riscv32imac-unknown-none-elf/release/loader.bin MD5
REM CertUtil -hashfile target/riscv32imac-unknown-none-elf/release/xous.img MD5
REM scp -i c:/users/bunnie/.ssh/id_pi target/riscv32imac-unknown-none-elf/release/xous.img target/riscv32imac-unknown-none-elf/release/loader.bin precursors/soc_csr.bin precursors/bbram-test1.nky pi@10.0.245.90:code/precursors/

REM wishbone-tool --load-name target/riscv32imac-unknown-none-elf/release/xous.img  --load-address 0x980000 --load-flash
