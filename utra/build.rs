use std::env;

fn main() {
    let svd_file = env::var("XOUS_SVD_FILE").unwrap_or("../emulation/example.svd".to_owned());
    println!("cargo:rerun-if-env-changed=XOUS_SVD_FILE");
    println!("cargo:rerun-if-env-changed={}", svd_file);

    let src_file = std::fs::File::open(svd_file).expect("couldn't open src file");
    let mut dest_file = std::fs::File::create("src/generated.rs").expect("couldn't open dest file");
    svd2utra::generate(src_file, &mut dest_file).unwrap();
}
