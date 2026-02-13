#[macro_use]
extern crate clap;

extern crate crc;

use std::fs::File;
use std::io::{Cursor, Write};
use std::str::FromStr;

use clap::{App, Arg};
use xous_semver::SemVer;
use xous_tools::elf::read_minielf;
use xous_tools::sign_image::bin_to_uf2;
use xous_tools::swap_writer::SwapWriter;
use xous_tools::tags::inif::IniF;
use xous_tools::tags::inis::IniS;
use xous_tools::tags::pnam::ProcessNames;
use xous_tools::utils::parse_u32;
use xous_tools::xous_arguments::XousArgumentCode;
use xous_tools::xous_arguments::XousArguments;

// this must match exactly what's in devkey/dev.key
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
            Arg::with_name("antirollback")
            .long("antirollback")
            .takes_value(true)
            .help("Anti-rollback number. Must be greater than or equal to the current anti-rollback number on the target system.")
            // 1 is probably reasonable default value, as for out-of-tree builds I don't see any scenario where for developers
            // the anti-rollback counter would be incremented. Security-conscious developers would care to see that the kernel
            // version is intimately linked to the app version; if you're preferring to do out of tree builds then you've
            // YOLO'd on security, anyways. The place where this might need to be notched up is if someone had a device
            // once upon a time used for secure things, and then they decided to give it a new life as a toy, in which case,
            // the anti-rollback counter could be incremented and they'd have to increment this to match the current ARB state.
            .default_value("1")
        )
        .arg(
            Arg::with_name("swap")
                .long("swap")
                .takes_value(false)
                .help("When specified, creates a swap image"),
        )
        .arg(
            Arg::with_name("git-rev")
                .long("git-rev")
                .takes_value(true)
                .required(false)
                .help("Explicit git commit hash for swap nonce (e.g., '0d934e1...'). If not specified, uses git rev-parse HEAD."),
        )
        .arg(
            Arg::with_name("git-describe")
                .long("git-describe")
                .takes_value(true)
                .required(false)
                .help("Explicit git describe version for swap signing (e.g., 'v0.10.0-19-g0d934e1'). If not specified, uses git describe."),
        )
        .get_matches();

    let mut process_names = ProcessNames::new();

    let mut pid: u32 = 1;

    let anti_rollback = if let Some(arb) = matches.value_of("antirollback") {
        parse_u32(arb).map_err(|_| String::from("Antirollback should be a number"))?
    } else {
        1
    };
    // set this to a...somewhat conservative value. Hopefully we're not deprecating 500 firmware
    // versions ever, but also much less than the wear-out threshold of the counter. This helps
    // prevent someone "just poking around" from configuring this value to something harmful
    // to the hardware.
    if anti_rollback > 500 {
        panic!("Antirollback value of {} is too large, refusing to sign", anti_rollback);
    }

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

    let git_rev = matches.value_of("git-rev");
    let semver: Option<[u8; 16]> = if let Some(git_describe_str) = matches.value_of("git-describe") {
        Some(
            git_describe_str
                .parse::<SemVer>()
                .expect("git-describe format incorrect")
                .into(),
        )
    } else {
        None
    };

    if matches.is_present("swap") {
        let mut swap_buffer = SwapWriter::new();
        args.write(&mut swap_buffer)?;

        // Create the swap target image and encrypt swap_buffer to it
        let mut swap = Cursor::new(Vec::new());
        swap_buffer.encrypt_to(&mut swap, &private_key, Some(anti_rollback as usize), git_rev, semver)?;

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

        // whack a dummy version into the image. This will cause version compatibility checks to fail,
        // if and when they happen. The alternatives are:
        //  - force the user to specify a version. I think this annoys the user and they probably won't get it
        //    right
        //  - automatically resolve the version from the web. A commit
        //    e41195e3495cb27b447f0aec897c8928f29ced9d temporarily pulls in the code to do this, but the
        //    problem is that querying github to automatically resolve versions pulls in a *huge* attack
        //    surface, and also builds now try to tickle network resources which feels distasteful to me and
        //    not the right direction to go.
        //
        // So between two bad options, the decision for now is to do nothing and leave this field at 0, which
        // as of now doesn't cause trouble but if in the future we want to do something where a kernel checks
        // the version of a binary to make sure it's compatible before running it - it'll break.
        let version = "v0.0.0".to_string();
        let semver: [u8; 16] = SemVer::from_str(&version)?.into();

        let result = xous_tools::sign_image::sign_image(
            &source.get_ref(),
            &private_key,
            false,
            &None,
            Some(semver),
            true,
            bao1x_api::signatures::SIGBLOCK_LEN,
            xous_tools::sign_image::Version::Bao1xV1,
            Some("app"),
            Some(anti_rollback as usize),
            false,
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
