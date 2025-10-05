# Bao1x Boot Chain

The Bao1x boot chain is as follows:

## Boot 0

`boot0` is a minimal, no-UI assumed code verification root of trust. It is designed to be compatible with any board. It is also immutable and cannot be updated.

### Boot 0 characteristics

  - `boot0` is setup to be read-only
  - It is burned into the chip during final test in the OSAT, and then sealed by configuring IFR bits (special JTAG-only accessible bits after which the JTAG port is fused out)
  - It assumes no external LDO, and so configures the CPU to run at a reduced speed (200MHz) for maximum compatibility.
  - It will draw around 70mA (typ) as it operates
  - It completes code checking in about 70ms
    - There is a pre-boot power-on delay of an additional ~100ms where low-level FSMs are initializing the chip (e.g. setting up error correction, clearing caches, etc.), so the total delay is ~170ms (TODO: explicitly measure this once the final code is in place, the numbers listed here are rough estimates from memory).
  - It assumes no UI
  - It checks both the integrity of `boot0` and `boot1`. If `boot1` has no valid signature or the `AltBootCoding` one-way counter has an odd modulus, it falls back to the `baremetal`/`loader` region for a fail-safe boot image.
  - The root of trust comes from a bank of up to four `ed25519` public keys that are burned into the top of `boot0`.
    - The fourth key is the `developer public key`. If code is found to match this key, the secret key area is erased and the chip is put into developer mode. On the first transition to `developer mode` the chip will need to be rebooted. There is no undo for this operation.
    - A key slot is skipped if all entries in it are 0.
    - A key slot is skipped if the key is revoked. A key is revoked by advancing the one-way counter corresponding to the key slot. *Note: once the developer key is revoked, it is impossible to enter developer mode*.
    - The keys shipped in `boot0` are placed there by Baochip on the standard SKU. For large orders (10k+ chips) custom keys are possible with some setup fee.
  - If none of the keys match, the chip volatile state is zeroized, a series of 'X' is printed on the DUART, and then the CPU is hung.
  - If the code signing is good, boot is allowed to proceed to `boot1`.

### Building Boot 0

`cargo xtask bao1x-boot0` will build a boot0 that only contains the developer public key.

Public keys corresponding to higher security private keys will be added at a later date. The current plan is to implement two high-strength signing keys in a pair of Precursor devices that are kept off-line and in a physically secure vault. One of these need to be brought out to sign new images. The other is a reserve in case the other is compromised.

The `boot0` image can only be written to the device with specialized JTAG commands, when the device is in the "CP" (chip probing) state. In a normal device flow, this state is only available while the chip is in the full wafer form and is locked out prior to packaging and singulation.

There are, of course, special developer chips running around in the world that have `boot0` writeable even though they are packaged. They are noted with a "ES" (engineering sample) suffix on the part number, or they are in a totally different package than normal (BGA instead of a WLCSP).

## Boot 1

`boot1` is a writeable, updateable partition. The code must be signed by Baochip for it to be runnable; or the chip has to be set into developer mode in which case any pre-existing secrets are erased and anyone can install their own `boot1` code.

Unlike `boot0`, `boot1` contains board-specific code. A multi-step update process is made available to OEMs who make their own versions of hardware using a Baochip, but in general end users are not envisioned to be updating this block except on a regular basis.

### Supported Boards

The version provided by Baochip in the default SKU is designed to work with the system assumptions present in Dabao, or Baosec. In particular:
 - `Dabao` variant assumes PC13 is dual-purposed as the `boot update` switch and USB disconnect switch. It also assumes there is no DCDC regulator and the unit is entirely self-powered. All other pins are assumed to be tri-stated.
 - `Baosec` variant assumes an OLED display on PC0-PC4 + peripheral reset on PC6. It also assumes a key matrix on PF2-5/PF6-7, an AXP2101 on I2C0 PB11/12 which must have BLDO1 configured to 3.3V keep the system on, a DCDC disable on PF0, and a USB SE0 switch on PF5.
 - `Oem` variant falls back to `Dabao` in the Baochip implementation, but the extra coding is provided for developers to define their own type in their own boot1.

The version of the code presented is dynamically selectable by the `BoardTypeCoding` one-way counter. Baosec includes a step in the factory to set the `BoardTypeCoding` one way counter to an odd modulus. A time-out on accessing the AXP2101 will cause the system to fall back to `Dabao`-like behavior.

### Boot Flow & Update Behavior

`boot1` will, by default, attempt to validate the next stage (`baremetal`/`loader`) and run it without any user intervention. The one-way counter `BootWaitCoding` can be incremented where on odd values it the system will by default wait for a new image or console command to present itself. This is useful for developers who are running a tight code/test loop. Alternatively, if any of the valid buttons for either Dabao or Baosec configurations are detected to be pressed, it will enter the boot-wait mode.

The `boot1` console runs over both serial port on PB13/14, or the USB over USB-serial. It defaults to using the serial port, until a valid USB connection is detected. If the USB is unplugged or a button press is detected (after release of the initial button used to enter the boot wait) it will automatically attempt to boot the next stage.

### Updating Protocol

In the console mode, files may be sent to the boot1 loader for writing to any of these valid partitions:

- `baremetal` is for applications that prefer to run without an OS
- `loader` is in the same slot as `baremetal` and acts as a loader for the Xous kernel
- `kernel` is the slot for the kernel image. It is a monolithic blob written to RRAM which contains the actual kernel plus core services meant to be resident on-chip.
- `app` are a series of slots for application images located in unencrypted but authenticated off-chip memory.

In USB-serial mode, data is transferred to the device using a custom base64 encoded serialization protocol, where the memory offset and type of block are encoded in each base64 packet. This takes advantage of the 32-byte erase block size of RRAM to give highly flexible chip programming.

In USB-mass storage mode, data is transferred as UF2 formatted objects, which are then unpacked and programmed into memory.

#### Side-Quest: App translation layer

*This is a to-do note inserted in the docs because it made sense to think of it here. Eventually, this needs to be implemented and cleaned up into a more sensible location.*

Because applications can be installed in any order, memory fragmentation is a problem. In order to address this, the app space is passed through a translation lookup layer.

- App area is organized into 64-kiB remap blocks
- An index of which program ID each block maps to is included at the top of swap memory. The size of the index varies with the size of the disk, and is encoded into the structure format.
  - For a 16MiB partition, that would correspond to 256 entries in the index.
  - The loader would read this index in and create a hash table that allows the efficient mapping of a program ID + remap offset to a physical offset
  - In each 32-bit entry in the index:
    - An 8-bit program ID uniquely identifies up to 256 programs
    - 16 bits identify the offset of the block in the memory
    - The remaining 8 bits are reserved

### Building Boot 1

`cargo xtask bao1x-boot1` will build the boot1 loader.

### Replacing or Updating Boot1

Users who wish to replace/update `boot1` (for example, OEMs making their own version of a board using Baochip) must do the following procedure:

1. Write a `failsafe` boot image to the `baremetal`/`loader` region
2. Increment the `AltBootCoding` one-way counter
3. Reboot into the `failsafe` boot image
4. Load the new `boot1` into the boot1 slot
5. Increment the `AltBootCoding` one-way counter
6. The next boot will be running the user `boot1` code

## Baremetal or Loader Stage

The baremetal or loader stage are both located at the exact same offset. One can think of the loader as "just another baremetal app" that happens to blossom into a full-blows OS.

Because much of the hardware initialization is done by boot0/boot1, baremetal and loader *should* shed much of the hardware initializations it does. The only re-init that needs to happen are for parameters unique or different to the environments.

Documentation of the baremetal or loader stages is deferred to the respective crates, which are located outside of this subdirectory.