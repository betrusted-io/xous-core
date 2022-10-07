mod app_manifest;
mod versioning;
use versioning::*;
mod utils;
use utils::*;
mod builder;
use builder::*;
mod verifier;
use verifier::*;

use std::env;

/// gitrev of the current precursor SoC version targeted by this build. This must
/// be manually updated every time the SoC version is bumped.
const PRECURSOR_SOC_VERSION: &str = "c809403";

/// This is the minimum Xous version required to read a PDDB backup generated
/// by the current kernel revision.
const MIN_XOUS_VERSION: &str = "v0.9.8-791";

/// target triple for precursor builds
const TARGET_TRIPLE: &str = "riscv32imac-unknown-xous-elf";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = Builder::new();
    // encodes a timestamp into the build, unless '--no-timestamp' is passed
    generate_version(env::args().filter(|x| x == "--no-timestamp").count() == 0);

    // A base set of packages. This is all you need for a normal
    // operating system that can run libstd
    let base_pkgs = [
        "xous-ticktimer",  // "well known" service: thread scheduling
        "xous-log",        // "well known" service: debug logging
        "xous-names",      // "well known" service: manage inter-server connection lookup
        "xous-susres",     // ticktimer registers with susres to coordinate time continuity across sleeps
    ].to_vec();
    // minimal set of packages to do bare-iron graphical I/O
    let gfx_base_pkgs = [
        &base_pkgs[..],
        &[
            "graphics-server",  // raw (unprotected) frame buffer primitives
            "keyboard",   // required by graphics-server
            "spinor",     // required by keyboard - to save key mapping
            "llio",       // required by spinor
        ]
    ].concat();
    // packages in the user image - most of the services at this layer have cross-dependencies
    let user_pkgs = [
        &gfx_base_pkgs[..],
        &[
            // net services
            "com",
            "net",
            "dns",
            // UX abstractions
            "gam",
            "ime-frontend",
            "ime-plugin-shell",
            "codec",
            "modals",
            // security
            "root-keys",
            "trng",
            "sha2",
            "engine-25519",
            "jtag",
            // GUI front end
            "status",
            "shellchat",
            // filesystem
            "pddb",
            // usb services
            "usb-device-xous",
        ]
    ].concat();
    // for fast testing of compilation targets of the PDDB to real hardware
    let pddb_dev_pkgs = [
        &base_pkgs[..],
        &[
            "pddb",
            "sha2",
        ],
    ].concat();
    // for fast checking of AES hardware accelerator
    let aestest_pkgs = ["ticktimer-server", "log-server", "aes-test"].to_vec();

    // packages located on crates.io. For testing non-local build configs that are less
    // concerned about software supply chain and more focused on developer convenience.
    let base_pkgs_remote = [
        "xous-ticktimer@0.1.5",   // "well known" service: thread scheduling
        "xous-log@0.1.3",         // "well known" service: debug logging
        "xous-names@0.9.11",       // "well known" service: manage inter-server connection lookup
        "xous-susres@0.1.6",      // ticktimer registers with susres to coordinate time continuity across sleeps
    ].to_vec();
    let xous_kernel_remote = "xous-kernel@0.9.5";

    // ---- extract position independent args ----
    let lkey = get_flag("--lkey")?;
    if lkey.len() != 0 {
        builder.loader_key_file(lkey[0].to_string());
    }
    let kkey = get_flag("--kkey")?;
    if kkey.len() != 0 {
        builder.kernel_key_file(kkey[0].to_string());
    }

    let extra_apps = get_flag("--app")?;
    builder.add_apps(&extra_apps);
    let extra_services = get_flag("--service")?;
    builder.add_services(&extra_services);

    // ---- now process the verb plus position dependent arguments ----
    let mut args = env::args();
    let task = args.nth(1);
    match task.as_deref() {
        Some("install-toolkit") | Some("install-toolchain") => {
            let arg = env::args().nth(2);
            ensure_compiler(
                &Some(TARGET_TRIPLE),
                true,
                arg.map(|x| x == "--force").unwrap_or(false),
            )?
        }
        // ----- renode configs --------
        Some("renode-image") => {
            builder.target_renode()
                   .add_services(&user_pkgs.into_iter().map(String::from).collect())
                   .add_apps(&get_cratespecs());
        }
        Some("renode-image-debug") => {
            builder.target_renode()
                   .add_services(&user_pkgs.into_iter().map(String::from).collect())
                   .stream(BuildStream::Debug)
                   .add_apps(&get_cratespecs());
        }
        Some("renode-test") => {
            builder.target_renode()
                   .add_services(&base_pkgs.into_iter().map(String::from).collect())
                   .add_services(&get_cratespecs());
        }
        Some("libstd-test") => {
            builder.target_renode()
                   .add_services(&base_pkgs.into_iter().map(String::from).collect())
                   .add_services(&get_cratespecs());
            builder.add_loader_feature("renode-bypass");
        }
        Some("libstd-net") => {
            builder.target_renode()
                   .add_services(&base_pkgs.into_iter().map(String::from).collect())
                   .add_services(&get_cratespecs());
            builder.add_loader_feature("renode-bypass")
                   .add_loader_feature("renode-minimal");
            builder.add_service("net")
                .add_service("com")
                .add_service("llio")
                .add_service("dns");
        }
        Some("renode-aes-test") => {
            builder.target_renode()
                   .add_services(&aestest_pkgs.into_iter().map(String::from).collect())
                   .add_services(&get_cratespecs());
        }
        Some("ffi-test") => {
            builder.target_renode()
                   .add_services(&gfx_base_pkgs.into_iter().map(String::from).collect())
                   .add_services(&get_cratespecs());
            builder.add_service("ffi-test");
            builder.add_loader_feature("renode-bypass");
        }
        Some("renode-remote") => {
            builder.target_renode()
                   .add_services(&base_pkgs_remote.into_iter().map(String::from).collect())
                   .use_kernel(xous_kernel_remote);
        }

        // ------- hosted mode configs -------
        Some("run") => {
            builder.target_hosted()
                   .add_services(&user_pkgs.into_iter().map(String::from).collect())
                   .add_feature("pddbtest")
                   .add_feature("ditherpunk")
                   .add_feature("tracking-alloc")
                   .add_feature("tls")
                   // .add_feature("test-rekey")
                   .add_apps(&get_cratespecs());
        }
        Some("pddb-ci") => {
            builder.target_hosted()
                   .add_services(&user_pkgs.into_iter().map(String::from).collect())
                   .add_feature("pddb/ci")
                   .add_feature("pddb/deterministic");
        }
        Some("pddb-btest") => {
            builder.target_hosted()
                   .add_services(&user_pkgs.into_iter().map(String::from).collect())
                   .add_feature("pddbtest")
                   .add_feature("autobasis")  // this will make secret basis tracking synthetic and automated for stress testing
                   .add_feature("autobasis-ci")
                   .add_feature("pddb/deterministic");
        }
        Some("hosted-debug") => {
            builder.target_hosted()
                   .add_services(&user_pkgs.into_iter().map(String::from).collect())
                   .add_feature("pddbtest")
                   .add_feature("ditherpunk")
                   .add_feature("tracking-alloc")
                   .add_feature("tls")
                   .stream(BuildStream::Debug)
                   .add_apps(&get_cratespecs());
        }
        Some("gfx-dev") => {
            builder.target_hosted()
                   .add_services(&gfx_base_pkgs.into_iter().map(String::from).collect())
                   .add_services(&get_cratespecs())
                   .add_feature("graphics-server/testing");
        },
        Some("hosted-ci") => {
            builder.target_hosted()
                   .add_services(&user_pkgs.into_iter().map(String::from).collect())
                   .hosted_build_only()
                   .add_apps(&get_cratespecs());
        }

        // ------ Precursor hardware image configs ------
        Some("app-image") => {
            builder.target_precursor(PRECURSOR_SOC_VERSION)
                   .add_services(&user_pkgs.into_iter().map(String::from).collect())
                   .add_apps(&get_cratespecs());
        }
        Some("perf-image") => {
            // note: to use this image, you need to load a version of the SOC that has the performance counters built in.
            // this can be generated using the command `python3 .\betrusted_soc.py -e .\dummy.nky --perfcounter` in the betrusted-soc repo.
            //
            // to read out performance monitoring data, use the `usb_update.py` script as follows:
            // ` python3 .\..\usb_update.py --dump v2p.txt --dump-file .\ring_aes_8.bin`
            // where the `v2p.txt` file contains a virtual to physical mapping that is generated by the `perflib` framework and
            // formatted in a fashion that can be automatically extracted by the usb_update script.
            builder.target_precursor("c809403-perflib")
                   .add_services(&user_pkgs.into_iter().map(String::from).collect())
                   .add_apps(&get_cratespecs())
                   .add_feature("perfcounter")
                   .add_kernel_feature("v2p");
        }
        Some("tts") => {
            builder.target_precursor(PRECURSOR_SOC_VERSION);

            let mut pkgs = user_pkgs.to_vec();
            pkgs.push("tts-frontend");
            pkgs.push("ime-plugin-tts");
            pkgs.retain(|&pkg| pkg != "ime-plugin-shell");

            builder.add_services(&pkgs.into_iter().map(String::from).collect())
                .add_apps(&get_cratespecs())
                .add_service("espeak-embedded#https://ci.betrusted.io/job/espeak-embedded/lastSuccessfulBuild/artifact/target/riscv32imac-unknown-xous-elf/release/espeak-embedded")
                .override_locale("en-tts")
                .add_feature("tts")
                .add_feature("braille");
        }
        Some("tiny") => {
            builder.target_precursor(PRECURSOR_SOC_VERSION)
                   .add_services(&base_pkgs.into_iter().map(String::from).collect())
                   .add_services(&get_cratespecs());
        }
        Some("usbdev") => {
            builder.target_precursor(PRECURSOR_SOC_VERSION)
                   .add_services(&gfx_base_pkgs.into_iter().map(String::from).collect())
                   .add_services(&get_cratespecs());
            builder.add_service("usb-test");
        }
        Some("pddb-dev") => {
            builder.target_precursor(PRECURSOR_SOC_VERSION)
                   .add_services(&pddb_dev_pkgs.into_iter().map(String::from).collect())
                   .add_services(&get_cratespecs());
        },
        Some("trng-test") => {
            builder.target_precursor(PRECURSOR_SOC_VERSION)
                   .add_services(&user_pkgs.into_iter().map(String::from).collect())
                   .add_feature("urandomtest");
        },
        Some("ro-test") => {
            builder.target_precursor(PRECURSOR_SOC_VERSION)
                   .add_services(&user_pkgs.into_iter().map(String::from).collect())
                   .add_feature("ringosctest");
        }
        Some("av-test") => {
            builder.target_precursor(PRECURSOR_SOC_VERSION)
                   .add_services(&user_pkgs.into_iter().map(String::from).collect())
                   .add_feature("avalanchetest");
        }

        // ---- other single-purpose commands ----
        Some("generate-locales") => generate_locales()?,
        Some("wycheproof-import") => whycheproof_import()?,
        _ => print_help(),
    }
    builder.build()?;

    // the intent of this call is to check that crates we are sourcing from crates.io
    // match the crates in our local source. The usual cause of an inconsistency is
    // a maintainer forgot to publish a change to crates.io.
    //
    // Note a key problem is that we don't check that the Cargo.toml files are correct,
    // because the manifest format is heavily modified on upload to crates.io.
    // This means that an attacker who controlls crates.io (or any part of the chain
    // from manifest upload to download) can freely modify dependencies, rendering
    // source code equivalence checking moot.
    //
    // this has to be called after the build because the crates need to be downloaded for
    // checking before you can check them!
    let do_verify = env::args().filter(|x| x == "--no-verify").count() == 0;
    if do_verify {
        check_project_consistency()
    } else {
        Ok(())
    }
}

fn print_help() {
    eprintln!(
"cargo xtask [verb] [cratespecs ..]
    [--feature [feature name]]
    [--lkey [loader key]] [--kkey [kernel key]]
    [--app [cratespec]]
    [--service [cratespec]]
    [--no-timestamp]
    [--no-verify]

[cratespecs] is a list of 0 or more items of the following syntax:
   [name]                crate 'name' to be built from local source
   [name@version]        crate 'name' to be fetched from crates.io at the specified version
   [name#URL]            pre-built binary crate of 'name' downloaded from a server at 'URL'
   [path-to-binary]      file path to a prebuilt binary image on local machine.
                         Files in '.' must be specified as './file' to avoid confusion with local source

The [cratespecs] list is treated as apps or services based on the context of [verb]. Additional crates can
be merged in with explicit app/service treatment with the following flags:
 [--app] [cratespec]     [cratespec] is treated as an additional app
 [--service] [cratespec] [cratespec] is treated as an additional service

[--lkey] and [--kkey]    Paths to alternate private key files for loader and kernel key signing (defaults to developer key)
[--no-timestamp]         Do not include a timestamp in the build. By default, `ticktimer` is rebuilt on every run to encode a timestamp.
[--no-verify]            Do not verify that local sources match crates.io downloaded sources

- An 'app' must be enumerated in apps/manifest.json.
   A pre-processor configures the launch menu based on the list of specified apps.
- A 'service' is merged into the device image without any pre-processing.

[verb] options:
Hardware images:
 app-image               Precursor user image. [cratespecs] are apps
 perf-image              Precursor user image, with performance profiling. [cratespecs] are apps
 tts                     builds an image with text to speech support via externally linked C executable. [cratespecs] are apps
 usbdev                  minimal, insecure build for new USB core bringup. [cratespecs] are services
 trng-test               automation framework for TRNG testing (CPRNG seeded by RO^AV). [cratespecs] ignored.
 ro-test                 automation framework for TRNG testing (RO directly, no CPRNG). [cratespecs] ignored.
 av-test                 automation framework for TRNG testing (AV dircetly, no CPRNG). [cratespecs] ignored.
 tiny                    Precursor tiny image. For testing with services built out-of-tree.

Hosted emulation:
 run                     Run user image in hosted mode with release flags. [cratespecs] are apps
 pddb-ci                 PDDB config for CI testing (eg: TRNG->deterministic for reproducible errors). [cratespecs] ignored.
 pddb-btest              PDDB stress tester for secret basis creation/deletion [cratespecs] ignored.
 hosted-debug            Run user image in hosted mode with debug flags. [cratespecs] are apps
 gfx-dev                 Testing mode for graphics primitves. [cratespecs] are services
 pddb-dev                Testing for compilation errors on hardware targets on the PDDB.

Renode emulation:
 renode-image            Renode user image. Unspecified [cratespecs] are apps
 renode-test             Renode test image. Unspecified [cratespecs] are services
 renode-image-debug      Renode user image with --debug flag set
 libstd-test             Renode test image that includes the minimum packages. [cratespecs] are services
                         Bypasses sig checks, keys locked out.
 libstd-net              Renode test image for testing network functions. Bypasses sig checks, keys locked out.
 ffi-test                builds an image for testing C-FFI bindings and integration. [cratespecs] are services
 renode-aes-test         Renode image for AES emulation development. Extremely minimal.
 renode-remote           Renode test image that pulls its crates from crates.io

Other commands:
 generate-locales        (re)generate the locales include for the language selected in xous-rs/src/locale.rs
 wycheproof-import       generate binary test vectors for engine-25519 from whycheproof-import/x25519.json
 install-toolkit         installs Xous toolkit with no prompt, useful in CI. Specify `--force` to remove existing toolchains

Note: By default, the `ticktimer` will get rebuilt every time. You can skip this by appending `--no-timestamp` to the command.
"
    )
}

type DynError = Box<dyn std::error::Error>;

enum MemorySpec {
    SvdFile(String),
}

/// [cratespecs] are positional arguments, and is a list of 0 to N tokens that immediately
/// follow [verb]
fn get_cratespecs() -> Vec<String> {
    let mut cratespecs = Vec::<String>::new();
    let mut args = env::args();
    args.nth(1); // skip the verb
    for arg in args {
        if arg.starts_with('-') {
            // stop processing the list as soon as first named argument is found
            break;
        }
        cratespecs.push(arg)
    }
    cratespecs
}

fn get_flag(flag: &str) -> Result<Vec<String>, DynError> {
    let mut list = Vec::<String>::new();
    let args = env::args();
    let mut flag_found = false;
    for arg in args {
        if arg == flag {
            flag_found = true;
            continue
        }
        if flag_found {
            if arg.starts_with('-') {
                eprintln!("Malformed arguments. Expected argument after flag {}, but found {}", flag, arg);
                return Err("Bad arguments".into());
            }
            list.push(arg);
            flag_found = false;
            continue
        }
    }
    Ok(list)
}
