# `early_settings`

This crate provides a service and API to access some settings stored in the first 4096 bytes of FLASH.

Settings currently stored here:
 - keymap
 - early sleep flag

**Do not store any personal informations here, user settings must go into `libs/userprefs`!**

This scratch area is provided for extreme edge cases:
 - keymap must live here because users need to write their password in order to mount PDDB
 - early sleep is used in the "lock device" lifecycle, placing it here is the most convenient thing to do to achieve that use-case

If you're thinking of placing something in here please open an issue or shoot us a Matrix message on `#xous-apps:matrix.org`.