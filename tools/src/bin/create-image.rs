#[macro_use]
extern crate clap;

extern crate crc;

use std::convert::TryInto;
use std::fs::File;

use tools::elf::{read_minielf, read_program};
use tools::tags::bflg::Bflg;
use tools::tags::inie::IniE;
use tools::tags::inif::IniF;
use tools::tags::memory::{MemoryRegion, MemoryRegions};
use tools::tags::pnam::ProcessNames;
use tools::tags::xkrn::XousKernel;
use tools::utils::{parse_csr_csv, parse_u32};
use tools::xous_arguments::XousArguments;

use clap::{App, Arg};

struct RamConfig {
    offset: u32,
    size: u32,
    name: u32,
    regions: MemoryRegions,
    memory_required: u32,
}

fn csr_to_config(hv: tools::utils::CsrConfig, ram_config: &mut RamConfig) {
    let mut found_ram_name = None;
    fn round_mem(src: u32) -> u32 {
        (src + 4095) & !4095
    }
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
        ram_config
            .regions
            .add(MemoryRegion::new(v.start, round_mem(v.length), region_name));
    }
}
fn main() {
    env_logger::init();
    let matches = App::new("Xous Image Creator")
        .version(crate_version!())
        .author("Sean Cross <sean@xobs.io>")
        .about("Create a boot image for Xous")
        .arg(
            Arg::with_name("kernel")
                .short("k")
                .long("kernel")
                .value_name("KERNEL_ELF")
                .takes_value(true)
                .required(true)
                .help("Kernel ELF image to bundle into the image"),
        )
        .arg(
            Arg::with_name("init")
                .short("i")
                .long("init")
                .takes_value(true)
                .multiple(true)
                .number_of_values(1)
                .help("Initial program to load"),
        )
        .arg(
            Arg::with_name("inif")
                .short("f")
                .long("inif")
                .takes_value(true)
                .multiple(true)
                .number_of_values(1)
                .help("Initial program to load from FLASH"),
        )
        .arg(
            Arg::with_name("csv")
                .short("c")
                .long("csv")
                .alias("csr-csv")
                .alias("csr")
                .value_name("CSR_CSV")
                .help("csr.csv file from litex")
                .takes_value(true)
                .required_unless_one(&["ram", "svd", "csv"]),
        )
        .arg(
            Arg::with_name("svd")
                .short("s")
                .long("svd")
                .value_name("SOC_SVD")
                .help("soc.csv file from litex")
                .takes_value(true)
                .required_unless_one(&["ram", "svd", "csv"]),
        )
        .arg(
            Arg::with_name("ram")
                .short("r")
                .long("ram")
                .takes_value(true)
                .value_name("OFFSET:SIZE")
                .help("RAM offset and size, in the form of [offset]:[size]")
                .required_unless_one(&["ram", "svd", "csv"]),
        )
        .arg(
            Arg::with_name("debug")
                .short("d")
                .long("debug")
                .takes_value(false)
                .help("Reduce kernel-userspace security and enable debugging programs"),
        )
        .arg(
            Arg::with_name("output")
                .value_name("OUTPUT")
                .required(true)
                .help("Output file to store tag and init information"),
        )
        .get_matches();

    let mut ram_config = RamConfig {
        offset: Default::default(),
        size: Default::default(),
        name: MemoryRegion::make_name("sram"),
        regions: MemoryRegions::new(),
        memory_required: 0,
    };

    let mut process_names = ProcessNames::new();

    if let Some(val) = matches.value_of("ram") {
        let ram_parts: Vec<&str> = val.split(':').collect();
        if ram_parts.len() != 2 {
            eprintln!("Error: --ram argument should be of the form [offset]:[size]");
            return;
        }

        ram_config.offset = match parse_u32(ram_parts[0]) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("Error: Unable to parse {}: {:?}", ram_parts[0], e);
                return;
            }
        };

        ram_config.size = match parse_u32(ram_parts[1]) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("Error: Unable to parse {}: {:?}", ram_parts[1], e);
                return;
            }
        };

        ram_config.memory_required += ram_config.size / 4096;
    }

    if let Some(csr_csv) = matches.value_of("csv") {
        let hv = parse_csr_csv(csr_csv).expect("couldn't find csr.csv file");
        csr_to_config(hv, &mut ram_config);
    }

    if let Some(soc_svd) = matches.value_of("svd") {
        let soc_svd_file = std::path::Path::new(soc_svd);
        let desc = svd2utra::parse_svd(soc_svd_file).unwrap();
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
                    tools::utils::CsrMemoryRegion {
                        start: region.base.try_into().unwrap(),
                        length: length
                            .try_into()
                            .unwrap(),
                    },
                );
            } else {
                map.insert(
                    region.name.to_lowercase(),
                    tools::utils::CsrMemoryRegion {
                        start: region.base.try_into().unwrap(),
                        length: region.size.try_into().unwrap(),
                    },
                );
            }
        }
        csr_to_config(tools::utils::CsrConfig { regions: map }, &mut ram_config);
    }

    let mut args = XousArguments::new(ram_config.offset, ram_config.size, ram_config.name);

    if !ram_config.regions.is_empty() {
        args.add(ram_config.regions);
    }

    if matches.is_present("debug") {
        args.add(Bflg::new().debug());
    }

    let kernel = read_program(
        matches
            .value_of("kernel")
            .expect("kernel was somehow missing"),
    )
    .expect("unable to read kernel");

    process_names.set(1, "kernel");
    let mut pid = 2;
    if let Some(init_paths) = matches.values_of("init") {
        for init_path in init_paths {
            let program_name = std::path::Path::new(init_path);
            process_names.set(
                pid,
                program_name
                    .file_stem()
                    .expect("program had no name")
                    .to_str()
                    .expect("program name is not valid utf-8"),
            );
            pid += 1;
            let init = read_minielf(init_path).expect("couldn't parse init file");
            args.add(IniE::new(init.entry_point, init.sections, init.program));
        }
    }

    if let Some(init_paths) = matches.values_of("inif") {
        for init_path in init_paths {
            let program_name = std::path::Path::new(init_path);
            process_names.set(
                pid,
                program_name
                    .file_stem()
                    .expect("program had no name")
                    .to_str()
                    .expect("program name is not valid utf-8"),
            );
            pid += 1;
            let init = read_minielf(init_path).expect("couldn't parse init file");
            args.add(IniF::new(init.entry_point, init.sections, init.program, init.alignment_offset));
        }
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

    let output_filename = matches
        .value_of("output")
        .expect("output filename not present");
    let f = File::create(output_filename)
        .unwrap_or_else(|_| panic!("Couldn't create output file {}", output_filename));
    args.write(&f).expect("Couldn't write to args");

    println!("Arguments: {}", args);

    println!(
        "Runtime will require {} bytes to track memory allocations",
        ram_config.memory_required
    );
    println!("Image created in file {}", output_filename);
}
