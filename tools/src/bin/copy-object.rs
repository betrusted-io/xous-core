use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process;

use tools::elf;

fn main() {
    env_logger::init();
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: {} input.elf [output.bin]", args.get(0).unwrap_or(&"copy-object".to_owned()));
        return;
    }

    let input_filename = Path::new(args.get(1).unwrap()).to_path_buf();
    let output_filename = args.get(2).map(|x| Path::new(x).to_path_buf()).unwrap_or_else(|| {
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
