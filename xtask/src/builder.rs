use std::fs::OpenOptions;
use std::{
    env,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};
use crate::{DynError, TARGET_TRIPLE, app_manifest::generate_app_menus, MemorySpec};

#[allow(dead_code)]
#[derive(Copy, Clone, Debug)]
pub(crate) enum BuildStream {
    Debug,
    Release
}
impl BuildStream {
    pub fn to_str(&self) -> &str {
        match self {
            BuildStream::Debug => "debug",
            BuildStream::Release => "release",
        }
    }
}

pub enum CrateSpec {
    /// name of the crate
    Local(String),
    /// crates.io: (name of crate, version)
    CratesIo(String, String),
    /// a prebuilt package: (name of executable, URL for download)
    Prebuilt(String, String),
    /// a prebuilt binary, done using command line tools
    BinaryFile(String),
    /// an empty entry
    None,
}
impl Clone for CrateSpec {
    fn clone(&self) -> CrateSpec {
        match self {
            CrateSpec::Local(s) => CrateSpec::Local(s.to_string()),
            CrateSpec::CratesIo(n, v) => CrateSpec::CratesIo(n.to_string(), v.to_string()),
            CrateSpec::Prebuilt(n, u) => CrateSpec::Prebuilt(n.to_string(), u.to_string()),
            CrateSpec::BinaryFile(path) => CrateSpec::BinaryFile(path.to_string()),
            CrateSpec::None => CrateSpec::None,
        }
    }
}
impl From<&str> for CrateSpec {
    fn from(spec: &str) -> CrateSpec {
        // remote crates are specified as "name@version", i.e. "xous-names@0.9.9"
        if spec.contains('@') {
            let (name, version) = spec.split_once('@').expect("couldn't parse crate specifier");
            CrateSpec::CratesIo(
                name.to_string(),
                version.to_string()
            )
        // prebuilt crates are specified as "name#url"
        // i.e. "espeak-embedded#https://ci.betrusted.io/job/espeak-embedded/lastSuccessfulBuild/artifact/target/riscv32imac-unknown-xous-elf/release/"
        } else if spec.contains('#') {
            let (name, url) = spec.split_once('#').expect("couldn't parse crate specifier");
            CrateSpec::Prebuilt(
                name.to_string(),
                url.to_string()
            )
        // local files are specified as paths, which, at a minimum include one directory separator "/" or "\"
        // i.e. "./local_file"
        // Note that this is after a test for the '#' character, so that disambiguates URL slashes
        // It does mean that files with a '#' character in them are mistaken for URL coded paths, and '@' as remote crates.
        } else if spec.contains('/') || spec.contains('\\') {
            CrateSpec::BinaryFile(spec.to_string())
        } else {
            CrateSpec::Local(spec.to_string())
        }
    }
}

pub(crate) struct Builder {
    loader: CrateSpec,
    loader_features: Vec::<String>,
    loader_disable_defaults: bool,
    loader_key: String,
    kernel: CrateSpec,
    kernel_features: Vec::<String>,
    kernel_disable_defaults: bool,
    kernel_key: String,
    /// crates that are installed in the xous.img, each one running in its own separate process space
    services: Vec::<CrateSpec>,
    /// Apps are different from services in that context menus are auto-generated for apps; furthermore, the apps must
    /// exist in the app manifest JSON file. Aside from that, the Xous kernel treats apps and services identically.
    apps: Vec::<CrateSpec>,
    features: Vec::<String>,
    stream: BuildStream,
    min_ver: String,
    target: Option<String>,
    utra_target: String,
    run_svd2repl: bool,
    locale_override: Option<String>,
    locale_stash: String,
    /// when set to true, hosted mode builds but does not run
    dry_run: bool,
}

impl Builder {
    pub fn new() -> Builder {
        Builder {
            loader: CrateSpec::Local("loader".to_string()),
            loader_features: Vec::new(),
            loader_key: "devkey/dev.key".into(),
            loader_disable_defaults: false,
            kernel: CrateSpec::Local("xous-kernel".to_string()),
            kernel_features: Vec::new(),
            kernel_key: "devkey/dev.key".into(),
            kernel_disable_defaults: false,
            services: Vec::new(),
            apps: Vec::new(),
            features: Vec::new(),
            stream: BuildStream::Release,
            min_ver: crate::MIN_XOUS_VERSION.to_string(),
            target: Some(crate::TARGET_TRIPLE.to_string()),
            utra_target: format!("precursor-{}", crate::PRECURSOR_SOC_VERSION).to_string(),
            run_svd2repl: false,
            locale_override: None,
            locale_stash: String::new(),
            dry_run: false,
        }
    }
    /// Specify an alternate loader key, as a String that can encode a file name
    /// in the local directory, or a path + filename.
    #[allow(dead_code)]
    pub fn loader_key_file<'a>(&'a mut self, filename: String) -> &'a mut Builder {
        self.loader_key = filename;
        self
    }
    /// Specify an alternate loader key, as a String that can encode a file name
    /// in the local directory, or a path + filename.
    #[allow(dead_code)]
    pub fn kernel_key_file<'a>(&'a mut self, filename: String) -> &'a mut Builder {
        self.kernel_key = filename;
        self
    }
    /// Set the build stream (debug or release)
    #[allow(dead_code)]
    pub fn stream<'a>(&'a mut self, stream: BuildStream) -> &'a mut Builder {
        self.stream = stream;
        self
    }
    /// Disable default features on the loader
    #[allow(dead_code)]
    pub fn loader_disable_defaults<'a>(&'a mut self) -> &'a mut Builder {
        self.loader_disable_defaults = true;
        self
    }
    /// Disable default features on the loader
    #[allow(dead_code)]
    pub fn kernel_disable_defaults<'a>(&'a mut self) -> &'a mut Builder {
        self.kernel_disable_defaults = true;
        self
    }
    /// Set a minimum xous version. This is the minimum Xous version necessary to read
    /// the PDDB that is generated by this build. The purpose of this is so that we can
    /// trim migration code out of the PDDB: when we have a breaking change to the PDDB,
    /// the PDDB contains code to detect the version change and migrate to the latest
    /// version. Eventually (on the order of many months or years), this code gets retired,
    /// otherwise we accumulate rarely-used code ad nauseum.
    #[allow(dead_code)]
    pub fn set_min_xous_ver<'a>(&'a mut self, min_ver_string: &str) -> &'a mut Builder {
        self.min_ver = min_ver_string.to_string();
        self
    }

    /// specify a locale string to override for the current build
    pub fn override_locale<'a>(&'a mut self, locale: &str) -> &'a mut Builder {
        self.locale_override = Some(locale.into());
        self
    }

    /// Configure for hosted mode
    pub fn target_hosted<'a>(&'a mut self) -> &'a mut Builder {
        self.loader = CrateSpec::None;
        self.target = None;
        self.stream = BuildStream::Release;
        self.utra_target = "hosted".to_string();
        self.run_svd2repl = false;
        self
    }
    /// Configure for renode targets
    pub fn target_renode<'a>(&'a mut self) -> &'a mut Builder {
        self.target = Some(crate::TARGET_TRIPLE.to_string());
        self.stream = BuildStream::Release;
        self.run_svd2repl = true;
        self.utra_target = "renode".to_string();
        self.loader = CrateSpec::Local("loader".to_string());
        self.kernel = CrateSpec::Local("xous-kernel".to_string());
        self
    }
    /// Configure for precursor targets. This is the default, but it's good practice
    /// to call it anyways just in case the defaults change. The `soc_version` should
    /// be just the gitrev of the soc version, not the entire feature name.
    pub fn target_precursor<'a>(&'a mut self, soc_version: &str) -> &'a mut Builder {
        self.target = Some(crate::TARGET_TRIPLE.to_string());
        self.stream = BuildStream::Release;
        self.utra_target = format!("precursor-{}", soc_version).to_string();
        self.run_svd2repl = false;
        self.loader = CrateSpec::Local("loader".to_string());
        self.kernel = CrateSpec::Local("xous-kernel".to_string());
        self
    }

    /// Override the default kernel. For example, to use the kernel from crates.io, specify as "xous-kernel@0.9.9"
    #[allow(dead_code)]
    pub fn use_kernel<'a>(&'a mut self, spec: &str) -> &'a mut Builder {
        self.kernel = spec.into();
        self
    }
    /// Override the default loader
    #[allow(dead_code)]
    pub fn use_loader<'a>(&'a mut self, spec: &str) -> &'a mut Builder {
        self.loader = spec.into();
        self
    }

    /// Add just one service
    #[allow(dead_code)]
    pub fn add_service<'a>(&'a mut self, service_spec: &str) -> &'a mut Builder {
        self.services.push(service_spec.into());
        self
    }
    /// Add a list of services
    pub fn add_services<'a>(&'a mut self, service_list: &Vec::<String>) -> &'a mut Builder {
        for service in service_list {
            self.services.push(service.as_str().into());
        }
        self
    }

    /// Add just one app
    #[allow(dead_code)]
    pub fn add_app<'a>(&'a mut self, app_spec: &str) -> &'a mut Builder {
        self.apps.push(app_spec.into());
        self
    }
    /// Add a list of apps
    pub fn add_apps<'a>(&'a mut self, app_list: &Vec::<String>) -> &'a mut Builder {
        for app in app_list {
            self.apps.push(app.as_str().into());
        }
        self
    }

    /// add a feature to be passed on to services
    pub fn add_feature<'a>(&'a mut self, feature: &str) -> &'a mut Builder {
        self.features.push(feature.into());
        self
    }
    /// add a feature to be passed on to just the loader
    pub fn add_loader_feature<'a>(&'a mut self, feature: &str) -> &'a mut Builder {
        self.loader_features.push(feature.into());
        self
    }
    /// add a feature to be passed on to just the loader
    #[allow(dead_code)]
    pub fn add_kernel_feature<'a>(&'a mut self, feature: &str) -> &'a mut Builder {
        self.kernel_features.push(feature.into());
        self
    }
    /// only build a hosted target. don't run it. Used exclusively to confirm that hosted mode builds in CI.
    pub fn hosted_build_only<'a>(&'a mut self) -> &'a mut Builder {
        self.dry_run = true;
        self
    }

    /// The builder sets up all the cargo arguments to build a set of packages with features for a respective
    /// target and stream. It also runs the build as well. It's meant to be called only by the `build()`
    /// method, and it gets called repeatedly to build the kernel, loader, and services.
    fn builder(
        &self,
        packages: &Vec::<CrateSpec>,
        features: &Vec::<String>,
        target: &Option<&str>,
        // the stream is specified separately here because the loader is special-case always release
        stream: BuildStream,
        extra_args: &Vec::<String>,
    ) -> Result<Vec::<String>, DynError> {
        // list of build artifacts, as full paths specific to the host OS
        let mut artifacts = Vec::<String>::new();
        // set up the list of arguments to cargo
        // we have two streams we could build:
        //   - local crates are built with "build"
        //   - remote crates are built with "install"
        let mut local_args = vec!["build"];
        let mut remote_args = vec!["install", "--target-dir", "target"];
        remote_args.push("--root");
        let output_root = format!("{}/target/{}{}/",
            project_root().into_os_string().into_string().unwrap(),
            match target {
                Some(t) => format!("{}/", t),
                None => "".to_string(),
            },
            stream.to_str(),
        );
        remote_args.push(&output_root);

        // modify the stream if release (debug is the default for builds; release is the default for installs)
        match stream {
            BuildStream::Release => {
                local_args.push("--release");
            }
            BuildStream::Debug => {
                remote_args.push("--debug");
            }
        }

        // add any extra args. These are cargo-specific args, such as "--no-default-features"
        for arg in extra_args.iter() {
            local_args.push(arg);
            remote_args.push(arg);
        }

        // set the target triple, if specified
        // and determine the location of the build artifacts
        if let Some(t) = target {
            local_args.push("--target");
            local_args.push(t);
            remote_args.push("--target");
            remote_args.push(t);
        }

        // add the packages
        let mut local_pkgs = Vec::<&str>::new();
        let mut remote_pkgs = Vec::<(&str, &str)>::new();
        for pkg in packages.iter() {
            match pkg {
                CrateSpec::Local(name) => local_pkgs.push(&name),
                CrateSpec::CratesIo(name, version) => remote_pkgs.push((&name, &version)),
                _ => {},
            }
        }

        if local_pkgs.len() > 0 {
            for pkg in local_pkgs {
                local_args.push("--package");
                local_args.push(pkg);
                artifacts.push(format!("{}{}", &output_root, pkg));
            }
            if features.len() > 0 {
                for feature in features {
                    local_args.push("--features");
                    local_args.push(feature);
                }
            }

            // emit debug
            print!("    Command: cargo");
            for &arg in local_args.iter() {
                print!(" {}", arg);
            }
            println!();
            // build
            let status = Command::new(cargo())
            .current_dir(project_root())
            .args(&local_args)
            .status()?;
            if !status.success() {
                return Err("Local build failed".into());
            }
        }
        if remote_pkgs.len() > 0 {
            // remote packages are installed one at a time
            if features.len() > 0 {
                for feature in features {
                    remote_args.push("--features");
                    remote_args.push(feature);
                }
            }

            for (name, version) in remote_pkgs {
                // emit debug
                print!("    Command: cargo");
                for &arg in remote_args.iter() {
                    print!(" {}", arg);
                }
                println!(" {} {}", name, version);
                // build
                let status = Command::new(cargo())
                .current_dir(project_root())
                .args([&remote_args[..], &[name, "--version", version].to_vec()[..]].concat())
                .status()?;
                if !status.success() {
                    return Err("Remote build failed".into());
                }
                artifacts.push(format!("{}bin/{}", &output_root, name));
            }
        }

        Ok(artifacts)
    }

    /// Consume the builder and execute the configured build task. This handles dispatching all configurations,
    /// including renode, hosted, and hardware targets.
    pub fn build(mut self) -> Result<(), DynError> {
        if self.apps.len() == 0 && self.services.len() == 0 {
            // no services were specified - don't build anything
            return Ok(())
        }
        crate::utils::ensure_compiler(&Some(TARGET_TRIPLE), false, false)?;
        self.locale_override(); // apply the locale override

        // ------ configure target generation feature flags ------
        let target = if self.utra_target.contains("renode") {
            self.features.push("renode".into());
            self.loader_features.push("renode".into());
            self.kernel_features.push("renode".into());
            Some(crate::TARGET_TRIPLE)
        } else if self.utra_target.contains("hosted") {
            self.features.push("hosted".into());
            // there is no loader in hosed mode
            self.kernel_features.push("hosted".into());
            None
        } else if self.utra_target.contains("precursor") {
            self.features.push("precursor".into());
            self.features.push(format!("utralib/{}", &self.utra_target));
            self.kernel_features.push("precursor".into());
            self.kernel_features.push(format!("utralib/{}", &self.utra_target));
            self.loader_features.push("precursor".into());
            self.loader_features.push(format!("utralib/{}", &self.utra_target));
            Some(crate::TARGET_TRIPLE)
        } else {
            return Err("Target unknown: please check your UTRA target".into());
        };

        // LAST_CONFIG tracks the last SVD configuration. It's used by utralib to track if it
        // should rebuild itself based on a change in SVD configs. Note that for some reason
        // it takes two consecutive builds with the same SVD config before the build system
        // figures out that it doesn't need to rebuild everything. After then, it behaves as expected.
        let last_config = format!("target/{}/{}/build/LAST_CONFIG", TARGET_TRIPLE, self.stream.to_str());
        std::fs::create_dir_all(format!("target/{}/{}/build/", TARGET_TRIPLE, self.stream.to_str())).unwrap();
        let changed = match OpenOptions::new()
            .read(true)
            .open(&last_config) {
            Ok(mut file) => {
                let mut contents = String::new();
                file.read_to_string(&mut contents).unwrap();
                if contents != self.utra_target {
                    true
                } else {
                    false
                }
            }
            _ => true
        };
        if changed {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&last_config).unwrap();
            write!(file, "{}", self.utra_target).unwrap();
        }

        // ------ build the services & apps ------
        let mut app_names = Vec::<String>::new();
        for app in self.apps.iter() {
            match app {
                CrateSpec::Local(name) => app_names.push(name.into()),
                CrateSpec::CratesIo(name, _version) => app_names.push(name.into()),
                _ => {}
            }
        }
        generate_app_menus(&app_names);
        let mut services_path = self.builder(
            &[&self.services[..], &self.apps[..]].concat(),
            &self.features,
            &target,
            self.stream,
            &vec![],
        )?;

        // ------ either create an image, or launch hosted mode ------
        if self.target.is_none() { // hosted mode doesn't specify a cross-compilation target!
            // throw a warning if prebuilts are specified for hosted mode
            for item in [&self.services[..], &self.apps[..]].concat() {
                match item {
                    CrateSpec::Prebuilt(name, _) => println!("Warning! Pre-built binaries not supported for hosted mode ({})", name),
                    _ => {},
                }
            }
            // fixup windows paths
            if cfg!(windows) {
                for service in services_path.iter_mut() {
                    service.push_str(".exe")
                }
            }
            let mut hosted_args = vec!["run"];
            match self.stream {
                BuildStream::Release => hosted_args.push("--release"),
                _ => {}
            }
            hosted_args.push("--");
            for service in services_path.iter() {
                hosted_args.push(service);
            }
            // jam in any pre-built local binary files that were specified
            let binary_files_string = self.enumerate_binary_files()?;
            let mut binary_files: Vec<&str> = binary_files_string.iter().map(|s| s.as_ref()).collect();
            hosted_args.append(&mut binary_files);

            if !self.dry_run {
                let mut dir = project_root();
                dir.push("kernel");
                println!("Starting hosted mode...");
                print!("    Command: cargo");
                for arg in &hosted_args {
                    print!(" {}", arg);
                }
                println!();
                let status = Command::new(cargo())
                    .current_dir(dir)
                    .args(&hosted_args)
                    .status()?;
                if !status.success() {
                    return Err("cargo run failed to launch hosted mode".into());
                }
            } else {
                // confirm the kernel can build before quitting
                let _ = self.builder(
                    &vec![CrateSpec::Local("xous-kernel".into())],
                    &self.features,
                    &target,
                    self.stream,
                    &vec![],
                )?;
                println!("Dry run requested: only building and not running");
            }
        } else {
            // ------ build the kernel ------
            let mut kernel_extra = vec![];
            if self.kernel_disable_defaults {
                kernel_extra.push("--no-default-features".to_string());
            }
            let kernel_path = self.builder(
                &vec![self.kernel.clone()],
                &self.kernel_features,
                &target,
                self.stream,
                &kernel_extra,
            )?;

            // ------ build the loader ------
            // stash any LTO settings applied to the kernel; proper layout of the loader
            // block depends on the loader being compact and highly optimized.
            let existing_lto = std::env::var("CARGO_PROFILE_RELEASE_LTO")
                .map(|v| Some(v))
                .unwrap_or(None);
            let existing_codegen_units = std::env::var("CARGO_PROFILE_RELEASE_CODEGEN_UNITS")
                .map(|v| Some(v))
                .unwrap_or(None);
            // these settings will generate the most compact code (but also the hardest to debug)
            std::env::set_var("CARGO_PROFILE_RELEASE_LTO", "true");
            std::env::set_var("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "1");
            let mut loader_extra = vec![];
            if self.loader_disable_defaults {
                loader_extra.push("--no-default-features".to_string());
            }
            let loader = self.builder(
                &vec![self.loader.clone()],
                &self.loader_features,
                &target,
                BuildStream::Release, // loader doesn't fit if you build with Debug
                &loader_extra,
            )?;
            // restore the LTO settings
            if let Some(existing) = existing_lto {
                std::env::set_var("CARGO_PROFILE_RELEASE_LTO", existing);
            }
            if let Some(existing) = existing_codegen_units {
                std::env::set_var("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", existing);
            }

            // ------ if targeting renode, regenerate the Platform file -----
            if self.run_svd2repl {
                Command::new(cargo())
                    .current_dir(project_root())
                    .args(&[
                        "run",
                        "-p",
                        "svd2repl",
                        "--",
                        "utralib/renode/renode.svd",
                        "emulation/soc/betrusted-soc.repl",
                    ])
                    .status()?;
            }

            // ---------- extract SVD file path, as computed by utralib ----------
            let svd_spec_path = format!("target/{}/{}/build/SVD_PATH", TARGET_TRIPLE, self.stream.to_str());
            let mut svd_spec_file = OpenOptions::new()
                .read(true)
                .open(&svd_spec_path)?;
            let mut svd_path = String::new();
            svd_spec_file.read_to_string(&mut svd_path)?;

            // ---------- install any pre-built packages ----------
            services_path.append(&mut self.fetch_prebuilds()?);
            services_path.append(&mut self.enumerate_binary_files()?);

            // --------- package up and sign a binary image ----------
            let output_bundle = self.create_image(
                &kernel_path[0],
                &services_path,
                MemorySpec::SvdFile(svd_path)
            )?;
            println!();
            println!(
                "Kernel+Init bundle is available at {}",
                output_bundle.display()
            );

            let mut loader_bin = output_bundle.parent().unwrap().to_owned();
            loader_bin.push("loader.bin");
            let mut loader_presign = output_bundle.parent().unwrap().to_owned();
            loader_presign.push("loader_presign.bin");
            let status = Command::new(cargo())
                .current_dir(project_root())
                .args(&[
                    "run",
                    "--package",
                    "tools",
                    "--bin",
                    "copy-object",
                    "--",
                    &loader[0],
                    loader_presign.as_os_str().to_str().unwrap(),
                ])
                .status()?;
            if !status.success() {
                return Err("cargo build failed".into());
            }

            let status = Command::new(cargo())
                .current_dir(project_root())
                .args(&[
                    "run",
                    "--package",
                    "tools",
                    "--bin",
                    "sign-image",
                    "--",
                    "--loader-image",
                    loader_presign.to_str().unwrap(),
                    "--loader-key",
                    &self.loader_key,
                    "--loader-output",
                    loader_bin.to_str().unwrap(),
                    "--min-xous-ver",
                    &self.min_ver,
                ])
                .status()?;
            if !status.success() {
                return Err("loader image sign failed".into());
            }

            let mut xous_img_path = output_bundle.parent().unwrap().to_owned();
            let mut xous_img_presign_path = xous_img_path.clone();
            xous_img_path.push("xous.img");
            xous_img_presign_path.push("xous_presign.img");
            let mut xous_img =
                std::fs::File::create(&xous_img_presign_path).expect("couldn't create xous.img");
            let mut bundle_file = std::fs::File::open(output_bundle).expect("couldn't open output bundle");
            let mut buf = vec![];
            bundle_file
                .read_to_end(&mut buf)
                .expect("couldn't read output bundle file");
            xous_img
                .write_all(&buf)
                .expect("couldn't write bundle file to xous.img");
            println!("Bundled image file created at {}", xous_img_path.display());

            let status = Command::new(cargo())
                .current_dir(project_root())
                .args(&[
                    "run",
                    "--package",
                    "tools",
                    "--bin",
                    "sign-image",
                    "--",
                    "--kernel-image",
                    xous_img_presign_path.to_str().unwrap(),
                    "--kernel-key",
                    &self.kernel_key,
                    "--kernel-output",
                    xous_img_path.to_str().unwrap(),
                    "--min-xous-ver",
                    &self.min_ver,
                    // "--defile",
                ])
                .status()?;
            if !status.success() {
                return Err("kernel image sign failed".into());
            }

            println!();
            println!("Signed loader at {}", loader_bin.display());
            println!("Signed kernel at {}", xous_img_path.display());
        }
        self.locale_restore(); // restore the locale if it was overridden

        Ok(())
    }

    fn create_image(
        &self,
        kernel: &String,
        init: &[String],
        memory_spec: MemorySpec,
    ) -> Result<PathBuf, DynError> {
        let stream = self.stream.to_str();
        let mut args = vec!["run", "--package", "tools", "--bin", "create-image", "--"];

        let output_file = format!("target/{}/{}/args.bin", TARGET_TRIPLE, stream);
        args.push(&output_file);

        args.push("--kernel");
        args.push(kernel);

        for i in init {
            args.push("--init");
            args.push(i);
        }

        match memory_spec {
            MemorySpec::SvdFile(ref s) => {
                args.push("--svd");
                args.push(s);
            }
        }

        let status = Command::new(cargo())
            .current_dir(project_root())
            .args(&args)
            .status()?;

        if !status.success() {
            return Err("cargo build failed".into());
        }
        Ok(project_root().join(&format!("target/{}/{}/args.bin", TARGET_TRIPLE, stream)))
    }

    fn fetch_prebuilds(&self) -> Result<Vec::<String>, DynError> {
        let mut paths = Vec::<String>::new();
        for item in [&self.services[..], &self.apps[..]].concat() {
            match item {
                CrateSpec::Prebuilt(name, url) => {
                    let exec_name = format!("target/{}/{}/{}", TARGET_TRIPLE, self.stream.to_str(), name);
                    println!("Fetching {} executable from build server...", name);
                    let mut exec_file = OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create(true)
                        .truncate(true)
                        .open(&exec_name)
                        .expect("Can't open our version file for writing");
                    let mut freader = ureq::get(&url)
                    .call()?
                    .into_reader();
                    std::io::copy(&mut freader, &mut exec_file)?;
                    println!(
                        "{} pre-built exec is {} bytes",
                        name,
                        exec_file.metadata().unwrap().len()
                    );
                    paths.push(exec_name);
                }
                _ => {}
            }
        }
        Ok(paths)
    }

    fn enumerate_binary_files(&self) -> Result<Vec::<String>, DynError> {
        let mut paths = Vec::<String>::new();
        for item in [&self.services[..], &self.apps[..]].concat() {
            match item {
                CrateSpec::BinaryFile(path) => {
                    paths.push(path);
                }
                _ => {}
            }
        }
        Ok(paths)
    }

    fn locale_override(&mut self) {
        if let Some(locale) = &self.locale_override {
            { // stash the existing locale
                let mut locale_file = OpenOptions::new()
                    .read(true)
                    .open("xous-rs/src/locale.rs")
                    .expect("Can't open locale file for reading");
                locale_file.read_to_string(&mut self.locale_stash).unwrap();
            }

            let mut locale_override = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open("xous-rs/src/locale.rs")
                .expect("Can't open locale for modification");
            write!(
                locale_override,
                "pub const LANG: &str = \"{}\";\n",
                locale
            ).unwrap();
        }
    }

    fn locale_restore(&self) {
        if self.locale_override.is_some() {
            let mut locale_restore = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open("xous-rs/src/locale.rs")
                .expect("Can't open locale for modification");
            write!(
                locale_restore,
                "{}",
                self.locale_stash
            ).unwrap();
        }
    }
}

pub fn cargo() -> String {
    env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

pub fn project_root() -> PathBuf {
    Path::new(&env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)
        .unwrap()
        .to_path_buf()
}
