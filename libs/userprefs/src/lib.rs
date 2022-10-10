use bincode::error::{DecodeError, EncodeError};
use pddb::Pddb;
use std::io::Read;
use std::io::Write;

static PREFS_DICT: &str = "UserPrefsDict";

#[derive(Debug)]
pub enum Error {
    EncodeError(EncodeError),
    DecodeError(DecodeError),
    IoError(std::io::Error),
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err)
    }
}

impl From<EncodeError> for Error {
    fn from(err: EncodeError) -> Self {
        Self::EncodeError(err)
    }
}

impl From<DecodeError> for Error {
    fn from(err: DecodeError) -> Self {
        Self::DecodeError(err)
    }
}

#[derive(
    bincode::Encode, bincode::Decode, PartialEq, Debug, Default, prefsgenerator::GetterSetter,
)]
struct UserPrefs {
    radio_on_on_boot: bool,
    connect_known_networks_on_boot: bool,
    autobacklight_on_boot: bool,
    password_after_sleep: bool,
    close_bases_on_lock: bool,
}

pub struct Manager {
    pddb_handle: Pddb,
}

impl Manager {
    fn new() -> Self {
        Self {
            pddb_handle: Pddb::new(),
        }
    }

    fn pddb_store_key(&self, key: &str, value: &[u8]) -> Result<(), Error> {
        match self.pddb_handle.get(
            PREFS_DICT,
            key,
            Some(".System"),
            true,
            true,
            None,
            None::<fn()>,
        ) {
            Ok(mut data) => match data.write(value) {
                Ok(_) => Ok(self.pddb_handle.sync().unwrap_or(())),
                Err(e) => Err(e.into()),
            },
            Err(e) => Err(e.into()),
        }
    }

    fn pddb_get_key(&self, key: &str) -> Result<Vec<u8>, Error> {
        match self.pddb_handle.get(
            PREFS_DICT,
            key,
            Some(".System"),
            true,
            true,
            None,
            None::<fn()>,
        ) {
            Ok(mut record) => {
                let mut data = Vec::<u8>::new();
                record.read_to_end(&mut data)?;
                Ok(data)
            }
            Err(e) => return Err(e.into()),
        }
    }
}

impl Default for Manager {
    fn default() -> Self {
        Self::new()
    }
}

mod test {
    #[test]
    fn describe() {
        use crate::Manager;
        let _m = Manager::new();
}
