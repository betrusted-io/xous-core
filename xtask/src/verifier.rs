use std::env;
use crate::builder::CrateSpec;
use std::path::{Path, PathBuf};

use crate::DynError;

pub fn verify(spec: CrateSpec) -> Result<(), DynError> {
    let mut cache_path = Path::new(&env::var("CARGO_HOME").unwrap()).to_path_buf();
    cache_path.push("registry");
    cache_path.push("src");
    for entry in std::fs::read_dir(cache_path)? {
        let entry = entry?;
        let path = entry.path();
        println!("entries: {}", path.to_str().unwrap());
    }

    Ok(())
}