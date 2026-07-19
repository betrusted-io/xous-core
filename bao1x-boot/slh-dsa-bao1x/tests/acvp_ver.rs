#![allow(non_snake_case)]
#![cfg(feature = "alloc")]

use serde::Deserialize;
use slh_dsa::*;

const KEYGEN_KAT_JSON: &str = include_str!("acvp/SLH-DSA-sigVer-FIPS205/internalProjection.json");

#[derive(Deserialize, Debug)]
#[serde(transparent)]
struct HexString {
    #[serde(with = "hex::serde")]
    data: Vec<u8>,
}

#[derive(Deserialize, Debug)]
struct TestCase {
    pk: HexString,
    message: HexString,
    signature: HexString,
    testPassed: bool,
}

#[derive(Deserialize, Debug)]
struct TestGroup {
    parameterSet: String,
    tests: Vec<TestCase>,
}

#[derive(Deserialize, Debug)]
struct TestFile {
    testGroups: Vec<TestGroup>,
}

macro_rules! parameter_case {
    ($param:ident, $test_case:expr) => {{
        let sk = VerifyingKey::<$param>::try_from($test_case.pk.data.as_slice()).unwrap();
        if let Ok(sig) = $test_case.signature.data.as_slice().try_into() {
            let success = sk.slh_verify_internal(&[$test_case.message.data.as_slice()], &sig);
            assert_eq!($test_case.testPassed, success.is_ok());
        } else {
            assert!(!$test_case.testPassed);
        }
    }};
}

#[test]
fn test_verify_cvp() {
    let mut i = 0;
    let test_file: TestFile = serde_json::from_str(KEYGEN_KAT_JSON).unwrap();
    for test_group in test_file.testGroups {
        let p = test_group.parameterSet;
        for test_case in test_group.tests {
            match p.as_str() {
                Shake128f::NAME => parameter_case!(Shake128f, test_case),
                Shake128s::NAME => parameter_case!(Shake128s, test_case),
                Shake192f::NAME => parameter_case!(Shake192f, test_case),
                Shake192s::NAME => parameter_case!(Shake192s, test_case),
                Shake256f::NAME => parameter_case!(Shake256f, test_case),
                Shake256s::NAME => parameter_case!(Shake256s, test_case),
                Sha2_128f::NAME => parameter_case!(Sha2_128f, test_case),
                Sha2_128s::NAME => parameter_case!(Sha2_128s, test_case),
                Sha2_192f::NAME => parameter_case!(Sha2_192f, test_case),
                Sha2_192s::NAME => parameter_case!(Sha2_192s, test_case),
                Sha2_256f::NAME => parameter_case!(Sha2_256f, test_case),
                Sha2_256s::NAME => parameter_case!(Sha2_256s, test_case),
                _ => panic!("Unknown parameter set: {}", p),
            }
            i += 1;
        }
    }
    print!("Number of test cases: {}", i);
}
