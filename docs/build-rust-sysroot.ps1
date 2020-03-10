$rust_root=(Get-Location).ToString() + "\rust"
$env:RUSTC_BOOTSTRAP=1
$env:RUST_TARGET_PATH="$rust_root\xous-sysroot"
$env:RUSTFLAGS="--sysroot $rust_root\xous-sysroot"
$env:CC="riscv64-unknown-elf-gcc"
$env:AR="riscv64-unknown-elf-ar"

# Get the current rust tag (currently 1.41.0)
$rust_tag=(rustc -Vv | Select-String "release: ").ToString().Replace("release: ", "")

# Clone the latest Rust source
# git stash
Write-Output "Checking out Rust $rust_tag-xous..."
git clone 'git@github.com:xous-os/rust.git' rust
git checkout $rust_tag-xous
git submodule init
git submodule sync
git submodule update

Set-Location $rust_root/src
git clone --recursive https://github.com/rust-lang/compiler-builtins.git

mkdir -Force $rust_root\xous-sysroot
mkdir -Force $rust_root\xous-sysroot\lib\rustlib\riscv32imac-unknown-xous-elf\lib

Write-Output "Creating riscv32imac-unknown-xous-elf.json..."
$orig_json=(rustc --target riscv32imac-unknown-none-elf --print target-spec-json -Z unstable-options | Where-Object { $_ -notmatch "is-builtin" })
$orig_json.replace('"os": "none",', '"os": "xous",') | Out-File -FilePath $rust_root\xous-sysroot\riscv32imac-unknown-xous-elf.json -Encoding ascii

Set-Location $rust_root/src/libcore
cargo build --release --target riscv32imac-unknown-xous-elf
Copy-Item $rust_root/target/riscv32imac-unknown-xous-elf/release/libcore.rlib $rust_root\xous-sysroot\lib\rustlib\riscv32imac-unknown-xous-elf\lib

Set-Location $rust_root/src/liballoc
cargo build --release --target riscv32imac-unknown-xous-elf
Copy-Item $rust_root/target/riscv32imac-unknown-xous-elf/release/liballoc.rlib $rust_root\xous-sysroot\lib\rustlib\riscv32imac-unknown-xous-elf\lib

Set-Location $rust_root/src/compiler-builtins
cargo build --release --target riscv32imac-unknown-xous-elf --features mem
Copy-Item target/riscv32imac-unknown-xous-elf/release/libcompiler_builtins.rlib $rust_root\xous-sysroot\lib\rustlib\riscv32imac-unknown-xous-elf\lib

Set-Location $rust_root/src/libstd
cargo build --release --target riscv32imac-unknown-xous-elf
Copy-Item $rust_root/target/riscv32imac-unknown-xous-elf/release/libstd.rlib $rust_root\xous-sysroot\lib\rustlib\riscv32imac-unknown-xous-elf\lib

Set-Location $rust_root
