# Serial Consoles

Serial consoles are the primary method for interacting with devices and the Xous kernel. It operates over either a physical or virtual [serial port](https://en.wikipedia.org/wiki/Serial_port). Once you have set up a serial console, you will be able to talk to the various aspects of your chip through a shell-like [REPL](https://en.wikipedia.org/wiki/Read%E2%80%93eval%E2%80%93print_loop) mechanism.

## Baochip-1x Serial Console

Baochip relies primarily upon a "virtual serial console", which is a [USB CDC-ACM](https://en.wikipedia.org/wiki/USB_communications_device_class) device. In short, when you plug a Baochip into your computer, you will get something similar to either a COM port (on windows) or a tty node (on Linux).

A limitation of the virtual serial console is that if the system crashes during boot, you can't see the output. As a backup, there is a physical serial console also allocated by default by the bootloader. One can access these through pads PB13 (Rx: accepts data from your computer) and PB14 (Tx: sends data to your computer).

Access requires a physical serial cable with the following characteristics:

- Signal levels at VDDIO (maybe 1.8V or 3.3V; on `dabao` it is 3.3V)
- Supports 1,000,000 N-8-1 comm rates

Baochip-1x is tested using [Genuine FTDI serial cables](https://ftdichip.com/products/ttl-232r-rpi/). Note that some USB serial adapters, such as those from Olimex, can *not* support the 1,000,000 baud rate.

## Precursor Serial Console

Precursor supports a physical serial console which runs at 115200 N-8-1. It has an internal multiplexer that allows one pair of wires to be selected between the kernel, console, and application serial ports.

- kernel is where kernel messages & panics are printed
- console is the primary user port. It presents a shell-like environment
- application is an unassigned "free" serial port, usable by applications for debugging or other functionality

The Precursor serial port is accessible via the debug cable on the Raspberry Pi HAT, using the built-in serial port of the Raspberry Pi SoC.

## How to Interact with a Serial Console

Connecting to a serial console requires a helper program in your host environment.

### Windows

[putty](https://www.chiark.greenend.org.uk/~sgtatham/putty/latest.html) is a typical Windows-based serial console client. The hardest part is discovering your COM port. To find it, start the device manager (Windows-X -> Device Manager) and look for an entry labeled "Ports (COM &LPT)". If there is just one COM entry there, you're done, that's your COM port.

If there is more than one COM entry, unplug the cable you intend to connect to. The entry that disappeared is the one you want to use.

Inside PuTTY, select the "Serial" option for "Connection type", and enter your COM port, followed by a speed of 1000000. Once you click on "open", you should be able to interact with the device. Note that for virtual USB-serial devices, it disconnects every time you reboot the device, or transition between boot and kernel stages. If you need to observe the transition between these states, you must use a physical serial cable.

### Linux

There are many ways to talk to serial ports in Linux: `minicom`, `screen`, `picocom`, `cu` - it varies by distribution. If none of the options listed here work for you, please refer to your distribution's maintainers for guidance on how to talk to a serial port.

`screen` is the method used to test Xous interactions and `pyserial` is the method used to automate interactions.

Under Ubuntu/debian-like distributions, you can install `screen` with `apt install screen`. Then, add your user to the `dialout` group so you can access the serial port without relying on `sudo`:

`sudo usermod -aG dialout $USER`

You will need to log out and log back in again for this to take effect. Some distros may require you to install a custom `udev` rule. If you encounter permission issues, try prefacing your screen command with `sudo`.

Determine which device node the device is plugged into using `dmesg`. Look for an entry like this:

```
[180822.853542] usb 1-1.1: Manufacturer: Baochip
[180822.853552] usb 1-1.1: SerialNumber: HV92R0
[180822.864377] hid-generic 0003:1D50:6198.0045: hiddev96,hidraw0: USB HID v1.11 Device [Baochip Baosec-lite] on usb-0000:01:00.0-1.1/input0
[180822.867022] input: Baochip Baosec-lite as /devices/platform/scb/fd500000.pcie/pci0000:00/0000:00:00.0/0000:01:00.0/usb1/1-1/1-1.1/1-1.1:1.1/0003:1D50:6198.0046/input/input38
[180823.016898] hid-generic 0003:1D50:6198.0046: input,hidraw1: USB HID v1.11 Keyboard [Baochip Baosec-lite] on usb-0000:01:00.0-1.1/input1
[180823.019625] cdc_acm 1-1.1:1.2: ttyACM0: USB ACM device
```

Here, `ttyACM0` is the device node. Connect to it using `screen /dev/ttyACM0 1000000`.

`screen` requires control sequences to exit the terminal. By default it is `control-A \`, as in, hold ctrl+A, and then hit `\`. It will ask you if you want to exit screen, and respond with `y` to exit.

Note that `screen` will leave zombies around if there is an ungraceful exit for any reason. These are zombie processes that fight for control over the serial port and lead to extremely confusing behavior. If your terminal goes unresponsive to input or your output looks garbled, check for zombies.

To check for such zombies, run `ps aux | grep -i screen`. If you see any entries other than the `grep` entry, then you may have to kill the zombie. Either kill it using its process ID, or you can run `sudo pkill -i screen`. The `-i` is particularly important because often times the zombie is called `SCREEN`, which needs to be killed with `sudo` and must be either typed in exact case or referred to with the `-i` option.
