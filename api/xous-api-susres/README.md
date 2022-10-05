# Xous Api: susres

This crate defines the Xous suspend/resume API.

The suspend/resume API is responsible for sequencing events before
a system powers down, such that all of the SoC hardware blocks will
lose state, but the RAM will retain its contents.

Every service that contains hardware state which can be lost when
the SoC is powered down must register with the sequencer, and it
must atomically back up its hardware state to battery-backed RAM
and then call `suspend_until_resume`, which will block until power
is restored. Once power is restored, the service should copy the
hardware state out of battery-backed RAM and resume operations.

The API also contains hooks for initiating the suspend process,
and for rebooting the device.
