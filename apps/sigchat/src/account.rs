mod service_environment;

use pddb::Pddb;
use service_environment::ServiceEnvironment;
use std::io::{Error, ErrorKind, Read, Write};
use std::str::FromStr;

#[allow(dead_code)]
pub struct Account {
    pddb: Pddb,
    pddb_dict: String,
    service_environment: ServiceEnvironment,
    number: String,
    registered: bool,
}

const SERVICE_ENVIRONMENT_KEY: &str = "service_environment";
const NUMBER_KEY: &str = "number";
const REGISTERED_KEY: &str = "registered";

impl Account {
    pub fn new(pddb_dict: &str, number: &str) -> Result<Account, Error> {
        let pddb = pddb::Pddb::new();
        pddb.try_mount();
        set(
            &pddb,
            pddb_dict,
            SERVICE_ENVIRONMENT_KEY,
            Some(&ServiceEnvironment::Staging.to_string()),
        )?;
        set(&pddb, pddb_dict, NUMBER_KEY, Some(&number))?;
        set(&pddb, pddb_dict, REGISTERED_KEY, Some(&false.to_string()))?;
        Account::read(pddb_dict)
    }

    pub fn read(pddb_dict: &str) -> Result<Account, Error> {
        let pddb = pddb::Pddb::new();
        pddb.try_mount();
        match (
            get(&pddb, pddb_dict, SERVICE_ENVIRONMENT_KEY),
            get(&pddb, pddb_dict, NUMBER_KEY),
            get(&pddb, pddb_dict, REGISTERED_KEY),
        ) {
            (Ok(Some(service_environment)), Ok(Some(number)), Ok(Some(registered))) => {
                Ok(Account {
                    pddb: pddb,
                    pddb_dict: pddb_dict.to_string(),
                    service_environment: ServiceEnvironment::from_str(&service_environment)
                        .unwrap(),
                    number: number,
                    registered: registered.parse().unwrap(),
                })
            }
            (Err(e), _, _) => Err(e),
            (_, Err(e), _) => Err(e),
            (_, _, Err(e)) => Err(e),
            (_, _, _) => Err(Error::from(ErrorKind::InvalidData)),
        }
    }

    pub fn number(&self) -> &str {
        &self.number
    }

    #[allow(dead_code)]
    pub fn set_number(&mut self, value: &str) -> Result<(), Error> {
        match self.set(NUMBER_KEY, Some(value)) {
            Ok(_) => self.number = value.to_string(),
            Err(e) => log::warn!("failed to set signal account number: {e}"),
        }
        Ok(())
    }

    #[allow(dead_code)]
    fn get(&self, key: &str) -> Result<Option<String>, Error> {
        get(&self.pddb, &self.pddb_dict, key)
    }

    #[allow(dead_code)]
    fn set(&self, key: &str, value: Option<&str>) -> Result<(), Error> {
        set(&self.pddb, &self.pddb_dict, key, value)
    }
}

fn get(pddb: &Pddb, dict: &str, key: &str) -> Result<Option<String>, Error> {
    let value = match pddb.get(dict, key, None, true, false, None, None::<fn()>) {
        Ok(mut pddb_key) => {
            let mut buffer = [0; 256];
            match pddb_key.read(&mut buffer) {
                Ok(len) => match String::from_utf8(buffer[..len].to_vec()) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        log::warn!("failed to String: {:?}", e);
                        None
                    }
                },
                Err(e) => {
                    log::warn!("failed pddb_key read: {:?}", e);
                    None
                }
            }
        }
        Err(_) => None,
    };
    log::info!("get '{}' = '{:?}'", key, value);
    Ok(value)
}

fn set(pddb: &Pddb, dict: &str, key: &str, value: Option<&str>) -> Result<(), Error> {
    log::info!("set '{}' = '{:?}'", key, value);
    // delete key first to ensure data in a prior longer key is gone
    pddb.delete_key(dict, key, None).ok();
    if let Some(value) = value {
        match pddb.get(dict, key, None, true, true, None, None::<fn()>) {
            Ok(mut pddb_key) => match pddb_key.write(&value.as_bytes()) {
                Ok(len) => {
                    pddb.sync().ok();
                    log::trace!("Wrote {} bytes to {}:{}", len, dict, key);
                }
                Err(e) => {
                    log::warn!("Error writing {}:{} {:?}", dict, key, e);
                }
            },
            Err(e) => log::warn!("failed to set pddb {}:{}  {:?}", dict, key, e),
        };
    }
    Ok(())
}
