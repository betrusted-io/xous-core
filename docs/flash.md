# FLASH memory layout

Xous is designed to operate from a SPI FLASH filesystem. The key properties assumed of the
underlying implementation are as follows:

- XIP-capable
- Memory-mapped
- 4kiB erase sectors (same as page size)
- Transparent bad block mechanism
- ~100k erase/program cycles per sector

The FLASH memory used in the Precursor implementation is a Macronix MX66UM1G45G, which
is a 128MiB part, upgradable to higher capacities.

In Precursor, FLASH memory is located at a base of 0x2000_0000, which is the constant
`HW_SPIFLASH_MEM` in the `utra`, and the length is coded as `HW_SPIFLASH_MEM_LEN`.

Below is a map of the FLASH memory. For completeness, note that the hardware
boots from location 0x0000_0000, from 32kiB a non-Xous boot ROM that is compiled into
the FPGA binary image itself. This ROM currently consists of just a few instructions
to jump to the Xous loader, but eventually this will need to house the code that does
public key signature verification of the Xous kernel itself before jumping to it.

Also note that main SRAM is, by default, battery-backed, so during shutdown, the RAM
image stays resident. This means no space is allocated in FLASH for hibernation or standby.

Finally, everything in the memory layout up until the PDDB is considered to be largely
static data, only written during firmware and software updates. Therefore, no provision
exists for sector-sparing or weare levelling; we instead rely on the 100k-cycle write
endurance of the underlying hardware. The PDDB itself will have to implement some form
of wear leveling, however.

```
+-----------+----------------------------------------+
+ 2000_0000 |   Primary FPGA bitstream               |
+ 2021_7287 |   2,192,008 bytes                      |
+-----------+----------------------------------------+
+ 2021_7288 |   Reserved for bitstreams              |
+ 204F_FFFF |                                        |
+-----------+----------------------------------------+
+ 2050_0000 |   loader.bin - Xous loader             |
+ 2050_FFFF |                                        |
+-----------+----------------------------------------+
+ 2051_0000 |   Font planes                          |
+ 2097_FFFF |                                        |
+-----------+----------------------------------------+
+ 2098_0000 |   Xous kernel plus                     |
+ 20AF_FFFF |   Initial/trusted server set           |
+-----------+----------------------------------------+
+ 20B0_0000 |   Reserved                             |
+ 20CF_FFFF |                                        |
+-----------+----------------------------------------+
+ 20D0_0000 |   PDDB 'filesystem'                    |
+ 27FF_FFFF |                                        |
+-----------+----------------------------------------+

```
