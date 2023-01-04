# usb_bootloader

WIP USB bootloader in rust that supports [UF2](https://github.com/microsoft/uf2)

## Status

* Broadly working but experimental.
* usb-bootloader can be flashed to a bluepill dev board with no modifications
    * [deploy_standalone](deploy_standalone) should flash a working bootloader to a bluepill connected to an ST-LINK. If it doesn't work try [run_openocd](run_openocd) to make sure OpenOCD is working correctly. It's sometimes necessary to hold down the reset button while launching OpenOCD if the core has got into a weird state. If you want to debug the bootloader, run [run_openocd](run_openocd) in one terminal then [release](release) in another to launch gdb with a build that has ITM tracing turned on.
* `../blink/deploy_to "/media/.../BLUEPILL"` will build a blink example, convert it to UF2 and copy it to the USB drive
* usb-bootloader could be relatively easily changed to work with any embedded-hal implementation that has implemented [usb-device](https://github.com/mvirkkunen/usb-device)
* The flash reading/writing code in usb-bootloader could be moved into the embedded-hal implementations - it would be nice to have a simple trait that can read/write blocks of bytes from flash without having to worry about page size and other device specific details.

### Issues

* Smallest bootloader binary is currently ~22kb which is a fair bit larger than the 16kb of the C version
    * `cargo bloat` shows the main bits of code add up to ~14kb but even with [logging disabled and panic abort](https://jamesmunns.com/blog/fmt-unreasonably-expensive/) the formatting machinery is still being included. I've briefly looked into it and failed to work out why this is.
* I've only tested it on linux, the C version of the UF2 bootloader added various fixes to make Windows and OS X work correctly over time - I've included the obvious ones while coding usbd_scsi but not done any testing outside of linux. If people actually want to use it there and raise issues I'll gladly take a look

## License

Free and open source software distributed under the terms of both the [MIT License][lm] and the [Apache License 2.0][la].

[lm]: LICENSE-MIT
[la]: LICENSE-APACHE