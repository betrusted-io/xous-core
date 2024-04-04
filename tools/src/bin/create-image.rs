#[macro_use]
extern crate clap;

extern crate crc;

use std::convert::TryInto;
use std::fs::File;

use clap::{App, Arg};
use tools::elf::{read_minielf, read_program};
use tools::swap_writer::SwapWriter;
use tools::tags::bflg::Bflg;
use tools::tags::inie::IniE;
use tools::tags::inif::IniF;
use tools::tags::inis::IniS;
use tools::tags::memory::{MemoryRegion, MemoryRegions};
use tools::tags::pnam::ProcessNames;
use tools::tags::swap::Swap;
use tools::tags::xkrn::XousKernel;
use tools::utils::{parse_csr_csv, parse_u32};
use tools::xous_arguments::XousArguments;

struct RamConfig {
    offset: u32,
    size: u32,
    name: u32,
    regions: MemoryRegions,
    memory_required: u32,
}

fn csr_to_config(hv: tools::utils::CsrConfig, ram_config: &mut RamConfig) {
    let mut found_ram_name = None;
    fn round_mem(src: u32) -> u32 { (src + 4095) & !4095 }
    // Look for the largest memory block, which we'll treat as main memory
    for (k, v) in &hv.regions {
        if (
            k.find("sram").is_some()  // uniquely finds region on Precursor and Cramium (excludes 'reram' correctly)
            || k.find("ddr_ram").is_some()
            // finds region on atsama5d27 uniquely
        ) && v.length > ram_config.size
        {
            ram_config.size = round_mem(v.length);
            ram_config.offset = v.start;
            found_ram_name = Some(k.clone());
        }
    }

    if found_ram_name.is_none() {
        eprintln!("Error: Couldn't find a memory region named \"sram\" in config file");
        return;
    }

    // Now that we know which block is ram, add the other regions.
    let found_ram_name = MemoryRegion::make_name(&found_ram_name.unwrap());
    let mut raw_regions = Vec::<MemoryRegion>::new();
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
        raw_regions.push(MemoryRegion::new(v.start, round_mem(v.length), region_name));
    }
    // condense adjacent regions & eliminate overlapping regions
    raw_regions.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
    let mut candidate_region = raw_regions[0];
    for r in raw_regions[1..].iter() {
        if r.start > candidate_region.start + candidate_region.length {
            ram_config.regions.add(candidate_region);
            candidate_region = r.to_owned();
        } else {
            candidate_region.length = (r.start + r.length) - candidate_region.start;
        }
    }
    ram_config.regions.add(candidate_region);
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
            Arg::with_name("inis")
                .long("inis")
                .takes_value(true)
                .multiple(true)
                .number_of_values(1)
                .help("Program to be loaded into swap space"),
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
                .help("soc.svd file from litex (system-level)")
                .takes_value(true)
                .required_unless_one(&["ram", "svd", "csv"]),
        )
        .arg(
            Arg::with_name("extra-svd")
                .long("extra-svd")
                .value_name("EXTRA_SVD")
                .help("extra SVD files")
                .takes_value(true)
                .multiple(true)
                .number_of_values(1),
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
            Arg::with_name("swap")
                .long("swap")
                .takes_value(true)
                .value_name("OFFSET:SIZE")
                .help("Swap offset and size, in the form of [offset]:[size]; note: offset and size have platform-dependent interpretations")
        )
        .arg(
            Arg::with_name("swap-name")
                .long("swap-name")
                .takes_value(true)
                .value_name("OUTPUT")
                .help("Output file to store swap image data")
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

    let mut swap: Option<Swap> = None;

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
        let svd_file = std::fs::File::open(soc_svd_file).unwrap();
        let desc = svd2utra::parse_svd(vec![svd_file]).unwrap();
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
                const PAGE_SIZE: u64 = 4096;
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
                        length: length.try_into().unwrap(),
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
        // insert additional regions from core.svd, if specified
        if matches.is_present("extra-svd") {
            let files: Vec<_> = matches.values_of("extra-svd").unwrap().collect();
            for soc_svd in files {
                let soc_svd_file = std::path::Path::new(soc_svd);
                let svd_file = std::fs::File::open(soc_svd_file).unwrap();
                let desc = svd2utra::parse_svd(vec![svd_file]).unwrap();

                let mut csr_top = 0;
                for peripheral in desc.peripherals {
                    if peripheral.base > csr_top {
                        csr_top = peripheral.base;
                    }
                }
                for mut region in desc.memory_regions {
                    loop {
                        if map.contains_key(&region.name.to_lowercase()) {
                            let mut new_name = region.name.to_lowercase();
                            new_name.push_str("_");
                            region.name = new_name;
                            continue;
                        }
                        break;
                    }
                    println!("{}: {:x}", region.name, region.base);
                    map.insert(
                        region.name.to_lowercase(),
                        tools::utils::CsrMemoryRegion {
                            start: region.base.try_into().unwrap(),
                            length: region.size.try_into().unwrap(),
                        },
                    );
                }
            }
        }

        csr_to_config(tools::utils::CsrConfig { regions: map }, &mut ram_config);
    }

    // Swap has an architecture-dependent meaning.
    //
    // On Precursor, it's only used exclusively for testing, so we apply it as a
    // "patch" on top of the RAM area -- it's a specifier for what part
    // of RAM should be carved out to use as swap.
    //
    // On Cramium, the swap region may point to a section of memory-mapped RAM, *or*
    // it can point to register-mapped SPI RAM. The distinction is based solely upon
    // the starting address. If the starting address is `0`, we assume this is talking
    // about register-mapped SPI RAM. If it is non-zero, then we assume this is referring
    // to a window that is memory-mapped.
    if let Some(val) = matches.value_of("swap") {
        let swap_parts: Vec<&str> = val.split(':').collect();
        if swap_parts.len() != 2 {
            eprintln!("Error: --swap argument should be of the form [offset]:[size]");
            return;
        }

        let offset = match parse_u32(swap_parts[0]) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("Error: Unable to parse {}: {:?}", swap_parts[0], e);
                return;
            }
        };

        let size = match parse_u32(swap_parts[1]) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("Error: Unable to parse {}: {:?}", swap_parts[1], e);
                return;
            }
        };

        swap = Some(Swap::new(offset, size));
    }

    let mut args = XousArguments::new(ram_config.offset, ram_config.size, ram_config.name);
    let mut swap_args: Option<XousArguments> = None;

    if !ram_config.regions.is_empty() {
        if let Some(s) = swap {
            // now process the swap region.
            #[cfg(any(feature = "precursor", feature = "renode"))]
            {
                // this is slightly janky, but we only use this configuration for testing swap
                // so we can impose an artificial rule like "swap must be higher than RAM", instead
                // of trying to handle the generic cases like "swap could be anywhere, maybe even
                // a window inside RAM, or lower, or multiple fragments, or...".
                assert!(s.offset > ram_config.offset, "swap is assumed to be at a higher address than RAM");
                // split the RAM space, if necessary, to accommodate swap
                if s.offset < ram_config.offset + ram_config.size {
                    ram_config.size = s.offset - ram_config.offset;
                }
            }
            // Note that other configurations don't split RAM, since the swap is provisioned directly
            // in hardware, and thus, no post-processing is required.
            swap_args = Some(XousArguments::new(s.offset, s.size, s.name));

            args.add(s);
            args.add(ram_config.regions);
        } else {
            args.add(ram_config.regions);
        }
    }

    if matches.is_present("debug") {
        args.add(Bflg::new().debug());
    }

    let kernel = read_program(matches.value_of("kernel").expect("kernel was somehow missing"))
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

    if let Some(init_paths) = matches.values_of("inis") {
        if let Some(ref mut sargs) = swap_args {
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
                sargs.add(IniS::new(init.entry_point, init.sections, init.program, init.alignment_offset));
            }
        } else {
            println!("Warning: inis regions specified, but no swap region specified. Ignoring inis regions!");
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

    let output_filename = matches.value_of("output").expect("output filename not present");
    let f = File::create(output_filename)
        .unwrap_or_else(|_| panic!("Couldn't create output file {}", output_filename));
    args.write(&f).expect("Couldn't write to args");

    println!("Arguments: {}", args);

    if let Some(mut sargs) = swap_args {
        let mut swap_buffer = SwapWriter::new();
        sargs.write(&mut swap_buffer).expect("Couldn't write out swap args");

        let swap_filename = matches.value_of("swap-name").expect("swap filename not present");
        let sf = File::create(swap_filename)
            .unwrap_or_else(|_| panic!("Couldn't create output file {}", swap_filename));
        swap_buffer.encrypt_to(sf).expect("Couldn't flush swap buffer to disk");

        println!("Swap arguments: {}", sargs);
        println!("Swap data created in file {}", swap_filename);
    }
    println!("Runtime will require {} bytes to track memory allocations", ram_config.memory_required);
    if let Some(s) = swap {
        println!("Runtime will also require {} bytes to track swap", s.size / 4096);
    }
    println!("Image created in file {}", output_filename);
}
