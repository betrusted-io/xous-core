extern crate crc;

use std::collections::VecDeque;
use std::convert::TryInto;
use std::fs::File;
use std::io::Write;

use xous_tools::elf::{process_minielf, process_program, read_minielf};
use xous_tools::sign_image::sign_image;
use xous_tools::tags::inie::IniE;
use xous_tools::tags::memory::{MemoryRegion, MemoryRegions};
use xous_tools::tags::pnam::ProcessNames;
use xous_tools::tags::xkrn::XousKernel;
use xous_tools::xous_arguments::XousArguments;

struct RamConfig {
    offset: u32,
    size: u32,
    name: u32,
    regions: MemoryRegions,
    memory_required: u32,
}

fn csr_to_config(hv: xous_tools::utils::CsrConfig, ram_config: &mut RamConfig) {
    let mut found_ram_name = None;
    fn round_mem(src: u32) -> u32 { (src + 4095) & !4095 }
    // Look for the largest "ram" block, which we'll treat as main memory
    for (k, v) in &hv.regions {
        if k.find("ram").is_some() && v.length > ram_config.size {
            ram_config.size = round_mem(v.length);
            ram_config.offset = v.start;
            found_ram_name = Some(k.clone());
        }
    }

    if found_ram_name.is_none() {
        eprintln!("Error: Couldn't find a memory region named \"ram\" in config file");
        return;
    }

    // Now that we know which block is ram, add the other regions.
    let found_ram_name = MemoryRegion::make_name(&found_ram_name.unwrap());
    for (k, v) in &hv.regions {
        ram_config.memory_required += round_mem(v.length) / 4096;
        let region_name = MemoryRegion::make_name(k);
        // Don't add the RAM section to the extra regions block.
        if region_name == found_ram_name {
            ram_config.name = region_name;
            continue;
        }
        // Don't add empty sections.
        if round_mem(v.length) == 0 {
            continue;
        }
        ram_config.regions.add(MemoryRegion::new(v.start, round_mem(v.length), region_name));
    }
}
fn create_image(init_programs: &[String]) -> std::io::Result<Vec<u8>> {
    env_logger::init();

    let mut ram_config = RamConfig {
        offset: Default::default(),
        size: Default::default(),
        name: MemoryRegion::make_name("sram"),
        regions: MemoryRegions::new(),
        memory_required: 0,
    };

    let mut process_names = ProcessNames::new();

    let soc_svd_file = include_bytes!("../../../utralib/renode/renode.svd");
    let desc = svd2utra::parse_svd(soc_svd_file.as_slice()).unwrap();
    let mut map = std::collections::BTreeMap::new();

    let mut csr_top = 0;
    for peripheral in desc.peripherals {
        if peripheral.base > csr_top {
            csr_top = peripheral.base;
        }
    }
    for region in desc.memory_regions {
        // Ignore the "CSR" region and manually reconstruct it, because this
        // region is largely empty and we want to avoid allocating too much space.
        if region.name == "CSR" {
            const PAGE_SIZE: usize = 4096;
            // round to the nearest page, then add one page as the last entry in the csr_top
            // is an alloatable page, and not an end-stop.
            let length = if csr_top - region.base & (PAGE_SIZE - 1) == 0 {
                csr_top - region.base
            } else {
                ((csr_top - region.base) & !(PAGE_SIZE - 1)) + PAGE_SIZE
            } + PAGE_SIZE;
            map.insert(
                region.name.to_lowercase(),
                xous_tools::utils::CsrMemoryRegion {
                    start: region.base.try_into().unwrap(),
                    length: length.try_into().unwrap(),
                },
            );
        } else {
            map.insert(
                region.name.to_lowercase(),
                xous_tools::utils::CsrMemoryRegion {
                    start: region.base.try_into().unwrap(),
                    length: region.size.try_into().unwrap(),
                },
            );
        }
    }
    csr_to_config(xous_tools::utils::CsrConfig { regions: map }, &mut ram_config);

    let mut args = XousArguments::new(ram_config.offset, ram_config.size, ram_config.name);

    if !ram_config.regions.is_empty() {
        args.add(ram_config.regions);
    }

    let kernel =
        process_program(include_bytes!("../../../target/riscv32imac-unknown-xous-elf/release/xous-kernel"))
            .expect("unable to read kernel");
    process_names.set(1, "kernel");

    let pid = 2;
    let init = process_minielf(include_bytes!(
        "../../../target/riscv32imac-unknown-xous-elf/release/xous-ticktimer"
    ))
    .expect("couldn't parse init file");
    args.add(IniE::new(init.entry_point, init.sections, init.program));
    process_names.set(pid, "xous-ticktimer");

    let pid = pid + 1;
    let init =
        process_minielf(include_bytes!("../../../target/riscv32imac-unknown-xous-elf/release/xous-log"))
            .expect("couldn't parse init file");
    args.add(IniE::new(init.entry_point, init.sections, init.program));
    process_names.set(pid, "xous-log");

    let pid = pid + 1;
    let init =
        process_minielf(include_bytes!("../../../target/riscv32imac-unknown-xous-elf/release/xous-names"))
            .expect("couldn't parse init file");
    args.add(IniE::new(init.entry_point, init.sections, init.program));
    process_names.set(pid, "xous-names");

    let pid = pid + 1;
    let init =
        process_minielf(include_bytes!("../../../target/riscv32imac-unknown-xous-elf/release/xous-susres"))
            .expect("couldn't parse init file");
    args.add(IniE::new(init.entry_point, init.sections, init.program));
    process_names.set(pid, "xous-susres");

    let mut pid = pid + 1;
    for init_path in init_programs {
        let program_name = std::path::Path::new(&init_path);
        println!("Adding {} to output", program_name.display());
        process_names.set(
            pid,
            program_name
                .file_stem()
                .expect("program had no name")
                .to_str()
                .expect("program name is not valid utf-8"),
        );
        let init = read_minielf(init_path).expect("couldn't parse init file");
        args.add(IniE::new(init.entry_point, init.sections, init.program));
        pid += 1;
    }

    let xkrn = XousKernel::new(
        kernel.text_offset,
        kernel.text_size,
        kernel.data_offset,
        kernel.data_size,
        kernel.bss_size,
        kernel.entry_point,
        kernel.program,
    );
    args.add(xkrn);

    args.add(process_names);

    // Add tags for init and kernel.  These point to the actual data, which should
    // immediately follow the tags.  Therefore, we must know the length of the tags
    // before we create them.
    let mut f = vec![];
    args.write(&mut f).expect("Couldn't write to args");

    println!("Arguments: {}", args);

    println!("Runtime will require {} bytes to track memory allocations", ram_config.memory_required);
    Ok(f)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: VecDeque<String> = std::env::args().collect();
    println!("Args: {:?}", args);
    let program = args
        .pop_front()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "program name not present"))?;
    let output_filename = args
        .pop_front()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "output filename not present"))?;
    args.make_contiguous();

    if args.len() == 0 {
        return Err("no init programs specified".into());
    }
    println!("Program: {}", program);
    println!("Output file: {}", output_filename);
    println!("Init args: {:?}", args);
    let xous_presign_img = create_image(args.as_slices().0)?;

    let xous_pkey = pem::parse(include_bytes!("../../../devkey/dev.key"))?;
    if xous_pkey.tag != "PRIVATE KEY" {
        println!("Xous image key was a {}, not a PRIVATE KEY", xous_pkey.tag);
        return Err("invalid xous image private key type".into());
    }
    println!("Signing output image");
    let xous_img = sign_image(&xous_presign_img, &xous_pkey, false, &None, Some([0u8; 16]))?;

    let mut output_file = File::create(&output_filename)?;
    output_file.write_all(&xous_img)?;

    println!("Successfully wrote {}", output_filename);

    Ok(())
}
