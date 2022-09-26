fn main() {
    #[cfg(feature="precursor-c809403")]
    let svd_filename = "precursor/soc-c809403.svd";
    #[cfg(feature="precursor-c809403-perflib")]
    let svd_filename = "precursor/soc-perf-c809403.svd";

    let svd_file_path = std::path::Path::new(&svd_filename);
    println!("cargo:rerun-if-changed={}", svd_file_path.canonicalize().unwrap().display());

    let src_file = std::fs::File::open(svd_filename).expect("couldn't open src file");
    let mut dest_file = std::fs::File::create("src/generated.rs").expect("couldn't open dest file");
    svd2utra::generate(src_file, &mut dest_file).unwrap();
}
