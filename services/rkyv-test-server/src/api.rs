use xous_ipc::String;
pub(crate) const SERVER_NAME: &str = "Rkyv Test Server 1";
/// A `usize` value that gets set as the `id` for every message handled
/// by our server.
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum Opcode {
    /// Perform a mathematical function
    Mathematics = 0,

    /// Log the string to the log server with the given prefix
    LogString = 1,

    /// Callback with log string
    AddLogStringCallback = 2,

    /// Double the string
    DoubleString = 3,
}

/// These enums indicate what kind of callback type we're sending.
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum CallbackType {
    /// A log message was sent
    LogString,
}

/// A rich structure that contains multiple values.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum MathOperation {
    /// Add two numbers together and return the result.
    Add(i32, i32),

    /// Subtract two numbers and return the result.
    Subtract(i32, i32),

    /// Multiply the two numbers
    Multiply(i32, i32),

    /// Divide the two numbers
    Divide(i32, i32),
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct LogString {
    pub(crate) prefix: String<32>,
    pub(crate) message: String<512>,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct StringDoubler {
    pub(crate) value: String<512>,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug)]
pub enum Error {
    InternalError,
    Overflow,
    Underflow,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum MathResult {
    Value(i32),
    Error(Error),
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct RegisterCallback {
    server: (u32, u32, u32, u32)
}
