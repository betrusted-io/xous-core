# Xous user preference

Instead of having device settings sparse across the operating system UX, I propose we unify everything under a single umbrealla: a `Preferences` applet.

This would relieve `status` of the burden of having random stuff filed under its menu and streamline the codebase a little bit.

This applet would be comprised of two components:
 - a `preference` service that abstract a way of accessing an entity's preferences: given a key, `preference` returns a blob 

## Interesting toggles 

- [ ] Turn on Wi-Fi subsystem on boot
- [ ] Connect to known networks on boot
- [ ] Default backlight state
- [ ] (With autobacklight on) Backlight timeout
- [ ] Ask for device password after sleep wake-up
- [ ] Close and unmount all bases on device lock (before sleeping, manual locking in status menu)
- [ ] Keyboard layout
- [ ] Timezone
- [ ] Date/time
- [ ] Speaker volume
- [ ] Wi-Fi submenu