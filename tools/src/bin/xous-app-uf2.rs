#[macro_use]
extern crate clap;

extern crate crc;

use std::fs::File;
use std::io::{Cursor, Write};

use clap::{App, Arg};
use xous_tools::elf::read_minielf;
use xous_tools::sign_image::bin_to_uf2;
use xous_tools::swap_writer::SwapWriter;
use xous_tools::tags::inif::IniF;
use xous_tools::tags::inis::IniS;
use xous_tools::tags::pnam::ProcessNames;
use xous_tools::xous_arguments::XousArgumentCode;
use xous_tools::xous_arguments::XousArguments;

const DEV_KEY_PEM: &'static str = "-----BEGIN PRIVATE KEY-----\nMC4CAQAwBQYDK2VwBCIEIKindlyNoteThisIsADevKeyDontUseForProduction\n-----END PRIVATE KEY-----";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let matches = App::new("Xous Detached App UF2 Creator for Developer Images")
        .version(crate_version!())
        .author("bunnie <bunnie@baochip.com>")
        .about("Create a detached app image for Xous, signed for developer images, using the latest defaults")
        .arg(
            Arg::with_name("elf")
                .short("f")
                .long("elf")
                .takes_value(true)
                .multiple(true)
                .number_of_values(1)
                .required(true)
                .help("List of ELF files to incorporate in the detached app"),
        )
        .arg(
            Arg::with_name("swap")
                .long("swap")
                .takes_value(false)
                .help("When specified, creates a swap image"),
        )
        .get_matches();

    let mut process_names = ProcessNames::new();

    let mut pid: u32 = 1;

    let mut args = if matches.is_present("swap") {
        XousArguments::new(
            0,
            bao1x_api::offsets::baosec::SWAP_RAM_LEN as _,
            u32::from_le_bytes(*b"Swap") as XousArgumentCode,
        )
    } else {
        // There is no kernel in this image, so the RAM section has no meaning. Set to 0.
        let mut args = XousArguments::new(0, 0, 0);
        args.set_detached_offset(
            (bao1x_api::offsets::dabao::APP_RRAM_START - bao1x_api::offsets::KERNEL_START) as u32 - 0x1000,
        );
        args
    };
    if let Some(init_paths) = matches.values_of("elf") {
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
            if matches.is_present("swap") {
                args.add(IniS::new(init.entry_point, init.sections, init.program));
            } else {
                args.add(IniF::new(init.entry_point, init.sections, init.program, init.alignment_offset));
            }
        }
    }

    args.add(process_names);

    println!("Programs: {}", args);

    let private_key = pem::parse(DEV_KEY_PEM)?;

    if matches.is_present("swap") {
        let mut swap_buffer = SwapWriter::new();
        args.write(&mut swap_buffer)?;

        // Create the swap target image and encrypt swap_buffer to it
        let mut swap = Cursor::new(Vec::new());
        swap_buffer.encrypt_to(&mut swap, &private_key)?;

        // generate a uf2 file
        let swap_uf2 = "swap.uf2";
        let uf2_blob =
            bin_to_uf2(&swap.into_inner(), bao1x_api::BAOCHIP_1X_UF2_FAMILY, bao1x_api::SWAP_START_UF2 as _)?;
        let mut f =
            File::create(swap_uf2).unwrap_or_else(|_| panic!("Couldn't create output file {}", swap_uf2));
        f.write(&uf2_blob)?;
        println!("Created swap UF2 at {}", swap_uf2);
    } else {
        let mut source = Cursor::new(Vec::new());
        args.write(&mut source).expect("Couldn't write out ELF files");

        let result = xous_tools::sign_image::sign_image(
            &source.get_ref(),
            &private_key,
            false,
            &None,
            None,
            true,
            bao1x_api::signatures::SIGBLOCK_LEN,
            xous_tools::sign_image::Version::Bao1xV1,
            Some("app"),
        )?;

        let app_uf2 = "apps.uf2";
        let uf2_blob =
            bin_to_uf2(&result, bao1x_api::BAOCHIP_1X_UF2_FAMILY, bao1x_api::dabao::APP_RRAM_START as _)?;
        let mut f =
            File::create(app_uf2).unwrap_or_else(|_| panic!("Couldn't create output file {}", app_uf2));
        f.write(&uf2_blob)?;
        println!("Created app UF2 at {}", app_uf2);
    }

    Ok(())
}
