# Xous Apps and the Manifest Format

You can add apps to Xous by creating crates in this directory and adding them to the workspace.

1. Create an app by adding its crate to this directory, perhaps by copying an existing template and giving it a new name. Don't forget to change the name of the app in the app's `newapp/Cargo.toml` file, as this is what the build system uses to refer to the app.
2. Register the crate in the Workspace by editing the root `../Cargo.toml` (one directory above this README), and adding the new app crate's path to the `default-members` and the `members` lists.
3. Add an entry to `manifest.json` in this file so the build system can fold it into the system menus. Format documented below.
4. Rebuild using `cargo xtask app-image [app1] [app2] [...]`, where the `appN` are the names of the app crates you want to have built into your burnable Xous image.
5. Burn your new `xous.img` file onto your device using `tools/usb_update.py -k`. You will likely need to set up some drivers or install some packages, please [refer to the update guide](https://github.com/betrusted-io/betrusted-wiki/wiki/Updating-Your-Device#i-dont-rtfm-give-me-the-latest-xous) for more details.

As of Xous 0.9.6, the OS takes up about half the available RAM (8 out of 16 MiB) for code + data. For smooth operation of the PDDB and other resources, we suggest leaving about 4MiB of free space, so apps should try to stay within the range of 4MiB in size.

Note that as of Xous 0.9.6, code is copied into RAM from FLASH, so a large portion of the RAM usage is actually the code. This may be addressed in future versions, but this would require a fairly major break with how code verification and updates are done.

# manifest.json Format

Each entry for an app needs a record in `manifest.json` with this format:

```json
    "app_crate_name": {
        "context_name": "freeform name",
        "menu_name": {
            "appmenu.app_name": {
                "en": "app name in English",
                "ja": "app name in Japanese",
                "zh": "app name in Chinese",
                "en-tts": "app name for the blind"
            }
        }
    },
```

- All app names and descriptors must be unique across the build system
- `app_crate_name` should be replaced with the name of your app as specified by the "package" name in the app's `Cargo.toml` file.
- `context_name` is a reserved keyword and cannot be modified; its associated value ("freeform name" in this example) is a unique, free-from name that you give your app. There is a 64-character limit on this field.
- `menu_name` is a reserved keyword and cannot be modified.
- `appmenu.app_name` is the localization substitution string. This must be a unique name, and it s free-form. By convention, we use `appname.` as a prefix to the name of the app as described in the crate, but as long as it is unique nothing should break.
- Within the the `appmenu.app_name` record are the localized names for your App. We suggest creating strings for every language supported by the system. If you don't know how to translate your name, just use the same name in the language of your preference. This will at least prevent builds from breaking in different languages.
