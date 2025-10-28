# Baochip Tools CLI

**Baochip Tools CLI** is a unified command-line tool for managing, building, flashing, and debugging Baochip-based embedded systems. It provides a suite of utilities to streamline development, deployment, and diagnostics for both firmware and hardware.

This tool is designed to be use by used by the VS Code extension at https://github.com/baochip/bao-vscode-ext but can also be used alone.


## Features

- **Serial Port Management**: List and monitor available serial ports.
- **Flashing**: Copy UF2 firmware images to a mounted boot drive.
- **Version Checking**: Compare local and device firmware versions to determine if updates are needed.
- **Artifact Listing**: List available UF2 images for flashing.
- **Environment Diagnostics**: Check Python and serial environment health.

## Installation

This CLI is designed to be run from the `tools-bao` directory:

```sh
python tools-bao/bao.py <command> [options]
```

## Requirements

Python 3.7+

Before using, please install the requirements:
```bash
python -m pip install --force-reinstall -r tools-bao/requirements.txt
````

## Commands Overview

### `ports`
List all available serial ports on your system.

**Usage:**
```sh
python tools-bao/bao.py ports
```

### `monitor`
Open a serial monitor to interact with a device.

**Options:**
- `-p, --port` (required): Serial port (e.g., `COM5`, `/dev/ttyUSB0`)
- `-b, --baud`: Baud rate (default: 115200)
- `--ts`: Show timestamps on received lines
- `--save`: Append output to a file
- `--reset`: Toggle DTR/RTS on open
- `--crlf`: Use CRLF as TX line ending (default LF)
- `--raw`: Send keystrokes immediately (raw mode)
- `--no-echo`: Do not locally echo typed input
- `--rtscts`, `--xonxoff`, `--dsrdtr`: Enable hardware/software flow control

**Usage:**
```sh
python tools-bao/bao.py monitor -p COM3 --ts
```

### `flash`
Copy one or more UF2 files to a mounted boot drive.

**Options:**
- `--dest` (required): Mount path of the UF2 boot drive (e.g., `D:\`)
- `files`: One or more UF2 files to copy

**Usage:**
```sh
python tools-bao/bao.py flash --dest D:\ loader.uf2 xous.uf2
```

### `artifacts`
List available UF2 images for flashing. Can output as JSON for scripting.

**Options:**
- `--json`: Output in JSON format

**Usage:**
```sh
python tools-bao/bao.py artifacts --json
```

### `update-all`
Check if the board’s firmware version differs from the local version.

**Options:**
- `-p, --port` (required): Serial port to query
- `-b, --baud`: Baud rate (default: 115200)
- `--timeout`: Seconds to wait for version response (default: 3.0)
- `--json`: Output in JSON format

**Usage:**
```sh
python tools-bao/bao.py update-all -p COM3 --json
```

### `doctor`
Check your Python environment and serial port setup for common issues.

**Usage:**
```sh
python tools-bao/bao.py doctor
```

## Global Options

- `--version`: Show CLI version and exit.
- `-v, --verbose`: Enable verbose output (debug logging and tracebacks).
