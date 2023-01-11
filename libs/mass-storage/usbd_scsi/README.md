The contents of this directory were forked from [stm32-usb.rs](https://github.com/stm32-rs/stm32-usbd).

This device isn't an STM32. As a result, the code was modified heavily. It doesn't make sense to push patches back to the parent repo.

Thus, the code was forked at commit [c9cdd0daa920ea72fcb2ba72c0f682733b7034a7](https://github.com/stm32-rs/stm32-usbd/commit/c9cdd0daa920ea72fcb2ba72c0f682733b7034a7) and copied here.

Below is the original README.md included in the stm32-usb.rs repo. The original LICENSE files are also included adjacent to this README.

# usbd_scsi

[![Crate](https://img.shields.io/crates/v/usbd_scsi.svg)](https://crates.io/crates/usbd_scsi)
[![Documentation](https://docs.rs/usbd_scsi/badge.svg)](https://docs.rs/usbd_scsi)

[`usb-device`](https://crates.io/crates/usb-device) implementation that provides a USB scsi transparent command set subclass.

## License

Free and open source software distributed under the terms of both the [MIT License][lm] and the [Apache License 2.0][la].

[lm]: LICENSE-MIT
[la]: LICENSE-APACHE