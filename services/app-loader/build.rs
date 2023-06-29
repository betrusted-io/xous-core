use std::process::Command;

fn main() {
    // this should be changed at some point to support other platforms
    let target = "riscv32imac-unknown-xous-elf";
    
    // compile the spawn package
    let status = Command::new("cargo").args(&["build", "--package", "spawn", "--target", target])
        .status().unwrap();
    if !status.success() {
	panic!();
    }
    // turn it into a stub
    let status = Command::new("cargo").args(&["run", "--package", "tools", "--bin", "copy-object", "--"])
	.arg(&format!("../../target/{}/debug/spawn", target))
	.status().unwrap();
    if !status.success() {
	panic!();
    }

    println!("cargo:rerun-if-changed=spawn");
}
