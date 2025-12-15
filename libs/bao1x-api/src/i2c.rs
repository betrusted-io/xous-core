pub trait I2cApi {
    fn i2c_write(&mut self, dev: u8, adr: u8, data: &[u8]) -> Result<I2cResult, xous::Error>;

    /// initiate an i2c read. The read buffer is passed during the await.
    fn i2c_read(
        &mut self,
        dev: u8,
        adr: u8,
        buf: &mut [u8],
        repeated_start: bool,
    ) -> Result<I2cResult, xous::Error>;
}

#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Debug, PartialEq, Eq)]
pub enum I2cTransactionType {
    Write,
    Read,
    ReadRepeatedStart,
}

#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum I2cResult {
    /// For the outbound message holder
    Pending,
    /// Returns # of bytes read or written if successful
    Ack(usize),
    /// An error occurred.
    Nack,
    /// An unhandled error has occurred.
    InternalError,
}
#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[cfg(feature = "std")]
#[derive(Debug)]
pub struct I2cTransaction {
    pub i2c_type: I2cTransactionType,
    pub device: u8,
    pub address: u8,
    pub data: Vec<u8>,
    pub result: I2cResult,
}

#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[cfg(feature = "std")]
pub struct I2cTransactions {
    pub transactions: Vec<I2cTransaction>,
}
#[cfg(feature = "std")]
impl From<Vec<I2cTransaction>> for I2cTransactions {
    fn from(value: Vec<I2cTransaction>) -> Self { Self { transactions: value } }
}
