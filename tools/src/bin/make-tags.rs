extern crate xous_tools;

use std::fs::File;

use xous_tools::make_type;
// use xous_tools::tags::init::Init;
use xous_tools::tags::memory::{MemoryRegion, MemoryRegions};
use xous_tools::tags::xkrn::XousKernel;
use xous_tools::xous_arguments::{XousArguments, XousSize};

const RAM_START: XousSize = 0x40000000;
const RAM_SIZE: XousSize = 4 * 1024 * 1024;
const FLASH_START: XousSize = 0x20000000;
const FLASH_SIZE: XousSize = 16 * 1024 * 1024;
const IO_START: XousSize = 0xe0000000;
const IO_SIZE: XousSize = 65_536;
const LCD_START: XousSize = 0xB0000000;
const LCD_SIZE: XousSize = 32_768;

fn main() {
    let mut args = XousArguments::new(RAM_START, RAM_SIZE, make_type!("sram"));

    let mut regions = MemoryRegions::new();
    regions.add(MemoryRegion::new(
        FLASH_START,
        FLASH_SIZE,
        make_type!("ospi"),
    ));
    regions.add(MemoryRegion::new(IO_START, IO_SIZE, make_type!("ioio")));
    regions.add(MemoryRegion::new(LCD_START, LCD_SIZE, make_type!("mlcd")));
    args.add(regions);

    // let init = Init::new(
    //     0x20500000, 131072, 0x10000000, 0x20000000, 32768, 1234, 0x10000000,
    // );
    // args.add(init);

    let xkrn = XousKernel::new(
        0x20500000, 65536, 0x02000000, 0x04000000, 32768, 5678, vec![],
    );
    args.add(xkrn);

    println!("Arguments: {}", args);

    let f = File::create("args.bin").expect("Couldn't create args.bin");
    args.write(f).expect("Couldn't write to args");
}
