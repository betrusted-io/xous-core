use std::{
    path::PathBuf,
    io::Write,
    fs::{ self, File },
    env,
};
use structopt::StructOpt;
use clap::arg_enum;
use env_logger;
use log::*;
use uf2_util::{ convert_elf, convert_bin, Error };
use uf2_block::Block;

arg_enum! {
    #[derive(Debug, PartialEq)]
    enum InputType {
        Bin,
        Elf,
        Uf2,
    }
}

fn parse_hex_32(input: &str) -> Result<u32, std::num::ParseIntError> {
    if input.starts_with("0x") {
        u32::from_str_radix(&input[2..], 16)
    } else {
        input.parse::<u32>()
    }
}

#[derive(Debug, StructOpt)]
#[structopt(name = "uf2_util", about = "A utility for converting to & from UF2")]
struct Opt {
    #[structopt(parse(from_os_str))]
    input: PathBuf,

    #[structopt(parse(from_os_str))]
    output: Option<PathBuf>,

    #[structopt(short, long)]
    print: bool,

    #[structopt(short, long, default_value = "bin")]
    input_type: InputType,

    #[structopt(short, long, parse(try_from_str = parse_hex_32))]
    address: Option<u32>,

    #[structopt(short, long, default_value = "256")]
    block_size: u16,
}

fn main() -> Result<(), Error> {
    if env::var(env_logger::DEFAULT_FILTER_ENV).is_err() {
        // Set the default logging verbosity
        env::set_var(
            env_logger::DEFAULT_FILTER_ENV, 
            "info",
        );
    }
    env_logger::init();

    let opt = Opt::from_args();

    let block_size = opt.block_size;

    if opt.input_type == InputType::Bin && opt.address.is_none() {
        panic!("address must be provided if input_type is bin");
    }

    let out_path = opt.output.unwrap_or(opt.input.with_extension("uf2"));
    let data = fs::read(opt.input)?;

    info!("Type: {:?}", opt.input_type);
    info!("Output: {:?}", out_path);
    debug!("Base address: {:?}", opt.address);


    let bytes = match opt.input_type {
        InputType::Elf => convert_elf(&data, block_size)?,
        InputType::Bin => convert_bin(&data, block_size, opt.address.unwrap())?,
        InputType::Uf2 => data,
    };

    if opt.print {
        for c in bytes.chunks_exact(512) {
            let block = Block::parse(c)?;
            println!("{}", block);
        }
    } else {
        let mut out = File::create(out_path)?;
        out.write(&bytes)?;
    }

    Ok(())
}