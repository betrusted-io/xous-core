# Binary Data Used in Programming

This directory contains a collection of binary objects that get written into the chip by the chip probe station. These either set blocks that are untouchable by the CPU (in the IFR region), or they commit data to the main array (which is touchable by the CPU).

## IFR region blocks
* `ifr_0x280.bin`: locks out write access to boot0 partition from user code
* `ifr_0x280_jtag_disa.bin`: locks out write access to boot0 partition from both user code and JTAG
* `ifr_0xc0.bin`: defines the boot0 partition size
* `ifr_0x340.bin`: sets write-protect on the public keys stored in data slots 4-7

## Data blocks
* `pubkey-block.bin`: contains the public keys to be burned into data slots. Write to offset 0x3dc080 in the main array.

## One-Way Counter Defaults

These blobs are burned into the one-way counter array to force some counters to be tripped, thus pre-configuring the behavior of a given board.

These are convenience block for setting up chips for a given board, In production all OWCs are set to 0, we won't have board-specific SKUs.

* `dabao-owc.bin`: Sets board type to `dabao`, and sets wait on boot to true.

