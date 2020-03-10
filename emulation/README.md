# Platonic RISC-V Device

Platonic is an ideal system useful for developing an operating system.  It does not represent any sort of real hardware, though differences between platforms is minimal.

## Usage

It is recommended to use Renode.

## Debugging

$bin=@../../Xous/kernel/target/riscv32i-unknown-none-elf/debug/xous-kernel; i @scripts/single-node/litex_vexriscv.resc; machine StartGdbServer 3333

# qemu

$(QEMU) -machine $(MACH) -cpu $(CPU) -smp $(CPUS) -m $(MEM)  -nographic -serial mon:stdio -bios none -kernel $(OUT) -drive if=none,format=raw,file=$(DRIVE),id=foo -device virtio-blk-device,scsi=off,drive=foo
