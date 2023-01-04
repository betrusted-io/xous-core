use uf2_util::{ convert_elf, convert_bin };

include!("./data/constants.rs");

#[test]
fn from_elf() {
    let input = include_bytes!("./data/input.elf");
    let output = include_bytes!("./data/output.uf2");

    let result = convert_elf(input, PAGE_SIZE).unwrap();

    assert_eq!(output.len(), result.len(), "Number of output bytes differ");
    for i in 0..output.len() {
        assert_eq!(output[i], result[i], "Bytes don't match expected test output");
    }
}

#[test]
fn from_bin() {
    let input = include_bytes!("./data/input.bin");
    let output = include_bytes!("./data/output.uf2");

    let result = convert_bin(input, PAGE_SIZE, BASE_ADDRESS).unwrap();

    assert_eq!(output.len(), result.len(), "Number of output bytes differ");
    for i in 0..output.len() {
        assert_eq!(output[i], result[i], "Bytes don't match expected test output");
    }
}