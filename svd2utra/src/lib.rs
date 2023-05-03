mod generate;
pub use generate::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{DirBuilder, File};
    #[test]
    fn basic_generate() {
        DirBuilder::new().recursive(true).create("target").unwrap();
        let mut dest = File::create("target/example.rs").unwrap();
        generate("examples/soc.svd", &mut dest).unwrap();
    }
}
