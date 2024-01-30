mod generate;
pub use generate::*;

#[cfg(test)]
mod tests {
    use std::fs::{DirBuilder, File};

    use super::*;
    #[test]
    fn basic_generate() {
        let src = File::open("examples/soc.svd").unwrap();
        DirBuilder::new().recursive(true).create("target").unwrap();
        let mut dest = File::create("target/example.rs").unwrap();
        generate(src, &mut dest).unwrap();
    }
}
