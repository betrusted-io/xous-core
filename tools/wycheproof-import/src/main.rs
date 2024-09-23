use std::env;
use std::io::Write;
use std::ops::AddAssign;
use std::path::Path;
use std::process::exit;

use eyre::{Result, WrapErr, bail};
use serde::Deserialize;

#[derive(Deserialize)]
struct TestData {
    #[serde(rename(deserialize = "testGroups"))]
    test_groups: Vec<TestGroup>,
}

#[derive(Deserialize)]
struct TestGroup {
    tests: Vec<TestCase>,
}

#[serde_with::serde_as]
#[derive(Debug, Deserialize)]
struct TestCase {
    #[serde(rename(deserialize = "tcId"))]
    id: usize,
    #[serde_as(as = "serde_with::hex::Hex")]
    public: [u8; 32],
    #[serde_as(as = "serde_with::hex::Hex")]
    private: [u8; 32],
    #[serde_as(as = "serde_with::hex::Hex")]
    shared: [u8; 32],
    result: String,
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        println!("usage: wycheproof-import <input.json> <output.bin>");
        exit(0);
    }
    let input_file_path = Path::new(&args[1]);
    let output_file_path = Path::new(&args[2]);

    let test_data = std::fs::read_to_string(input_file_path)
        .wrap_err(format!("Could not read test data from '{}'", input_file_path.to_string_lossy()))?;
    let test_data: TestData = serde_json::from_str(&test_data).wrap_err("Error parsing test vectors")?;

    let expected_results = vec!["valid".to_string(), "acceptable".to_string()];
    let mut output_file = std::fs::File::create(output_file_path)
        .wrap_err(format!("Error creating output file '{}'", output_file_path.to_string_lossy()))?;
    let mut last_id = 0;

    for test_case in &test_data.test_groups[0].tests {
        if test_case.id != last_id + 1 {
            bail!(
                "Expect test cases to be continuously ascending. Expected next tcId to be {}, was {}",
                last_id + 1,
                test_case.id
            )
        }
        if !expected_results.contains(&test_case.result) {
            bail!("Expect test case results to be one of {:?}", expected_results);
        }
        last_id.add_assign(1);

        output_file.write_all(&test_case.public)?;
        output_file.write_all(&test_case.private)?;
        output_file.write_all(&test_case.shared)?;
    }
    Ok(())
}
