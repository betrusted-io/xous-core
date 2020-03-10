#!/bin/sh -x

rust_root="$(pwd)/rust"
rust_target="riscv32imac-unknown-xous-elf"
export RUSTC_BOOTSTRAP=1
export RUST_TARGET_PATH="$rust_root/xous-sysroot"
export RUSTFLAGS="--sysroot $rust_root/xous-sysroot"
export CC=riscv64-unknown-elf-gcc
export AR=riscv64-unknown-elf-ar

# Get the current rust tag (currently 1.41.0)
rust_tag=$(rustc -Vv | grep 'release: ' | cut -d' ' -f2)

# # Clone the latest Rust source
echo "Checking out Rust $rust_tag-xous..."
git clone 'git@github.com:xous-os/rust.git' rust
cd rust
git checkout $rust_tag-xous
git submodule init
git submodule sync
git submodule update

cd "$rust_root/src"
git clone --recursive https://github.com/rust-lang/compiler-builtins.git

mkdir -p "$RUST_TARGET_PATH/lib/rustlib/$rust_target/lib"

echo "Creating $rust_target.json..."
rustc --target riscv32imac-unknown-none-elf --print target-spec-json -Z unstable-options | sed '/"is-builtin":/d' | sed 's/"os": "none",/"os": "xous",/' > "$RUST_TARGET_PATH/$rust_target.json"

cd $rust_root/src/libcore
cargo build --release --target $rust_target
cp $rust_root/target/$rust_target/release/libcore.rlib $RUST_TARGET_PATH/lib/rustlib/$rust_target/lib

cd "$rust_root/src/liballoc"
cargo build --release --target $rust_target
cp "$rust_root/target/$rust_target/release/liballoc.rlib" "$RUST_TARGET_PATH/lib/rustlib/$rust_target/lib"

cd "$rust_root/src/compiler-builtins"
cargo build --release --target $rust_target --features mem
cp "target/$rust_target/release/libcompiler_builtins.rlib" "$RUST_TARGET_PATH/lib/rustlib/$rust_target/lib"

cd "$rust_root/src/libstd"
cargo build --release --target $rust_target
cp "$rust_root/target/$rust_target/release/libstd.rlib" "$RUST_TARGET_PATH/lib/rustlib/$rust_target/lib"

cd "$rust_root"
