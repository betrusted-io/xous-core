# Mass Storage Libraries

The contents of this directory were forked from [stm32-usb.rs](https://github.com/stm32-rs/stm32-usbd).

This device isn't an STM32. As a result, the code was modified heavily. It doesn't make sense to push patches back to the parent repo.

Thus, the code was forked at commit [c9cdd0daa920ea72fcb2ba72c0f682733b7034a7](https://github.com/stm32-rs/stm32-usbd/commit/c9cdd0daa920ea72fcb2ba72c0f682733b7034a7) and copied here.

Below is the original README.md included in the stm32-usb.rs repo. The original LICENSE files are also included adjacent to this README.

# Original README

Experimental [UF2](https://github.com/microsoft/uf2) bootloader written in rust.

Ignore the `hardware` folder - the PCB may work but I didn't bother getting it manufactured in the end as the bluepill was sufficient for my needs.

Check out the [firmware/usb_bootloader](firmware/usb_bootloader) folder for more info.

## License

Free and open source software distributed under the terms of both the [MIT License][lm] and the [Apache License 2.0][la].

[lm]: LICENSE-MIT
[la]: LICENSE-APACHE