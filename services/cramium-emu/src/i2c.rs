use cramium_api::*;

pub struct I2c {}

impl I2c {
    pub fn new() -> Self { I2c {} }

    /// This is used to pass a list of I2C transactions that must be completed atomically
    /// No further I2C requests may happen while this is processing.
    pub fn i2c_transactions(&self, _list: I2cTransactions) -> Result<I2cTransactions, xous::Error> {
        Ok(I2cTransactions { transactions: vec![] })
    }
}

impl I2cApi for I2c {
    fn i2c_read(
        &mut self,
        _dev: u8,
        _adr: u8,
        buf: &mut [u8],
        _repeated_start: bool,
    ) -> Result<usize, xous::Error> {
        Ok(buf.len())
    }

    fn i2c_write(&mut self, _dev: u8, _adr: u8, data: &[u8]) -> Result<usize, xous::Error> { Ok(data.len()) }
}
