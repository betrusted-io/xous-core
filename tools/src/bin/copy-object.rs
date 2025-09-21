use std::env;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;

use tools::elf;

fn main() {
    env_logger::init();
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!(
            "Usage: {} input.elf [output.bin] [--bao1x]",
            args.get(0).unwrap_or(&"copy-object".to_owned())
        );
        return;
    }

    let input_filename = Path::new(args.get(1).unwrap()).to_path_buf();

    // Parse remaining arguments
    let mut output_filename: Option<PathBuf> = None;
    let mut bao1x_flag = false;

    for arg in args.iter().skip(2) {
        if arg == "--bao1x" {
            bao1x_flag = true;
        } else if output_filename.is_none() {
            output_filename = Some(Path::new(arg).to_path_buf());
        }
    }

    // Set default output filename if none provided
    let output_filename = output_filename.unwrap_or_else(|| {
        let mut output_filename = input_filename.clone();
        output_filename.set_extension("bin");
        output_filename
    });

    if output_filename == input_filename {
        eprintln!("Input and output filename are the same: {}", output_filename.display());
        eprintln!("Specify an output path, or change the suffix of your input file from \".bin\"");
        process::exit(1);
    }
    let pd = elf::read_loader(&input_filename).unwrap_or_else(|e| {
        eprintln!("Unable to read input file: {}", e);
        process::exit(1);
    });
    let mut f = File::create(&output_filename).unwrap_or_else(|e| {
        eprintln!("Couldn't create output file {}: {}", output_filename.display(), e);
        process::exit(1);
    });
    if bao1x_flag {
        let mut statics = bao1x_api::StaticsInRom {
            jump_instruction: bao1x_api::JUMP_INSTRUCTION,
            version: bao1x_api::STATICS_IN_ROM_VERSION,
            valid_pokes: 0,
            data_origin: 0,
            data_size_bytes: 0,
            poke_table: [(0u32, 0u32); 30],
        };
        statics.data_origin = pd.data_offset;
        statics.data_size_bytes = pd.clear_size;
        statics.valid_pokes = pd.poke_table.len() as u16;
        for (&entry, dest) in pd.poke_table.iter().zip(statics.poke_table.iter_mut()) {
            *dest = entry;
        }
        f.write(statics.as_bytes()).unwrap_or_else(|e| {
            eprintln!("Couldn't write data to {}: {}", output_filename.display(), e);
            process::exit(1);
        });
    }
    f.write_all(&pd.program).unwrap_or_else(|e| {
        eprintln!("Couldn't write data to {}: {}", output_filename.display(), e);
        process::exit(1);
    });

    println!("Data offset: {:08x}", pd.data_offset);
    println!("Data size: {}", pd.data_size);
    println!("Text offset: {:08x}", pd.text_offset);
    println!("Entrypoint: {:08x}", pd.entry_point);
    println!("Copied {} bytes of data to {}", pd.program.len(), output_filename.display());
}
