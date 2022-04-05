# Platonic RISC-V Device

Platonic is an ideal system useful for developing an operating system.  It does not represent any sort of real hardware, though differences between platforms is minimal.

## Usage

For emulation, you will need to install Renode (use the [nightly](https://dl.antmicro.com/projects/renode/builds/) to get `SPI.NORFlash` peripheral support). Then, run `renode` on `xous-release.resc`. For example:

```
renode emulation/xous-resc.repl
```

## Editing Renode Peripherals

Renode supports adding peripherals written in C#. For example, many Betrusted peripherals have models created under the `peripherals/` directory.

Since there is no external compiler, it can be difficult to know if your code is correct. In fact, if you're unfamiliar with C# or the Renode codebase, editing unfamiliar C# code can be a slow exercise in frustration.

Fortunately, Visual Studio Code is free and has excellent C# tooling. All you need to do is point it at your Renode installation, load the C# plugin, and open a `.cs` file.

1. Copy `peripherals.csproj.template` to `peripherals.csproj`
2. Open `peripherals.csproj` and point `<RenodePath>` to your Renode installation directory. On ubuntu, this may be `/opt/renode/bin/`.
3. Install [C# for VSCode](https://marketplace.visualstudio.com/items?itemName=ms-dotnettools.csharp)
4. You can refer to [core Renode peripherals](https://github.com/renode/renode-infrastructure/tree/master/src/Emulator/Peripherals/Peripherals) as examples of what C# code looks like.

## Debugging with GDB

When running Renode, you can attach a GDB instance. It runs on port 3333. Simply run `tar ext :3333` in gdb to attach.
