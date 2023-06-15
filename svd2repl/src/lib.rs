mod generate;
pub use generate::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{DirBuilder, File};
    #[test]
    fn basic_generate() {
        let src = File::open("examples/soc.svd").unwrap();
        DirBuilder::new().recursive(true).create("target").unwrap();
        let mut dest = File::create("target/example.repl").unwrap();
        generate(src, &mut dest).unwrap();
    }
}
