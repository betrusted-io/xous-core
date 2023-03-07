use std::env;
use std::fs::OpenOptions;
use std::io::Write;
#[cfg(not(feature = "hosted"))]
use std::io::Read;
use std::path::PathBuf;

fn out_dir() -> PathBuf {
    PathBuf::from(env::var_os("OUT_DIR").unwrap())
}

/// Helper macro that returns a constant number of features enabled among specified list.
macro_rules! count_enabled_features {
    ($($feature:literal),*) => {
        {
            let mut enabled_features = 0;
            $(
                enabled_features += cfg!(feature = $feature) as u32;
            )*
            enabled_features
        }
    }
}

/// Helper macro that returns a compile-time error if multiple or none of the
/// features of some category are defined.
///
/// # Example
///
/// Given the following code:
///
/// ```
/// allow_single_feature!("feature-category-name", "a", "b", "c");
/// ```
///
/// These runs fail compilation check:
/// $ cargo check --features a,b # error msg: 'Multiple feature-category-name specified. Only one is allowed.
/// $ cargo check # error msg: 'None of the feature-category-name specified. Pick one.'
///
/// This compiles:
/// $ cargo check --feature a
macro_rules! allow_single_feature {
    ($name:literal, $($feature:literal),*) => {
        const _: () = {
            const MSG_MULTIPLE: &str = concat!("\nMultiple ", $name, " specified. Only one is allowed.");
            const MSG_NONE: &str = concat!("\nNone of the ", $name, " specified. Pick one.");

            match count_enabled_features!($($feature),*) {
                0 => std::panic!("{}", MSG_NONE),
                1 => {}
                2.. => std::panic!("{}", MSG_MULTIPLE),
            }
        };
    }
}

macro_rules! allow_single_target_feature {
    ($($args:tt)+) => {
        allow_single_feature!("targets", $($args)+);
    }
}

#[cfg(feature = "precursor")] // Gitrevs are only relevant for Precursor target
macro_rules! allow_single_gitrev_feature {
    ($($args:tt)+) => {
        allow_single_feature!("gitrevs", $($args)+);
    }
}

fn main() {
    // ------ check that the feature flags are sane -----
    // note on selecting "hosted" mode. An explicit "hosted" flag is provided to clarify
    // the build system's intent. However, in general, most packages prefer to use this idiom:
    //
    // #[cfg(not(target_os = "xous"))]
    //
    // This flag is synonymous with feature = "hosted", and it also makes "hosted" mode the
    // "default" package in the case that the code is being built in CI or in external
    // packages that don't know to configure the "hosted" feature flag.
    //
    // This idiom breaks if Xous ever gets to the point of compiling and running code on
    // its own platform; but generally, if the target binary is running on e.g. windows/linux/non-xous
    // target triples, the user's intent was "hosted" mode.
    //
    // This script retains the use of an explicit "hosted" flag because we want to catch
    // unintentional build system misconfigurations that meant to build for a target other
    // than "hosted", rather than just falling back silently to defaults.
    allow_single_target_feature!("precursor", "hosted", "renode", "atsama5d27");

    #[cfg(feature = "precursor")]
    allow_single_gitrev_feature!(
        "precursor-perflib",
        "precursor-dvt",
        "precursor-pvt"
    );

    // ----- select an SVD file based on a specific revision -----
    #[cfg(feature = "precursor-perflib")]
    let svd_filename = "precursor/soc-perf.svd";
    #[cfg(feature = "precursor-perflib")]
    let generated_filename = "src/generated/precursor_perf.rs";

    #[cfg(feature = "renode")]
    let svd_filename = "renode/renode.svd";
    #[cfg(feature = "renode")]
    let generated_filename = "src/generated/renode.rs";

    #[cfg(feature = "precursor-dvt")]
    let svd_filename = "precursor/soc-dvt.svd";
    #[cfg(feature = "precursor-dvt")]
    let generated_filename = "src/generated/precursor_dvt.rs";

    #[cfg(feature = "precursor-pvt")]
    let svd_filename = "precursor/soc-pvt.svd";
    #[cfg(feature = "precursor")]
    let generated_filename = "src/generated/precursor_pvt.rs";

    #[cfg(feature = "atsama5d27")]
    let svd_filename = "atsama5d/ATSAMA5D27.svd";
    #[cfg(feature = "atsama5d27")]
    let generated_filename = "src/generated/atsama5d27.rs";

    // ----- control file generation and rebuild sequence -----
    // check and see if the configuration has changed since the last build. This should be
    // passed by the build system (e.g. xtask) if the feature is used.
    //
    // Debug this using:
    //  $env:CARGO_LOG="cargo::core::compiler::fingerprint=info"
    #[cfg(not(feature = "hosted"))]
    {
        let svd_file_path = std::path::Path::new(&svd_filename);
        println!(
            "cargo:rerun-if-changed={}",
            svd_file_path.canonicalize().unwrap().display()
        );

        // Regenerate the utra file in RAM.
        let src_file = std::fs::File::open(svd_filename).expect("couldn't open src file");
        let mut dest_vec = vec![];
        svd2utra::generate(src_file, &mut dest_vec).unwrap();

        // If the file exists, check to see if it is different from what we just generated.
        // If not, skip writing the new file.
        // If the file doesn't exist, or if it's different, write out a new utra file.
        let should_write = if let Ok(mut existing_file) = std::fs::File::open(generated_filename) {
            let mut existing_file_contents = vec![];
            existing_file.read_to_end(&mut existing_file_contents).expect("couldn't read existing utra generated file");
            existing_file_contents != dest_vec
        } else {
            true
        };
        if should_write {
            let mut dest_file =
                std::fs::File::create(generated_filename).expect("couldn't open dest file");
            dest_file
                .write_all(&dest_vec)
                .expect("couldn't write contents to utra file");
        }

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
    #[cfg(feature = "hosted")]
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
