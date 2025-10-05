#[macro_use]
extern crate clap;

extern crate crc;

use std::fs::File;

use clap::{App, Arg};
use tools::elf::read_minielf;
use tools::tags::inif::IniF;
use tools::tags::pnam::ProcessNames;
use tools::utils::parse_u32;
use tools::xous_arguments::XousArguments;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let matches = App::new("Xous Detached App Image Creator")
        .version(crate_version!())
        .author("Sean Cross <sean@xobs.io>")
        .author("bunnie <bunnie@baochip.com>")
        .about("Create a detached app image for Xous")
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
            Arg::with_name("output")
                .value_name("OUTPUT")
                .required(true)
                .help("Output file to store tag and init information"),
        )
        .arg(
            Arg::with_name("detached-offset")
                .long("detached-offset")
                .takes_value(true)
                .required(true)
                .help("Offset of a detached image from the main kernel image start")
                .value_name("Detached image offset"),
        )
        .get_matches();

    let mut process_names = ProcessNames::new();

    // There is no kernel in this image, so the RAM section has no meaning. Set to 0.
    let mut args = XousArguments::new(0, 0, 0);
    if let Some(offset) = matches.value_of("detached-offset") {
        // why the 0x1000 "fudge" on this constant? There's an extra 0x1000 of data inserted
        // by the ELF creator and I seem to remember it also on the primary xous.img but I can't
        // remember why.
        let offset_u32 = parse_u32(offset).map_err(|_| "Cant parse detached-offset")? - 0x1000;
        args.set_detached_offset(offset_u32);
    }

    let mut pid = 1;

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

    args.add(process_names);

    let output_filename = matches.value_of("output").expect("output filename not present");
    let f = File::create(output_filename)
        .unwrap_or_else(|_| panic!("Couldn't create output file {}", output_filename));
    args.write(&f).expect("Couldn't write to args");

    println!("Arguments: {}", args);
    println!("Image created in file {}", output_filename);
    Ok(())
}
