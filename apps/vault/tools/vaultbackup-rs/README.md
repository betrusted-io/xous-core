# `vaultbackup-rs`

A tool to backup/restore `vault` app storage to/from your computer.

## Supported data

 - passwords
 - TOTP tokens

## Dependencies

[`hidapi`](https://github.com/libusb/hidapi) and `protoc` is required for this tool to work.

On Debian-based systems, it can be installed with `apt-get install libhidapi-dev protobuf-compiler`.

On top of that, Linux systems need the following `udev` rules:

```udev
SUBSYSTEM=="usb", ATTRS{idVendor}=="1209", ATTRS{idProduct}=="5bf0", GROUP="plugdev", TAG+="uaccess"
SUBSYSTEM=="usb", ATTRS{idVendor}=="1209", ATTRS{idProduct}=="3613", GROUP="plugdev", TAG+="uaccess"
```

Place them in `/etc/udev/rules.d/99-precursor-usb.rules`, and either reboot your system or reload `udev` rules with:

```
# udevadm control --reload-rules
```

`udev` rules aren't strictly required, they're there to allow you to communicate with Precursor without root privileges.

If you prefer prefixing `sudo` to every `vaultbackup-rs` invocation, you're free to do so.

## Usage

The Precursor device must have "host readout" enabled, by selecting the `vault` context menu at F4
and selecting "Enable host readout".

```
vaultbackup
A backup/restore tool for the Precursor vault app.

USAGE:
    vaultbackup-rs <SUBCOMMAND>

OPTIONS:
    -h, --help    Print help information

SUBCOMMANDS:
    backup     Backup data from device
    format     Format a known password manager export for Vault
    help       Print this message or the help of the given subcommand(s)
    restore    Restore data to device
```

Using the tool is quite simple:

```
vaultbackup-rs <ACTION> <TARGET> <PATH>
```

`ACTION` can either be `backup` or `restore`, while target specifies the data you want to act upon:

| Target  | Meaning  |
|---|---|
|`password`|Act on passwords storage|
|`totp`|Act on TOTP storage|

`PATH` specifies the path where `vaultbackup-rs` reads/writes data.

The output format is JSON.

`totp` target supports both TOTP and HOTP codes.

When the `totp_entry` object's `hotp` field is `true`, the `step_seconds` field is re-purposed as the HOTP count.

## **Important note 1**
This tool ONLY backs up entries in mounted bases, so if you are wanting to make a backup of some secrets but not others, make sure you unlock all the secret bases you want backed up, and UNMOUNT any you don't want backed up.

## **Important note 2**
This tool ALWAYS restores to the most recently unlocked basis, so if you desire to have the entries spread across multiple secret bases, you'll want to split the JSON file into separate files before attempting to import/restore the secrets so you can import only the necessary secrets for each mount. Once you've made a backup or a few (one for just the System "less secret" entries and one with additional secret bases unlocked), you will notice that the System entries show up in EVERY backup. If you want to quickly strip those out, you can use the handy python package `jsondiff` for this. `pip install jsondiff` and then `jdiff system-backup.json secretbase1.json -p -i 2` will output only the unique entries from the second file, so you could redirect this to a new file with a name that reminds you of the basis it should get imported to.

## Importing other password manager's exports

`vaultbackup-rs` supports importing other password manager's export data in Vault, but to do so, you have to format it to Vault's format first.

The `subcommand` does this for you.

Supported password managers:
 - Bitwarden: TOTP, logins
 - Google Authenticator: TOTP

### Bitwarden

```bash
$ vaultbackup-rs format bitwarden your-bitwarden-export.json
```

### Google Authenticator

For Google Authenticator, the input file is expected to contain
`otpauth://` or `otpauth-migration://` URIs, one per line; for example,
to import QR codes from the Google Authenticator app:

```bash
$ zbarcam --raw | tee authenticator-export.txt
$ vaultbackup-rs format authenticator authenticator-export.txt
$ vaultbackup-rs restore totp authenticator_to_vault_totps.json
```

If `zbarcam` can't decode a dense QR code with multiple TOTP exports, try selecting just one TOTP code at a time, and then exporting them in series. The commands will automatically collate all the individually read QR codes.

### Plaintext Passwords from CSV

Passwords may be imported from a CSV file with a two-step process. The first step is to run the following command:

```bash
$ vaultbackup-rs format csv-pass your_passwords.csv
```

This will read in a CSV and output a JSON file named `csv_to_vault_passwords.json`, suitable for restoring to `vault`. The second step is to do the actual restore operation, using the `restore` command, referencing the generated JSON fle. The intermediate JSON file may be inspected to ensure that the full CSV contents were translated correctly.

:warning: it takes a few minutes to upload a couple hundred passwords!

`vaultbackup-rs` will appear to "hang" while it is running if you have a lot of passwords to upload. It takes a long time because each password is atomically committed and synced to the cryptographic store individually.

#### CSV File Format

The CSV file should have the following header:

```
site,username,password,notes
```

`site` and `username` are mandatory fields; `password` and `notes` may be left blank, but the comma delimiters are still required to indicate the blank fields.

The CSV file may contain any valid UTF-8 characters, and anything between double quotes `"` is interpreted as a single field. For example:

```
site,username,password,notes
test.com,bunnie,f00b4r,a test password
commas.net,troll,"c0mm4,c,c","a site that loves commas,
and a new line in a CSV,,,"
simple_site,username only,,
```

The above CSV file contains three entries. `test.com` is the base case. `commas.net` shows how both the extra commas and the newline in the `notes` field are captured within a single field, because they are surrounded by `"`. `simple_site` shows a record with only a username, and no password or notes; note the two trailing commas holding the place for the blank `password` and `notes` fields.

The current implementation bails ungracefully if any lines are encountered without the correct number of fields. Here is an example of the error message in this case:

```
Error: CSV deserialization error, "Error(UnequalLengths { pos: Some(Position { byte: 780, line: 18, record: 11 }), expected_len: 4, len: 5 })"
```

The parser expects 4 fields (`expected_len`), but the actual length in ths case was 5 (so the problem is an extra comma).

If you are using unicode and/or special characters in your file, and you run on Windows, be sure to export the CSV with the correct encoder settings. Windows defaults to UTF-16, but Rust only operates on UTF-8.
