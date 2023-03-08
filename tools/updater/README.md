# Precursor Updater

This script automatically updates a Precursor device. Please run with `--help` to see
all the available options. By default, the script will attempt to load the latest stable release onto a Precursor device that is attached to the host via USB.

When the script is finished running, you will need to reset the device by pressing on the
switch inside the hole in the lower right hand side of the case. See https://ci.betrusted.io/i/reset.jpg for an example of how to do this.

See our [troubleshooting](https://github.com/betrusted-io/betrusted-wiki/wiki/Updating-Your-Device#troubleshooting) guide for more help, especially if you are running Windows.

# Maintainer Notes

This note is for the maintainer of the tool. Users do not need to run these commands.

The repo is structured to use `setup.py`, so the first hits for documentation that guide you through using the `toml` manifest options won't work (couldn't get them to work anyways, even though I tried with the toml method). Thus, the method to push a new release is as follows:

1. Run `python3 setup.py sdist` in this directory
2. Run `python3 -m twine upload dist/precursorupdater-0.0.XXX.tar.gz` where XXX is replaced with the version number.
