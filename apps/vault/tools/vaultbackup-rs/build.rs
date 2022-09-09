fn main() {
    protobuf_codegen::Codegen::new()
        .cargo_out_dir("protos")
        .include("src")
        .input("src/otpauth_migration.proto")
        .run_from_script();
}
