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
+ 2021_7288 |   Padding                              |
+ 2027_7FFF |                                        |
+-----------+----------------------------------------+
+ 2027_8000 |   csr.csv corresponding to bitstream   |
+ 2027_FFFF |   (32kiB max, see below)               |
+-----------+----------------------------------------+
+ 2028_0000 |   Reserved for backup bitstream        |
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
+ 27F7_FFFF |                                        |
+-----------+----------------------------------------+
+ 27F8_0000 |   512k reserved space for EC image     |
+ 27FF_FFFF |   Split into 300k wf200 + 200k EC fw   |
+-----------+----------------------------------------+

```
The EC update block is further structured as follows:
```
+-----------+----------------------------------------+
+ 27F8_0000 |   wf200 firmware control page          |
+     +1000 |                                        |
+-----------+----------------------------------------+
+ 27F8_1000 |   up to 311k for wf200 firmware        |
+   +4_D000 |   padded with trailing 0xFF            |
+-----------+----------------------------------------+
+ 27FC_E000 |   EC firmware control page             |
+     +1000 |                                        |
+-----------+----------------------------------------+
+ 27FC_F000 |   up to 199k EC fw                     |
+   +3_1000 |   padded with trailing 0xFF            |
+-----------+----------------------------------------+

```

The firmware control pages have the following structure:
```
base + 0x0000: [u8; 32] = SHA-512/256 hash from base+0x20 to last byte of firmware
base + 0x0020: u32      = length of firmware in bytes, computed from start of first firmware page
base + 0x0024: [u8; 988] = [0xff as u8; 988] 0xff padding to start of first firmware page
base + 0x1000: [u8; FW_LEN] = actual firmware

remainder of unused sectors are 0xff pad.
```


The csr.csv block is further structured as follows:

```
+-----------+----------------------------------------+
+ 2027_8000 |   Length of csr.csv data               |
+ 2027_8003 |   4 bytes, little-endian               |
+-----------+----------------------------------------+
+ 2027_8004 |   csr.csv data (variable length)       |
+ 2027_8xxx |   Typically ~12kiB, byte ordered       |
+-----------+----------------------------------------+
+ 2027_8xxx |   padding to 0xFF                      |
+ 2027_FFBF |   padding included in sha512           |
+-----------+----------------------------------------+
+ 2027_FFC0 |   sha512 of 2027_8000:2027_FFBF        |
+ 2027_FFFF |   64 bytes, network order              |
+-----------+----------------------------------------+
```

## Testing Structures

Prior to the creation of the PDDB, some hard-coded audio data is loaded for development purposes.

This documents their location in FLASH. The samples are shorter than the allocated regions, but the WAV headers encode their actual lenth.

```
+-----------+----------------------------------------+
+ 2600_0000 |   8khz short sample (WAV/<512kiB)      |
+ 2607_FFFF |   16-bit stereo PCM ~16s long          |
+-----------+----------------------------------------+
+ 2608_0000 |   44.1khz short sample (WAV/<2,816kiB) |
+ 2633_FFFF |   16-bit stereo PCM ~16s long          |
+-----------+----------------------------------------+
+ 2634_0000 |   8khz short sample (WAV/<28,928kiB)   |
+ 2707_FFFF |   16-bit stereo PCM ~330s long         |
+-----------+----------------------------------------+
+ 27F8_0000 |   Start of EC region (do not use)      |
+-----------+----------------------------------------+
```
