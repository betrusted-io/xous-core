use std::path::PathBuf;
use std::env;
use std::fs::OpenOptions;
use std::io::Write;

fn out_dir() -> PathBuf {
    PathBuf::from(env::var_os("OUT_DIR").unwrap())
}

fn main() {
    // ------ check that the feature flags are sane -----
    #[cfg(
        all(feature="precursor",
            not(any(
                    feature="precursor-c809403",
                    feature="precursor-c809403-perflib"
                )
            )
        )
    )]
    panic!("Precursor target specified, but no corresponding gitrev specified");

    // this list grows O(N) as we add more targets. :-/ I don't know of a better way
    // to express "only one of N should be selected" using cfg syntax
    #[cfg(all(
        feature="precursor",
        any(feature="hosted", feature="renode")
    ))]
    panic!("Multiple targets specified. This is disallowed");
    #[cfg(all(
        feature="hosted",
        any(feature="precursor", feature="renode")
    ))]
    panic!("Multiple targets specified. This is disallowed");
    #[cfg(all(
        feature="renode",
        any(feature="precursor", feature="hosted")
    ))]
    panic!("Multiple targets specified. This is disallowed");

    // also grows O(N) as we add more gitrevs
    #[cfg(all(
        feature="precursor-c809403",
        feature="precursor-c809403-perflib"
    ))]
    panic!("Multiple gitrevs specified for Precursor target. This is disallowed");

    // ----- select an SVD file based on a specific revision -----
    #[cfg(feature="precursor-c809403")]
    let svd_filename = "precursor/soc-c809403.svd";
    #[cfg(feature="precursor-c809403")]
    let generated_filename = "src/generated/precursor_c809403.rs";

    #[cfg(feature="precursor-c809403-perflib")]
    let svd_filename = "precursor/soc-perf-c809403.svd";
    #[cfg(feature="precursor-c809403-perflib")]
    let generated_filename = "src/generated/precursor_perf_c809403.rs";

    #[cfg(feature="renode")]
    let svd_filename = "renode/renode.svd";
    #[cfg(feature="renode")]
    let generated_filename = "src/generated/renode.rs";

    // ----- control file generation and rebuild sequence -----
    // check and see if the configuration has changed since the last build. This should be
    // passed by the build system (e.g. xtask) if the feature is used.
    #[cfg(not(feature="hosted"))]
    {
        let last_config = out_dir().join("../../LAST_CONFIG");
        if last_config.exists() {
            println!("cargo:rerun-if-changed={}", last_config.canonicalize().unwrap().display());
        }
        let svd_file_path = std::path::Path::new(&svd_filename);
        println!("cargo:rerun-if-changed={}", svd_file_path.canonicalize().unwrap().display());

        let src_file = std::fs::File::open(svd_filename).expect("couldn't open src file");
        let mut dest_file = std::fs::File::create(generated_filename).expect("couldn't open dest file");
        svd2utra::generate(src_file, &mut dest_file).unwrap();

        // ----- feedback SVD path to build framework -----
        // pass the computed SVD filename back to the build system, so that we can pass this
        // on to the image creation program. This is necessary so we can extract all the memory
        // regions and create the whitelist of memory pages allowed to the kernel; any page not
        // explicitly used by the hardware model is ineligible for mapping and allocation by any
        // process. This helps to prevent memory aliasing attacks by hardware blocks that partially
        // decode their addresses (this would be in anticipation of potential hardware bugs; ideally
        // this isn't ever a problem).
        let svd_path = out_dir().join("../../SVD_PATH");
        let mut svd_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(svd_path)
            .unwrap();
        write!(svd_file, "utralib/{}", svd_filename).unwrap();
    }
    #[cfg(feature="hosted")]
    {
        let svd_path = out_dir().join("../../SVD_PATH");
        let mut svd_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(svd_path)
            .unwrap();
        write!(svd_file, "").unwrap(); // there is no SVD file for hosted mode
    }
}
