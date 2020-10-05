mod generate;
use generate::*;

use std::fs::File;

fn main() {
    println!("Generating using examples/soc.svd, output is example.rs (this stub is for testing only!)");
    println!("Use `cargo xtask ci <path-to-svdfile>` to generate a final SVD file.");
    let src = File::open("examples/soc.svd").unwrap();
    let mut dest = File::create("example.rs").unwrap();
    generate(src, &mut dest).unwrap();
}
