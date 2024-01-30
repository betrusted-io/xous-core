# Audio Samples
The .wav files included here are meant for audio testing only and
are intended to be bundled into the final distribution. Developers
who want to use this for testing will need to manually stage and load
these samples into the correct location in the FLASH. See
https://github.com/betrusted-io/xous-core/blob/master/docs/flash.md
for the locations of the samples.

`tools/usb_update.py` can provision the "long_8khz.wav" sample that is used
by the latest demo of audio playback in xous-core.

The track samples included here are from "Midwinter", by
Jackalope (https://soundcloud.com/tokyojackalope)

Used with permission to redistribute as part of this repository.

# SoC Prebuild
A copy of a pre-built FPGA SoC binary that matches the latest version
of the current tree is also included here. It's recommended to
flash this pre-build FPGA image so your kernel matches the hardware
if you're only building the kernel from scratch, and not also
building the FPGA hardware.
