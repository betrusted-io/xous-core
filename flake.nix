{
  description = "Xous development environment";

  inputs = {
    nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0.2511.905687";
    rust-xous.url = "github:sbellem/rust-xous-flake";
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-xous,
    }:
    let
      rustVersion = "1.92.0";
      # Base version tag + shortRev (SHA uniquely identifies the build)
      xousBaseVersion = "v0.9.16";
      # Format must be: v<maj>.<min>.<patch>[-<extra>]-g<hexsha>
      # shortRev is already hex, so this should parse correctly
      xousVersion = "${xousBaseVersion}-0-g${self.shortRev or "0000000"}";

      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      forAllSystems =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f {
            pkgs = import nixpkgs { inherit system; };
            rustToolchain = rust-xous.packages.${system}.rustToolchain;
          }
        );
    in
    {
      packages = forAllSystems (
        { pkgs, rustToolchain }:
        let
          # Common postPatch to replace SemVer::from_git() with hardcoded version
          patchSemver = ''
            substituteInPlace tools/src/sign_image.rs \
              --replace 'SemVer::from_git()?.into()' '"${xousVersion}".parse::<SemVer>().unwrap().into()'
          '';

          # Common environment for reproducible Rust builds
          reproducibleRustEnv = ''
            export HOME=$PWD
            export CARGO_HOME=$PWD/.cargo
            mkdir -p $CARGO_HOME
            # Reproducibility flags
            export CARGO_INCREMENTAL=0
            export RUSTFLAGS="-C codegen-units=1 --remap-path-prefix=$PWD=/build"
          '';

          dabao-helloworld = pkgs.stdenv.mkDerivation {
            pname = "dabao-helloworld";
            version = "0.1.0";
            src = self;
            nativeBuildInputs = [ rustToolchain ];
            postPatch = patchSemver;
            buildPhase = ''
              ${reproducibleRustEnv}
              cargo xtask dabao helloworld
            '';
            installPhase = ''
              mkdir -p $out
              cp target/riscv32imac-unknown-xous-elf/release/*.uf2 $out/ || true
              cp target/riscv32imac-unknown-xous-elf/release/*.img $out/ || true
              cp target/riscv32imac-unknown-xous-elf/release/*.bin $out/ || true
            '';
          };

          bao1x-boot0 = pkgs.stdenv.mkDerivation {
            pname = "bao1x-boot0";
            version = "0.1.0";
            src = self;
            nativeBuildInputs = [ rustToolchain ];
            postPatch = patchSemver;
            buildPhase = ''
              ${reproducibleRustEnv}
              cargo xtask bao1x-boot0
            '';
            installPhase = ''
              mkdir -p $out
              cp target/riscv32imac-unknown-none-elf/release/*.uf2 $out/ || true
              cp target/riscv32imac-unknown-none-elf/release/*.img $out/ || true
            '';
          };

          bao1x-alt-boot1 = pkgs.stdenv.mkDerivation {
            pname = "bao1x-alt-boot1";
            version = "0.1.0";
            src = self;
            nativeBuildInputs = [ rustToolchain ];
            postPatch = patchSemver;
            buildPhase = ''
              ${reproducibleRustEnv}
              cargo xtask bao1x-alt-boot1
            '';
            installPhase = ''
              mkdir -p $out
              cp target/riscv32imac-unknown-none-elf/release/*.uf2 $out/ || true
              cp target/riscv32imac-unknown-none-elf/release/*.img $out/ || true
            '';
          };

          bao1x-boot1 = pkgs.stdenv.mkDerivation {
            pname = "bao1x-boot1";
            version = "0.1.0";
            src = self;
            nativeBuildInputs = [ rustToolchain ];
            postPatch = patchSemver;
            buildPhase = ''
              ${reproducibleRustEnv}
              cargo xtask bao1x-boot1
            '';
            installPhase = ''
              mkdir -p $out
              cp target/riscv32imac-unknown-none-elf/release/*.uf2 $out/ || true
              cp target/riscv32imac-unknown-none-elf/release/*.img $out/ || true
            '';
          };
        in
        {
          # Main packages
          inherit dabao-helloworld bao1x-boot0 bao1x-alt-boot1 bao1x-boot1;

          # Combined bootloader package
          bootloader = pkgs.runCommand "bootloader" {} ''
            mkdir -p $out
            cp -r ${bao1x-boot0}/* ${bao1x-boot1}/* ${bao1x-alt-boot1}/* $out
          '';

          # Aliases
          dabao = dabao-helloworld;
          boot0 = bao1x-boot0;
          alt-boot1 = bao1x-alt-boot1;
          boot1 = bao1x-boot1;

          default = dabao-helloworld;
        }
      );

      devShells = forAllSystems (
        { pkgs, rustToolchain }:
        {
          default = pkgs.mkShell {
            packages = [ rustToolchain ];
            shellHook = ''
              if [ -z "$CARGO_HOME" ]; then
                export CARGO_HOME="$HOME/.cargo"
              fi
              echo "Xous development environment (Rust ${rustVersion} with Xous sysroot)"
              echo "XOUS_VERSION: ${xousVersion}"
              echo ""
              echo "Available targets:"
              echo "  - riscv32imac-unknown-none-elf (kernel/loader)"
              echo "  - riscv32imac-unknown-xous-elf (apps/services)"
              echo ""
              echo "Build commands:"
              echo "  nix build .#dabao-helloworld"
              echo "  nix build .#bootloader"
              echo "  nix build .#bao1x-boot0"
              echo "  nix build .#bao1x-boot1"
              echo "  nix build .#bao1x-alt-boot1"
            '';
          };
        }
      );
    };
}
