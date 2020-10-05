mod generate;
use generate::*;

use std::fs::File;

fn main() {
    let src = File::open("examples/soc.svd").unwrap();
    let mut dest = File::create("example.rs").unwrap();
    generate(src, &mut dest).unwrap();
}
