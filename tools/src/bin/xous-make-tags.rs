use std::fs::File;

use xous_tools::tags::memory::{MemoryRegion, MemoryRegions};
use xous_tools::tags::xkrn::XousKernel;
use xous_tools::xous_arguments::{XousArguments, XousSize};

const RAM_START: XousSize = 0x4000_0000;
const RAM_SIZE: XousSize = 4 * 1024 * 1024;
const FLASH_START: XousSize = 0x2000_0000;
const FLASH_SIZE: XousSize = 16 * 1024 * 1024;
const IO_START: XousSize = 0xe000_0000;
const IO_SIZE: XousSize = 65_536;
const LCD_START: XousSize = 0xB000_0000;
const LCD_SIZE: XousSize = 32_768;

fn main() {
    let mut args = XousArguments::new(RAM_START, RAM_SIZE, u32::from_le_bytes(*b"sram"));

    let mut regions = MemoryRegions::new();
    regions.add(MemoryRegion::new(FLASH_START, FLASH_SIZE, u32::from_le_bytes(*b"ospi")));
    regions.add(MemoryRegion::new(IO_START, IO_SIZE, u32::from_le_bytes(*b"ioio")));
    regions.add(MemoryRegion::new(LCD_START, LCD_SIZE, u32::from_le_bytes(*b"mlcd")));
    args.add(regions);

    // let init = Init::new(
    //     0x20500000, 131072, 0x10000000, 0x20000000, 32768, 1234, 0x10000000,
    // );
    // args.add(init);

    let xkrn = XousKernel::new(0x2050_0000, 65536, 0x0200_0000, 0x0400_0000, 32768, 5678, vec![]);
    args.add(xkrn);

    println!("Arguments: {}", args);

    let f = File::create("args.bin").expect("Couldn't create args.bin");
    args.write(f).expect("Couldn't write to args");
}
