#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct TestStruct {
    pub challenge: [u32; 8],
}
impl TestStruct {
    pub fn new() -> Self {
        TestStruct { challenge: [0; 8] }
    }
}

pub const SERVER_NAME_BENCHMARK: &str = "_Benchmark target_";

#[allow(dead_code)]
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum Opcode {
    TestScalar, //(u32),
    TestMemory, //(TestStruct),
    TestMemorySend,
}
