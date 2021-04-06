#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct LogRecord {
    pub file: xous_ipc::String<128>,
    pub line: Option<u32>,
    pub module: xous_ipc::String<128>,
    pub level: u32,
    pub args: xous_ipc::String<2800>,
}
