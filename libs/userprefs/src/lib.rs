use bincode::error::{DecodeError, EncodeError};
use pddb::Pddb;
use std::io::Read;
use std::io::Write;

static PREFS_DICT: &str = "UserPrefsDict";
static PREFS_KEY: &str = "UserPrefsKey";

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
    // TODO: optimize maybe by adding a dirty bit?
    radio_on_on_boot: bool,
    connect_known_networks_on_boot: bool,
    autobacklight_on_boot: bool,
    password_after_sleep: bool,
    close_bases_on_lock: bool,
}

impl TryFrom<Vec<u8>> for UserPrefs {
    fn try_from(data: Vec<u8>) -> Result<Self, Self::Error> {
        if data.is_empty() {
            return Ok(Self::default());
        }
        let decoded: UserPrefs =
            match bincode::decode_from_slice(&data, bincode::config::standard()) {
                Ok((data, _)) => data,
                Err(err) => return Err(Error::DecodeError(err)),
            };

        Ok(decoded)
    }

    type Error = Error;
}

impl TryFrom<&UserPrefs> for Vec<u8> {
    fn try_from(up: &UserPrefs) -> Result<Self, Self::Error> {
        match bincode::encode_to_vec(up, bincode::config::standard()) {
            Ok(ret) => Ok(ret),
            Err(err) => Err(Error::EncodeError(err)),
        }
    }

    type Error = Error;
}

pub struct Manager {
    pddb_handle: Pddb,
    prefs: UserPrefs,
}

impl Manager {
    fn new() -> Self {
        let mut us = Self {
            pddb_handle: Pddb::new(),
            prefs: UserPrefs::default(),
        };

        us.prefs = us.pddb_get().unwrap();

        us
    }

    fn pddb_store(&self, payload: &UserPrefs) -> Result<(), Error> {
        let payload: Vec<u8> = payload.try_into()?;
        match self.pddb_handle.get(
            PREFS_DICT,
            PREFS_KEY,
            Some(".System"),
            true,
            true,
            None,
            None::<fn()>,
        ) {
            Ok(mut data) => match data.write(&payload) {
                Ok(_) => Ok(self.pddb_handle.sync().unwrap_or(())),
                Err(e) => Err(e.into()),
            },
            Err(e) => Err(e.into()),
        }
    }

    fn pddb_get(&self) -> Result<UserPrefs, Error> {
        match self.pddb_handle.get(
            PREFS_DICT,
            PREFS_KEY,
            Some(".System"),
            true,
            true,
            None,
            None::<fn()>,
        ) {
            Ok(mut record) => {
                let mut data = Vec::<u8>::new();
                record.read_to_end(&mut data)?;
                Ok(data.try_into()?)
            }
            Err(e) => return Err(e.into()),
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
            PREFS_KEY,
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
    use crate::{Manager, UserPrefs};

    #[test]
    fn describe() {
        //UserPrefs::describe();
        let m = Manager::new();
    }
}
