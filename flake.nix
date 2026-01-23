{
  description = "Xous development environment";

  inputs = {
    nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0.2511.906247";
    flake-utils.url = "github:numtide/flake-utils/11707dc2f618dd54ca8739b309ec4fc024de578b";
    rust-xous.url = "github:sbellem/rust-xous-flake?rev=39eebf47342faf50a2892e9dfadee895068157b8";
    rust-xous.inputs.nixpkgs.follows = "nixpkgs";
    rust-overlay.url = "https://flakehub.com/f/oxalica/rust-overlay/0.1.2051";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    crane.url = "github:ipetkov/crane/0bda7e7d005ccb5522a76d11ccfbf562b71953ca";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-xous,
      rust-overlay,
      crane,
    }:
    let
      gitTag = "0.9.16"; # git describe --abbrev=0
      gitTagRevCount = 7276; # git rev-list --count $(git describe --abbrev=0)

      # for swap_writer
      gitRevFull = if self ? rev
        then self.rev
        else "0000000000000000000000000000000000000000";

      gitHash = builtins.substring 0 9 gitRevFull;

      sinceTagRevCount = if self ? revCount
        then toString (self.revCount - gitTagRevCount)
        else "0";

      xousVersion = "v${gitTag}-${sinceTagRevCount}-g${gitHash}";
    in
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [
            rust-overlay.overlays.default
            rust-xous.overlays.default
          ];
        };
        craneLib = (crane.mkLib pkgs).overrideToolchain pkgs.rustToolchainXous;

        # Clean source to only include cargo-relevant files
        src = craneLib.cleanCargoSource self;

        # Vendor all dependencies (including git deps) - this runs in a FOD with network access
        vendoredDeps = craneLib.vendorCargoDeps {
          inherit src;
        };

        # Vendor locales dependencies separately (it has its own Cargo.lock)
        localesSrc = pkgs.lib.cleanSourceWith {
          src = self;
          filter = path: type:
            (pkgs.lib.hasInfix "/locales/" path) ||
            (pkgs.lib.hasSuffix "/locales" path) ||
            (baseNameOf path == "Cargo.toml" && dirOf path == toString self + "/locales") ||
            (baseNameOf path == "Cargo.lock" && dirOf path == toString self + "/locales");
        };

        vendoredLocalesDeps = craneLib.vendorCargoDeps {
          src = self + "/locales";
        };

        # Common postPatch to replace SemVer::from_git() with hardcoded version
        patchSemver = ''
          substituteInPlace tools/src/sign_image.rs \
            --replace-fail 'SemVer::from_git()?.into()' '"${xousVersion}".parse::<SemVer>().unwrap().into()'
        '';

        # Patch versioning.rs to use XOUS_VERSION env var instead of git describe
        patchVersioning = ''
          substituteInPlace xtask/src/versioning.rs \
            --replace-fail 'let gitver = output.stdout;' \
                           'let gitver = std::env::var("XOUS_VERSION").map(|s| s.into_bytes()).unwrap_or(output.stdout);'
        '';

        # Patch swap_writer.rs to use GIT_REV env var instead of running git
        patchSwapWriter = ''
          substituteInPlace tools/src/swap_writer.rs \
            --replace-fail 'Command::new("git").args(&["rev-parse", "HEAD"]).output().expect("Failed to execute command")' \
                           'std::env::var("GIT_REV").map(|s| std::process::Output { status: std::process::ExitStatus::default(), stdout: s.into_bytes(), stderr: vec![] }).unwrap_or_else(|_| Command::new("git").args(&["rev-parse", "HEAD"]).output().expect("Failed to execute command"))'
        '';

        # Configure cargo to use vendored deps
        configureVendoring = ''
          mkdir -p .cargo
          cat ${vendoredDeps}/config.toml >> .cargo/config.toml
          mkdir -p locales/.cargo
          cat ${vendoredLocalesDeps}/config.toml >> locales/.cargo/config.toml
        '';

        # Common environment for reproducible Rust builds
        reproducibleRustEnv = ''
          export HOME=$PWD
          export CARGO_HOME=$PWD/.cargo
          mkdir -p $CARGO_HOME
          export XOUS_VERSION="${xousVersion}"
          # Git revision for swap_writer.rs (uses last 16 hex chars of full hash)
          export GIT_REV="${gitRevFull}"
          export CARGO_INCREMENTAL=0
          export RUSTFLAGS="-C codegen-units=1 --remap-path-prefix=$PWD=/build"
          export SOURCE_DATE_EPOCH=1
        '';

        # Helper to create build derivations
        mkXousBuild = { pname, xtaskCmd, targetDir ? "riscv32imac-unknown-none-elf" }:
          pkgs.stdenv.mkDerivation {
            inherit pname;
            version = "0.1.0";
            src = self;  # Use full source for xtask builds
            nativeBuildInputs = [ pkgs.rustToolchainXous ];

            postPatch = patchSemver + patchVersioning + patchSwapWriter;

            configurePhase = configureVendoring;

            buildPhase = ''
              ${reproducibleRustEnv}
              cargo xtask ${xtaskCmd} --offline --no-verify
            '';

            installPhase = ''
              mkdir -p $out
              cp target/${targetDir}/release/*.uf2 $out/ || true
              cp target/${targetDir}/release/*.img $out/ || true
              cp target/${targetDir}/release/*.bin $out/ || true
            '';
          };

        dabao-helloworld = mkXousBuild {
          pname = "dabao-helloworld";
          xtaskCmd = "dabao helloworld";
          targetDir = "riscv32imac-unknown-xous-elf";
        };

        bao1x-boot0 = mkXousBuild {
          pname = "bao1x-boot0";
          xtaskCmd = "bao1x-boot0";
        };

        bao1x-alt-boot1 = mkXousBuild {
          pname = "bao1x-alt-boot1";
          xtaskCmd = "bao1x-alt-boot1";
        };

        bao1x-boot1 = mkXousBuild {
          pname = "bao1x-boot1";
          xtaskCmd = "bao1x-boot1";
        };

        bao1x-baremetal-dabao = mkXousBuild {
          pname = "bao1x-baremetal-dabao";
          xtaskCmd = "bao1x-baremetal-dabao";
        };

        baosec = mkXousBuild {
          pname = "baosec";
          xtaskCmd = "baosec";
          targetDir = "riscv32imac-unknown-xous-elf";
        };

        nightlyRustToolchain = pkgs.rust-bin.selectLatestNightlyWith (toolchain:
          toolchain.default.override {
            extensions = [ "rustfmt" ];
          }
        );
      in
      {
        packages = {
          # Main packages
          inherit dabao-helloworld bao1x-boot0 bao1x-alt-boot1 bao1x-boot1 bao1x-baremetal-dabao baosec;

          # bootloader stage 1
          boot1 = pkgs.runCommand "boot1" {} ''
            mkdir -p $out
            cp -r ${bao1x-boot1}/* ${bao1x-alt-boot1}/* $out
          '';

          # Combined bootloader package (boot0 + boot1)
          bootloader = pkgs.runCommand "bootloader" {} ''
            mkdir -p $out
            cp -r ${bao1x-boot0}/* ${bao1x-boot1}/* ${bao1x-alt-boot1}/* $out
          '';

          # CI dependency caching - bundles shared dependencies
          ci-deps = pkgs.symlinkJoin {
            name = "xous-ci-deps";
            paths = [
              pkgs.rustToolchainXous
              vendoredDeps
            ];
          };

          # Aliases
          dabao = dabao-helloworld;
          baremetal = bao1x-baremetal-dabao;

          default = dabao-helloworld;
        };

        devShells = {
          default = pkgs.mkShell {
            packages = [ pkgs.rustToolchainXous ];
            shellHook = ''
              echo "──────────────────────────────────────────────────────────────"
              echo "Xous development environment"
              echo "  $(rustc --version)"
              echo "  xous-core ${xousVersion}"
              echo ""
              echo "Installed Rust targets:"
              ls "$(rustc --print sysroot)/lib/rustlib" | grep -v -E '^(etc|src)$' | sed 's/^/  • /'
              echo ""
              echo "Build commands:"
              echo "  • nix build .#dabao-helloworld"
              echo "  • nix build .#baosec"
              echo "  • nix build .#bao1x-baremetal-dabao"
              echo "  • nix build .#bao1x-boot0"
              echo "  • nix build .#bao1x-boot1"
              echo "  • nix build .#bao1x-alt-boot1"
              echo ""
              echo "Aliases:"
              echo "  • nix build .#dabao       (dabao-helloworld)"
              echo "  • nix build .#baremetal   (bao1x-baremetal-dabao)"
              echo "  • nix build .#boot1       (bao1x-boot1 + bao1x-alt-boot1)"
              echo "  • nix build .#bootloader  (bao1x-boot0 bao1x-boot1 + bao1x-alt-boot1)""
              echo ""
              echo "For formatting checks, use: nix develop .#nightly"
              echo "──────────────────────────────────────────────────────────────"
            '';
          };

          nightly = pkgs.mkShell {
            packages = [ nightlyRustToolchain ];
            shellHook = ''
              echo "──────────────────────────────────────────────────────────────"
              echo "Xous nightly development environment"
              echo "  $(rustc --version)"
              echo "  $(cargo --version)"
              echo "  xous-core ${xousVersion}"
              echo ""
              echo "Formatting commands:"
              echo "  • cargo fmt --check"
              echo "  • cargo fmt"
              echo "──────────────────────────────────────────────────────────────"
            '';
          };
        };
      }
    );
}
