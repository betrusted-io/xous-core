use std::fs::OpenOptions;
use std::{
    env,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

use crate::{app_manifest::generate_app_menus, DynError};

#[allow(dead_code)]
#[derive(Copy, Clone, Debug)]
pub(crate) enum BuildStream {
    Debug,
    Release,
}
impl BuildStream {
    pub fn as_str(&self) -> &str {
        match self {
            BuildStream::Debug => "debug",
            BuildStream::Release => "release",
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum LoaderRegion {
    Ram,
    Flash,
    Swap,
    Invalid,
}

#[derive(Debug, Clone)]
pub enum CrateSpec {
    /// name of the crate
    Local(String, LoaderRegion),
    /// crates.io: (name of crate, version)
    CratesIo(String, String, LoaderRegion),
    /// a prebuilt package: (name of executable, URL for download)
    Prebuilt(String, String, LoaderRegion),
    /// a prebuilt binary, done using command line tools: (Optional name, path)
    BinaryFile(Option<String>, String, LoaderRegion),
    /// an empty entry
    None,
}

impl CrateSpec {
    pub fn get_region(&self) -> LoaderRegion {
        match self {
            CrateSpec::Local(_s, region) => *region,
            CrateSpec::CratesIo(_n, _v, region) => *region,
            CrateSpec::Prebuilt(_n, _u, region) => *region,
            CrateSpec::BinaryFile(_n, _path, region) => *region,
            _ => LoaderRegion::Invalid,
        }
    }

    pub fn set_region(&mut self, region: LoaderRegion) {
        *self = match self {
            CrateSpec::Local(s, _region) => CrateSpec::Local(s.to_string(), region),
            CrateSpec::CratesIo(n, v, _region) => CrateSpec::CratesIo(n.to_string(), v.to_string(), region),
            CrateSpec::Prebuilt(n, u, _region) => CrateSpec::Prebuilt(n.to_string(), u.to_string(), region),
            CrateSpec::BinaryFile(n, path, _region) => {
                CrateSpec::BinaryFile(n.as_ref().or(None).cloned(), path.to_string(), region)
            }
            CrateSpec::None => CrateSpec::None,
        }
    }

    pub fn name(&self) -> Option<String> {
        match self {
            CrateSpec::Local(s, _region) => Some(s.to_string()),
            CrateSpec::CratesIo(n, _v, _region) => Some(n.to_string()),
            CrateSpec::Prebuilt(n, _u, _region) => Some(n.to_string()),
            CrateSpec::BinaryFile(n, path, _region) => {
                if let Some(name) = n {
                    Some(name.to_string())
                } else {
                    Some(path.to_string())
                }
            }
            _ => None,
        }
    }
}
impl From<&str> for CrateSpec {
    fn from(spec: &str) -> CrateSpec {
        // remote crates are specified as "name^version", i.e. "xous-names^0.9.9"
        if spec.contains('^') {
            let (name, version) = spec.split_once('^').expect("couldn't parse crate specifier");
            CrateSpec::CratesIo(name.to_string(), version.to_string(), LoaderRegion::Ram)
        // prebuilt crates are specified as "name#url"
        // i.e. "espeak-embedded#https://ci.betrusted.io/job/espeak-embedded/lastSuccessfulBuild/artifact/target/riscv32imac-unknown-xous-elf/release/"
        } else if spec.contains('#') {
            let (name, url) = spec.split_once('#').expect("couldn't parse crate specifier");
            CrateSpec::Prebuilt(name.to_string(), url.to_string(), LoaderRegion::Ram)
        // local files are specified as paths, which, at a minimum include one directory separator "/" or "\"
        // i.e. "./local_file"
        // Note that this is after a test for the '#' character, so that disambiguates URL slashes
        // It does mean that files with a '#' character in them are mistaken for URL coded paths, and '@' as
        // remote crates.
        } else if spec.contains('/') || spec.contains('\\') {
            //optionally a BinaryFile can have a name associated with it as "name:path"
            if spec.find(':').is_some() {
                let (name, path) = spec.split_once(':').unwrap();
                CrateSpec::BinaryFile(Some(name.to_string()), path.to_string(), LoaderRegion::Ram)
            } else {
                CrateSpec::BinaryFile(None, spec.to_string(), LoaderRegion::Ram)
            }
        } else {
            CrateSpec::Local(spec.to_string(), LoaderRegion::Ram)
        }
    }
}

pub(crate) struct Builder {
    loader: CrateSpec,
    loader_features: Vec<String>,
    loader_disable_defaults: bool,
    loader_key: String,
    kernel: CrateSpec,
    kernel_features: Vec<String>,
    kernel_disable_defaults: bool,
    kernel_key: String,
    /// crates that are installed in the xous.img, each one running in its own separate process space
    services: Vec<CrateSpec>,
    /// Apps are different from services in that context menus are auto-generated for apps; furthermore, the
    /// apps must exist in the app manifest JSON file. Aside from that, the Xous kernel treats apps and
    /// services identically.
    apps: Vec<CrateSpec>,
    features: Vec<String>,
    global_flags: Vec<String>,
    stream: BuildStream,
    min_ver: String,
    target: Option<String>,
    /// The kernel might require a different target than the rest of the programs.
    target_kernel: Option<String>,
    utra_target: String,
    run_svd2repl: bool,
    locale_override: Option<String>,
    locale_stash: String,
    /// when set to true, hosted mode builds but does not run
    dry_run: bool,
    /// when set to true, user selected packages are compiled but no image is created
    no_image: bool,
    /// when Some, specifies a swap region as offset, size
    swap: Option<(u32, u32)>,
}

impl Builder {
    pub fn new() -> Builder {
        Builder {
            loader: CrateSpec::Local("loader".to_string(), LoaderRegion::Ram),
            loader_features: Vec::new(),
            loader_key: "devkey/dev.key".into(),
            loader_disable_defaults: false,
            kernel: CrateSpec::Local("xous-kernel".to_string(), LoaderRegion::Ram),
            kernel_features: Vec::new(),
            kernel_key: "devkey/dev.key".into(),
            kernel_disable_defaults: false,
            services: Vec::new(),
            apps: Vec::new(),
            features: Vec::new(),
            global_flags: Vec::new(),
            stream: BuildStream::Release,
            min_ver: crate::MIN_XOUS_VERSION.to_string(),
            target: Some(crate::TARGET_TRIPLE_RISCV32.to_string()),
            target_kernel: Some(crate::TARGET_TRIPLE_RISCV32_KERNEL.to_string()),
            utra_target: format!("precursor-{}", crate::PRECURSOR_SOC_VERSION).to_string(),
            run_svd2repl: false,
            locale_override: None,
            locale_stash: String::new(),
            dry_run: false,
            no_image: false,
            swap: None,
        }
    }

    /// Specify an alternate loader key, as a String that can encode a file name
    /// in the local directory, or a path + filename.
    #[allow(dead_code)]
    pub fn loader_key_file(&mut self, filename: String) -> &mut Builder {
        self.loader_key = filename;
        self
    }

    /// Specify an alternate loader key, as a String that can encode a file name
    /// in the local directory, or a path + filename.
    #[allow(dead_code)]
    pub fn kernel_key_file(&mut self, filename: String) -> &mut Builder {
        self.kernel_key = filename;
        self
    }

    pub fn set_swap<'a>(&'a mut self, offset: u32, size: u32) -> &'a mut Builder {
        self.swap = Some((offset, size));
        self
    }

    pub fn is_swap_set(&self) -> bool { self.swap.is_some() }

    /// Set the build stream (debug or release)
    #[allow(dead_code)]
    pub fn stream(&mut self, stream: BuildStream) -> &mut Builder {
        self.stream = stream;
        self
    }

    /// Disable default features on the loader
    #[allow(dead_code)]
    pub fn loader_disable_defaults(&mut self) -> &mut Builder {
        self.loader_disable_defaults = true;
        self
    }

    /// Disable default features on the loader
    #[allow(dead_code)]
    pub fn kernel_disable_defaults(&mut self) -> &mut Builder {
        self.kernel_disable_defaults = true;
        self
    }

    pub fn add_global_flag(&mut self, flag: &str) -> &mut Builder {
        self.global_flags.push(flag.to_string());
        self
    }

    /// Set a minimum xous version. This is the minimum Xous version necessary to read
    /// the PDDB that is generated by this build. The purpose of this is so that we can
    /// trim migration code out of the PDDB: when we have a breaking change to the PDDB,
    /// the PDDB contains code to detect the version change and migrate to the latest
    /// version. Eventually (on the order of many months or years), this code gets retired,
    /// otherwise we accumulate rarely-used code ad nauseam.
    #[allow(dead_code)]
    pub fn set_min_xous_ver(&mut self, min_ver_string: &str) -> &mut Builder {
        self.min_ver = min_ver_string.to_string();
        self
    }

    /// specify a locale string to override for the current build
    pub fn override_locale(&mut self, locale: &str) -> &mut Builder {
        self.locale_override = Some(locale.into());
        self
    }

    /// Configure for hosted mode
    pub fn target_hosted(&mut self) -> &mut Builder {
        self.loader = CrateSpec::None;
        self.target = None;
        self.target_kernel = None;
        self.stream = BuildStream::Release;
        self.utra_target = "hosted".to_string();
        self.run_svd2repl = false;
        self
    }

    /// Configure for renode targets
    pub fn target_renode(&mut self) -> &mut Builder {
        self.target = Some(crate::TARGET_TRIPLE_RISCV32.to_string());
        self.target_kernel = Some(crate::TARGET_TRIPLE_RISCV32_KERNEL.to_string());
        self.stream = BuildStream::Release;
        self.run_svd2repl = true;
        self.utra_target = "renode".to_string();
        self.loader = CrateSpec::Local("loader".to_string(), LoaderRegion::Ram);
        self.kernel = CrateSpec::Local("xous-kernel".to_string(), LoaderRegion::Ram);
        self
    }

    /// Configure for precursor targets. This is the default, but it's good practice
    /// to call it anyway just in case the defaults change. The `soc_version` should
    /// be just the gitrev of the soc version, not the entire feature name.
    pub fn target_precursor(&mut self, soc_version: &str) -> &mut Builder {
        self.target = Some(crate::TARGET_TRIPLE_RISCV32.to_string());
        self.target_kernel = Some(crate::TARGET_TRIPLE_RISCV32_KERNEL.to_string());
        self.stream = BuildStream::Release;
        self.utra_target = format!("precursor-{}", soc_version).to_string();
        self.run_svd2repl = false;
        self.loader = CrateSpec::Local("loader".to_string(), LoaderRegion::Ram);
        self.kernel = CrateSpec::Local("xous-kernel".to_string(), LoaderRegion::Ram);
        self
    }

    pub fn target_precursor_no_image(&mut self, soc_version: &str) -> &mut Builder {
        self.target = Some(crate::TARGET_TRIPLE_RISCV32.to_string());
        self.target_kernel = Some(crate::TARGET_TRIPLE_RISCV32_KERNEL.to_string());
        self.stream = BuildStream::Release;
        self.utra_target = format!("precursor-{}", soc_version).to_string();
        self.run_svd2repl = false;
        self.no_image = true;
        self
    }

    /// Configure for ARM targets
    pub fn target_arm(&mut self) -> &mut Builder {
        self.target = Some(crate::TARGET_TRIPLE_ARM.to_string());
        self.target_kernel = Some(crate::TARGET_TRIPLE_ARM_KERNEL.to_string());
        self.stream = BuildStream::Debug;
        self.utra_target = "atsama5d27".to_string();
        self.run_svd2repl = false;
        self.loader = CrateSpec::Local("loader".to_string(), LoaderRegion::Ram);
        self.kernel = CrateSpec::Local("xous-kernel".to_string(), LoaderRegion::Ram);
        self
    }

    /// Configure various Cramium targets
    pub fn target_cramium_fpga(&mut self) -> &mut Builder {
        self.target = Some(crate::TARGET_TRIPLE_RISCV32.to_string());
        self.target_kernel = Some(crate::TARGET_TRIPLE_RISCV32_KERNEL.to_string());
        self.stream = BuildStream::Release;
        self.utra_target = "cramium-fpga".to_string();
        self.run_svd2repl = false;
        self.loader = CrateSpec::Local("loader".to_string(), LoaderRegion::Ram);
        self.kernel = CrateSpec::Local("xous-kernel".to_string(), LoaderRegion::Ram);
        self
    }

    pub fn target_cramium_soc(&mut self) -> &mut Builder {
        self.target = Some(crate::TARGET_TRIPLE_RISCV32.to_string());
        self.target_kernel = Some(crate::TARGET_TRIPLE_RISCV32_KERNEL.to_string());
        self.stream = BuildStream::Release;
        self.utra_target = "cramium-soc".to_string();
        self.run_svd2repl = false;
        self.loader = CrateSpec::Local("loader".to_string(), LoaderRegion::Ram);
        self.kernel = CrateSpec::Local("xous-kernel".to_string(), LoaderRegion::Ram);
        self
    }

    /// Override the default kernel. For example, to use the kernel from crates.io, specify as
    /// "xous-kernel@0.9.9"
    #[allow(dead_code)]
    pub fn use_kernel(&mut self, spec: &str) -> &mut Builder {
        self.kernel = spec.into();
        self
    }

    /// Override the default loader
    #[allow(dead_code)]
    pub fn use_loader(&mut self, spec: &str) -> &mut Builder {
        self.loader = spec.into();
        self
    }

    /// Check if a file exists and is executable
    pub fn executable_exists(&self, file: &str) -> bool {
        if let Ok(_metadata) = std::fs::metadata(file) {
            #[cfg(target_os = "windows")]
            if file.to_lowercase().ends_with("exe") {
                return true;
            }
            #[cfg(not(target_os = "windows"))]
            if _metadata.is_file() {
                use std::os::unix::fs::PermissionsExt;
                if _metadata.permissions().mode() & 0o100 != 0 {
                    return true;
                }
            }
        }
        false
    }

    /// Add just one service
    pub fn add_service(&mut self, service_spec: &str, region: LoaderRegion) -> &mut Builder {
        let mut spec: CrateSpec = service_spec.into();
        spec.set_region(region);
        self.services.push(spec);
        self
    }

    /// Add a list of services
    pub fn add_services<S: AsRef<str>>(&mut self, service_list: impl IntoIterator<Item = S>) -> &mut Builder {
        for service in service_list {
            self.services.push(service.as_ref().into());
        }
        self
    }

    /// Add just one app. Apps can be remote or downloaded externally.
    #[allow(dead_code)]
    pub fn add_app(&mut self, app_spec: &str, region: LoaderRegion) -> &mut Builder {
        let mut spec: CrateSpec = app_spec.into();
        spec.set_region(region);
        self.apps.push(spec);
        self
    }

    /// Add a list of apps. Apps can be remote or downloaded externally.
    pub fn add_apps<S: AsRef<str>>(&mut self, app_list: impl IntoIterator<Item = S>) -> &mut Builder {
        for app in app_list {
            self.apps.push(app.as_ref().into());
        }
        self
    }

    /// add a feature to be passed on to services
    pub fn add_feature(&mut self, feature: &str) -> &mut Builder {
        self.features.push(feature.into());
        self
    }

    /// remove a feature previously added by a previous call
    #[allow(dead_code)]
    pub fn remove_feature(&mut self, feature: &str) -> &mut Builder {
        self.features.retain(|x| x != feature);
        self
    }

    /// test if a feature is present
    pub fn has_feature(&self, feature: &str) -> bool { self.features.contains(&feature.to_string()) }

    /// add a feature to be passed on to just the loader
    pub fn add_loader_feature(&mut self, feature: &str) -> &mut Builder {
        self.loader_features.push(feature.into());
        self
    }

    /// add a feature to be passed on to just the loader
    #[allow(dead_code)]
    pub fn add_kernel_feature(&mut self, feature: &str) -> &mut Builder {
        self.kernel_features.push(feature.into());
        self
    }

    /// only build a hosted target. don't run it. Used exclusively to confirm that hosted mode builds in CI.
    pub fn hosted_build_only(&mut self) -> &mut Builder {
        self.dry_run = true;
        self
    }

    /// The builder sets up all the cargo arguments to build a set of packages with features for a respective
    /// target and stream. It also runs the build as well. It's meant to be called only by the `build()`
    /// method, and it gets called repeatedly to build the kernel, loader, and services.
    fn builder(
        &self,
        packages: &[CrateSpec],
        features: &[String],
        target: &Option<&str>,
        // the stream is specified separately here because the loader is special-case always release
        stream: BuildStream,
        extra_args: &[String],
        no_default_features: bool,
    ) -> Result<Vec<String>, DynError> {
        // list of build artifacts, as full paths specific to the host OS
        let mut artifacts = Vec::<String>::new();
        // set up the list of arguments to cargo
        // we have two streams we could build:
        //   - local crates are built with "build"
        //   - remote crates are built with "install"
        let mut local_args = vec!["build"];
        let mut remote_args = vec!["install", "--target-dir", "target"];
        remote_args.push("--root");
        let output_root = format!(
            "{}/target/{}{}/",
            project_root().into_os_string().into_string().unwrap(),
            match target {
                Some(t) => format!("{}/", t),
                None => "".to_string(),
            },
            stream.as_str(),
        );
        remote_args.push(&output_root);

        for flag in self.global_flags.iter() {
            local_args.push(flag);
        }

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
                CrateSpec::Local(name, _region) => local_pkgs.push(name),
                CrateSpec::CratesIo(name, version, _region) => remote_pkgs.push((&name, &version)),
                _ => {}
            }
        }

        if !local_pkgs.is_empty() {
            for pkg in local_pkgs {
                local_args.push("--package");
                local_args.push(pkg);
                artifacts.push(format!("{}{}", &output_root, pkg));
            }
            if no_default_features {
                local_args.push("--no-default-features");
            }
            if !features.is_empty() {
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
            let status = Command::new(cargo()).current_dir(project_root()).args(&local_args).status()?;
            if !status.success() {
                return Err("Local build failed".into());
            }
        }
        if !remote_pkgs.is_empty() {
            // remote packages are installed one at a time
            if no_default_features {
                local_args.push("--no-default-features");
            }
            if !features.is_empty() {
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

    pub fn split_region(&self, services: Vec<String>) -> (Vec<String>, Vec<String>, Vec<String>) {
        let mut inie = Vec::<String>::new();
        let mut inif = Vec::<String>::new();
        let mut inis = Vec::<String>::new();
        for service in services.iter() {
            let mut found = false;
            for app in self.apps.iter() {
                if let Some(n) = &app.name() {
                    if Path::new(service).file_name().unwrap_or_default().to_str().unwrap_or_default()
                        == Path::new(n).file_name().unwrap_or_default().to_str().unwrap_or_default()
                    {
                        match app.get_region() {
                            LoaderRegion::Flash => inif.push(service.to_string()),
                            LoaderRegion::Ram => inie.push(service.to_string()),
                            LoaderRegion::Swap => inis.push(service.to_string()),
                            _ => (),
                        }
                        found = true;
                        continue;
                    }
                }
            }
            if found {
                continue;
            }
            for serv in self.services.iter() {
                if let Some(n) = &serv.name() {
                    if Path::new(service).file_name().unwrap_or_default().to_str().unwrap_or_default()
                        == Path::new(n).file_name().unwrap_or_default().to_str().unwrap_or_default()
                    {
                        match serv.get_region() {
                            LoaderRegion::Flash => inif.push(service.to_string()),
                            LoaderRegion::Ram => inie.push(service.to_string()),
                            LoaderRegion::Swap => inis.push(service.to_string()),
                            _ => (),
                        }
                        found = true;
                        continue;
                    }
                }
            }
            if found {
                continue;
            }
            // the service wasn't found in any of the other lists, mark it as non-xip
            inie.push(service.to_string());
        }
        assert_eq!(inie.len() + inif.len() + inis.len(), services.len());
        (inie, inif, inis)
    }

    /// Consume the builder and execute the configured build task. This handles dispatching all
    /// configurations, including renode, hosted, and hardware targets.
    pub fn build(mut self) -> Result<(), DynError> {
        if self.apps.is_empty() && self.services.is_empty() {
            // no services were specified - don't build anything
            return Ok(());
        }

        // ------ configure target generation feature flags ------
        if self.utra_target.contains("renode") {
            self.features.push("renode".into());
            self.loader_features.push("renode".into());
            self.kernel_features.push("renode".into());
        } else if self.utra_target.contains("hosted") {
            self.features.push("hosted".into());
            // there is no loader in hosed mode
            self.kernel_features.push("hosted".into());
        } else if self.utra_target.contains("precursor") {
            self.features.push("precursor".into());
            self.features.push(format!("utralib/{}", &self.utra_target));
            self.kernel_features.push("precursor".into());
            self.kernel_features.push(format!("utralib/{}", &self.utra_target));
            self.loader_features.push("precursor".into());
            self.loader_features.push(format!("utralib/{}", &self.utra_target));
        } else if self.utra_target.contains("atsama5d2") {
            self.kernel_features.push("atsama5d27".into());
            self.loader_features.push("atsama5d27".into());
        } else if self.utra_target.contains("cramium-fpga") {
            self.features.push("cramium-fpga".into());
            self.features.push(format!("utralib/{}", &self.utra_target));
            self.kernel_features.push("cramium-fpga".into());
            self.kernel_features.push(format!("utralib/{}", &self.utra_target));
            self.loader_features.push("cramium-fpga".into());
            self.loader_features.push(format!("utralib/{}", &self.utra_target));
        } else if self.utra_target.contains("cramium-soc") {
            self.features.push("cramium-soc".into());
            self.features.push(format!("utralib/{}", &self.utra_target));
            self.kernel_features.push("cramium-soc".into());
            self.kernel_features.push(format!("utralib/{}", &self.utra_target));
            self.loader_features.push("cramium-soc".into());
            self.loader_features.push(format!("utralib/{}", &self.utra_target));
        } else {
            return Err("Target unknown: please check your UTRA target".into());
        }

        crate::utils::ensure_compiler(&self.target.as_deref(), false, false)?;
        crate::utils::ensure_kernel_compiler(&self.target_kernel.as_deref(), false)?;
        self.locale_override(); // apply the locale override

        // ------ build the services & apps ------
        let mut app_names = Vec::<String>::new();
        for app in self.apps.iter() {
            match app {
                CrateSpec::Local(name, _region) => app_names.push(name.into()),
                CrateSpec::CratesIo(name, _version, _region) => app_names.push(name.into()),
                CrateSpec::BinaryFile(Some(name), _location, _region) => {
                    // if binary file has a name, ensure it ends up in the app menu
                    app_names.push(name.to_string())
                }
                _ => {}
            }
        }
        generate_app_menus(&app_names);
        let mut services_path = self.builder(
            &[&self.services[..], &self.apps[..]].concat(),
            &self.features,
            &self.target.as_deref(),
            self.stream,
            &[],
            false,
        )?;

        // ------ either stop here, create an image, or launch hosted mode ------
        if self.no_image {
            println!("The following apps/services were compiled:");
            for path in services_path {
                println!("{}", path);
            }
        } else if self.target.is_none() {
            // hosted mode doesn't specify a cross-compilation target!
            // throw a warning if prebuilt files are specified for hosted mode
            for item in [&self.services[..], &self.apps[..]].concat() {
                if let CrateSpec::Prebuilt(name, _, _region) = item {
                    println!("Warning! Pre-built binaries not supported for hosted mode ({})", name);
                }
            }
            // fixup windows paths
            if cfg!(windows) {
                for service in services_path.iter_mut() {
                    service.push_str(".exe")
                }
            }
            let mut hosted_args = vec!["run"];
            if let BuildStream::Release = self.stream {
                hosted_args.push("--release");
            }
            hosted_args.push("--");
            for service in services_path.iter() {
                hosted_args.push(service);
            }
            // jam in any pre-built local binary files that were specified
            let binary_files_string = self.enumerate_binary_files()?;
            let mut canonicalized_paths = Vec::new();
            let mut binary_files_storage = Vec::<String>::new();
            for f in binary_files_string {
                if !self.executable_exists(&f) {
                    panic!("FATAL ERROR: App '{}' does not exist or is not executable.", f);
                }
                canonicalized_paths
                    .push(std::fs::canonicalize(f).expect("Couldn't canonicalize executable target"));
            }
            for f in canonicalized_paths {
                let path_as_str = f.to_str().expect("Couldn't canonicalize executable target").to_string();
                let windows_clean_path = if let Some(stripped) = path_as_str.strip_prefix("\\\\?\\") {
                    stripped.to_owned()
                } else {
                    path_as_str
                };
                binary_files_storage.push(windows_clean_path);
            }
            let mut binary_files: Vec<&str> = binary_files_storage.iter().map(|s| s.as_ref()).collect();
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
                let status = Command::new(cargo()).current_dir(dir).args(&hosted_args).status()?;
                if !status.success() {
                    return Err("cargo run failed to launch hosted mode".into());
                }
            } else {
                // confirm the kernel can build before quitting
                let _ = self.builder(
                    &[CrateSpec::Local("xous-kernel".into(), LoaderRegion::Ram)],
                    &self.kernel_features,
                    &self.target_kernel.as_deref(),
                    self.stream,
                    &[],
                    false,
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
                &[self.kernel.clone()],
                &self.kernel_features,
                &self.target_kernel.as_deref(),
                self.stream,
                &kernel_extra,
                false,
            )?;

            // ------ build the loader ------
            // stash any LTO settings applied to the kernel; proper layout of the loader
            // block depends on the loader being compact and highly optimized.
            let existing_lto = env::var("CARGO_PROFILE_RELEASE_LTO").map(Some).unwrap_or(None);
            let existing_codegen_units =
                env::var("CARGO_PROFILE_RELEASE_CODEGEN_UNITS").map(Some).unwrap_or(None);
            // these settings will generate the most compact code (but also the hardest to debug)
            env::set_var("CARGO_PROFILE_RELEASE_LTO", "true");
            env::set_var("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "1");
            let mut loader_extra = vec![];
            if self.loader_disable_defaults {
                loader_extra.push("--no-default-features".to_string());
            }
            let loader = self.builder(
                &[self.loader.clone()],
                &self.loader_features,
                &self.target_kernel.as_deref(),
                BuildStream::Release, // loader doesn't fit if you build with Debug
                &loader_extra,
                true, // loader builds without any default features
            )?;
            // restore the LTO settings
            if let Some(existing) = existing_lto {
                env::set_var("CARGO_PROFILE_RELEASE_LTO", existing);
            }
            if let Some(existing) = existing_codegen_units {
                env::set_var("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", existing);
            }

            // ------ if targeting renode, regenerate the Platform file -----
            if self.run_svd2repl {
                Command::new(cargo())
                    .current_dir(project_root())
                    .args([
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
            let svd_spec_path = format!(
                "target/{}/{}/build/SVD_PATH",
                self.target.as_ref().expect("target"),
                self.stream.as_str()
            );
            let mut svd_spec_file = OpenOptions::new().read(true).open(svd_spec_path)?;
            let mut svd_path_str = String::new();
            svd_spec_file.read_to_string(&mut svd_path_str)?;
            let mut svd_paths = Vec::new();
            for line in svd_path_str.lines() {
                svd_paths.push(line.to_owned());
            }

            // ---------- install any pre-built packages ----------
            services_path.append(&mut self.fetch_prebuilds()?);
            services_path.append(&mut self.enumerate_binary_files()?);

            // --------- package up and sign a binary image ----------
            let (inie, inif, inis) = self.split_region(services_path.clone());
            let output_bundle = self.create_image(&kernel_path[0], &inie, &inif, &inis, svd_paths)?;
            println!();
            println!("Kernel+Init bundle is available at {}", output_bundle.display());

            let mut loader_bin = output_bundle.parent().unwrap().to_owned();
            loader_bin.push("loader.bin");
            let mut loader_presign = output_bundle.parent().unwrap().to_owned();
            loader_presign.push("loader_presign.bin");
            let status = Command::new(cargo())
                .current_dir(project_root())
                .args([
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
                .args([
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
            xous_img_path.push("xous.img");

            let status = Command::new(cargo())
                .current_dir(project_root())
                .args([
                    "run",
                    "--package",
                    "tools",
                    "--bin",
                    "sign-image",
                    "--",
                    "--kernel-image",
                    output_bundle.to_str().unwrap(),
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
        inif: &[String],
        inis: &[String],
        memory_spec: Vec<String>,
    ) -> Result<PathBuf, DynError> {
        let stream = self.stream.as_str();
        let mut args = vec!["run", "--package", "tools", "--bin", "create-image"];
        args.push("--features");
        if self.utra_target.contains("renode") {
            args.push("renode");
        } else if self.utra_target.contains("precursor") {
            args.push("precursor");
        } else if self.utra_target.contains("atsama5d2") {
            args.push("atsama5d2");
        } else if self.utra_target.contains("cramium-soc") {
            args.push("cramium-soc")
        }
        args.push("--");

        let mut output_file = PathBuf::new();
        output_file.push("target");
        output_file.push(self.target.as_ref().expect("target"));
        output_file.push(stream);
        output_file.push("xous_presign.img");
        args.push(output_file.to_str().unwrap());

        args.push("--kernel");
        args.push(kernel);

        for i in init {
            args.push("--init");
            // strip '@' version specifiers out of the package names, if they exist.
            let i = if i.contains('@') { i.split('@').next().unwrap() } else { i };
            args.push(i);
        }

        for i in inif {
            args.push("--inif");
            // strip '@' version specifiers out of the package names, if they exist.
            let i = if i.contains('@') { i.split('@').next().unwrap() } else { i };
            args.push(i);
        }

        for i in inis {
            args.push("--inis");
            // strip '@' version specifiers out of the package names, if they exist.
            let i = if i.contains("@") { i.split("@").next().unwrap() } else { i };
            args.push(i);
        }

        let swap_spec = if let Some((offset, size)) = self.swap {
            format!("0x{:x}:0x{:x}", offset, size) // create-image requires a base decorator, but the argument into xtask does not.
        } else {
            String::new()
        };
        let mut swap_file = PathBuf::new();
        if self.swap.is_some() {
            args.push("--swap");
            args.push(&swap_spec);
            swap_file.push("target");
            swap_file.push(self.target.as_ref().expect("target"));
            swap_file.push(stream);
            swap_file.push("swap.img");
            args.push("--swap-name");
            args.push(swap_file.to_str().unwrap())
        }

        if memory_spec.len() == 1 {
            args.push("--svd");
            args.push(&memory_spec[0])
        } else {
            args.push("--svd");
            args.push(&memory_spec[0]);
            for spec in memory_spec[1..].iter() {
                args.push("--extra-svd");
                args.push(spec);
            }
        }

        let status = Command::new(cargo()).current_dir(project_root()).args(&args).status()?;

        if !status.success() {
            return Err("cargo build failed".into());
        }
        Ok(project_root().join(output_file))
    }

    fn fetch_prebuilds(&self) -> Result<Vec<String>, DynError> {
        let mut paths = Vec::<String>::new();
        for item in [&self.services[..], &self.apps[..]].concat() {
            if let CrateSpec::Prebuilt(name, url, _region) = item {
                let exec_name = format!(
                    "target/{}/{}/{}",
                    self.target.as_ref().expect("target"),
                    self.stream.as_str(),
                    name
                );
                println!("Fetching {} executable from build server...", name);
                let mut exec_file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&exec_name)
                    .expect("Can't open our version file for writing");
                let mut freader = ureq::get(&url).call()?.into_reader();
                std::io::copy(&mut freader, &mut exec_file)?;
                println!("{} pre-built exec is {} bytes", name, exec_file.metadata().unwrap().len());
                paths.push(exec_name);
            }
        }
        Ok(paths)
    }

    fn enumerate_binary_files(&self) -> Result<Vec<String>, DynError> {
        let mut paths = Vec::<String>::new();
        for item in [&self.services[..], &self.apps[..]].concat() {
            if let CrateSpec::BinaryFile(_name, path, _region) = item {
                paths.push(path);
            }
        }
        Ok(paths)
    }

    fn locale_override(&mut self) {
        if let Some(locale) = &self.locale_override {
            {
                // stash the existing locale
                let mut locale_file = OpenOptions::new()
                    .read(true)
                    .open("locales/src/locale.rs")
                    .expect("Can't open locale file for reading");
                locale_file.read_to_string(&mut self.locale_stash).unwrap();
            }

            let mut locale_override = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open("locales/src/locale.rs")
                .expect("Can't open locale for modification");
            writeln!(locale_override, "pub const LANG: &str = \"{}\";", locale).unwrap();
        }
    }

    fn locale_restore(&self) {
        if self.locale_override.is_some() {
            let mut locale_restore = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open("locales/src/locale.rs")
                .expect("Can't open locale for modification");
            write!(locale_restore, "{}", self.locale_stash).unwrap();
        }
    }
}

pub fn cargo() -> String { env::var("CARGO").unwrap_or_else(|_| "cargo".to_string()) }

pub fn project_root() -> PathBuf {
    Path::new(&env!("CARGO_MANIFEST_DIR")).ancestors().nth(1).unwrap().to_path_buf()
}
