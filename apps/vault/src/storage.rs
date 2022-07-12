use chrono::{DateTime, NaiveDateTime, Utc};
use std::convert::TryFrom;
use std::{
    io::Read,
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};

const VAULT_PASSWORD_DICT: &'static str = "vault.passwords";
const VAULT_TOTP_DICT: &'static str = "vault.totp";
/// bytes to reserve for a key entry. Making this slightly larger saves on some churn as stuff gets updated
const VAULT_ALLOC_HINT: usize = 256;
const VAULT_TOTP_ALLOC_HINT: usize = 128;
const VAULT_PASSWORD_REC_VERSION: u32 = 1;
const VAULT_TOTP_REC_VERSION: u32 = 1;

pub struct Manager {
    pddb: pddb::Pddb,
    trng: trng::Trng,
}

#[derive(Debug)]
pub enum Error {
    IoError(std::io::Error),
    TotpSerError(TOTPSerializationError),
    PasswordSerError(PasswordSerializationError),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::IoError(e)
    }
}

impl From<TOTPSerializationError> for Error {
    fn from(e: TOTPSerializationError) -> Self {
        Self::TotpSerError(e)
    }
}

impl From<PasswordSerializationError> for Error {
    fn from(e: PasswordSerializationError) -> Self {
        Self::PasswordSerError(e)
    }
}

impl Manager {
    pub fn new(xns: &xous_names::XousNames) -> Manager {
        Manager {
            pddb: pddb::Pddb::new(),
            trng: trng::Trng::new(xns).unwrap(),
        }
    }

    fn gen_guid(&mut self) -> String {
        let mut guid = [0u8; 16];
        self.trng.fill_bytes(&mut guid);
        hex::encode(guid)
    }

    fn pddb_store(
        &mut self,
        payload: &[u8],
        dict: &str,
        key_name: &str,
        alloc_hint: Option<usize>,
        basis: Option<String>,
    ) -> Result<(), Error> {
        match self.pddb.get(
            dict,
            &key_name,
            basis.as_deref(),
            true,
            true,
            alloc_hint,
            Some(crate::basis_change),
        ) {
            Ok(mut data) => match data.write(payload) {
                Ok(_) => Ok(self.pddb.sync().unwrap_or(())),
                Err(e) => Err(Error::IoError(e)),
            },
            Err(e) => Err(Error::IoError(e)),
        }
    }

    fn pddb_get(&mut self, dict: &str, key_name: &str) -> Result<Vec<u8>, Error> {
        match self.pddb.get(
            dict,
            key_name,
            None,
            false,
            false,
            None,
            Some(crate::basis_change),
        ) {
            Ok(mut record) => {
                let mut data = Vec::<u8>::new();
                record.read_to_end(&mut data)?;
                Ok(data)
            }
            Err(e) => return Err(Error::IoError(e)),
        }
    }

    fn basis_for_key(&mut self, dict: &str, key_name: &str) -> Result<String, Error> {
        match self.pddb.get(
            dict,
            key_name,
            None,
            false,
            false,
            None,
            Some(crate::basis_change),
        ) {
            Ok(record) => Ok(record
                .attributes()
                .expect("couldn't get key attributes")
                .basis),
            Err(e) => return Err(Error::IoError(e)),
        }
    }

    pub fn new_totp_record(
        &mut self,
        record: TotpRecord,
        basis: Option<String>,
    ) -> Result<(), Error> {
        let mut record = record;
        record.ctime = utc_now().timestamp() as u64;
        let serialized_record: Vec<u8> = record.into();
        let guid = self.gen_guid();

        self.pddb_store(
            &serialized_record,
            VAULT_TOTP_DICT,
            &guid,
            Some(VAULT_TOTP_ALLOC_HINT),
            basis,
        )
    }

    pub fn new_password_record(
        &mut self,
        record: PasswordRecord,
        basis: Option<String>,
    ) -> Result<(), Error> {
        let mut record = record;
        record.ctime = utc_now().timestamp() as u64;
        let serialized_record: Vec<u8> = record.into();
        let guid = self.gen_guid();

        self.pddb_store(
            &serialized_record,
            VAULT_PASSWORD_DICT,
            &guid,
            Some(VAULT_TOTP_ALLOC_HINT),
            basis,
        )
        .into()
    }

    pub fn all_totp(&mut self) -> Result<Vec<TotpRecord>, Error> {
        let keylist = self.pddb.list_keys(VAULT_TOTP_DICT, None)?;

        let mut ret = vec![];

        for key in keylist {
            let record = TotpRecord::try_from(self.pddb_get(VAULT_TOTP_DICT, &key)?)?;
            ret.push(record);
        }

        Ok(ret)
    }

    pub fn all_passwords(&mut self) -> Result<Vec<PasswordRecord>, Error> {
        let keylist = self.pddb.list_keys(VAULT_PASSWORD_DICT, None)?;

        let mut ret = vec![];

        for key in keylist {
            let record = PasswordRecord::try_from(self.pddb_get(VAULT_PASSWORD_DICT, &key)?)?;
            ret.push(record);
        }

        Ok(ret)
    }

    pub fn totp(&mut self, key_name: &str) -> Result<TotpRecord, Error> {
        TotpRecord::try_from(self.pddb_get(VAULT_TOTP_DICT, key_name)?)
            .map_err(|error| Error::TotpSerError(error))
    }

    pub fn password(&mut self, key_name: &str) -> Result<PasswordRecord, Error> {
        PasswordRecord::try_from(self.pddb_get(VAULT_PASSWORD_DICT, key_name)?)
            .map_err(|error| Error::PasswordSerError(error))
    }

    pub fn update_totp(&mut self, key_name: &str, record: TotpRecord) -> Result<(), Error> {
        let basis = self.basis_for_key(VAULT_TOTP_DICT, key_name)?;
        self.pddb
            .delete_key(VAULT_TOTP_DICT, key_name, Some(&basis))?;

        self.new_totp_record(record, Some(basis))
    }

    pub fn delete_totp(&mut self, key_name: &str) -> Result<(), Error> {
        let basis = self.basis_for_key(VAULT_TOTP_DICT, key_name)?;
        self.pddb
            .delete_key(VAULT_TOTP_DICT, key_name, Some(&basis))
            .map_err(|error| Error::IoError(error))
    }

    pub fn delete_password(&mut self, key_name: &str) -> Result<(), Error> {
        let basis = self.basis_for_key(VAULT_PASSWORD_DICT, key_name)?;
        self.pddb
            .delete_key(VAULT_PASSWORD_DICT, key_name, Some(&basis))
            .map_err(|error| Error::IoError(error))
    }
}

pub struct TotpRecord {
    pub version: u32,
    // as base32, RFC4648 no padding
    pub secret: String,
    pub name: String,
    pub algorithm: crate::totp::TotpAlgorithm,
    pub notes: String,
    pub digits: u32,
    pub timestep: u64,
    pub ctime: u64,
}

#[derive(Debug)]
pub enum TOTPSerializationError {
    BadVersion,
    BadAlgorithm,
    BadDigitsAmount,
    BadCtime,
    BadTimestep,
    MalformedInput,
}

impl TryFrom<Vec<u8>> for TotpRecord {
    type Error = TOTPSerializationError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        let desc_str = String::from_utf8(value).or(Err(TOTPSerializationError::MalformedInput))?;

        let mut pr = TotpRecord {
            version: 0,
            secret: String::new(),
            name: String::new(),
            algorithm: crate::totp::TotpAlgorithm::HmacSha1,
            notes: String::new(),
            digits: 0,
            ctime: 0,
            timestep: 0,
        };
        let lines = desc_str.split('\n');
        for line in lines {
            if let Some((tag, data)) = line.split_once(':') {
                match tag {
                    "version" => {
                        if let Ok(ver) = u32::from_str_radix(data, 10) {
                            pr.version = ver
                        } else {
                            log::warn!("ver error");
                            return Err(TOTPSerializationError::BadVersion);
                        }
                    }
                    "secret" => pr.secret.push_str(data),
                    "name" => pr.name.push_str(data),
                    "algorithm" => {
                        pr.algorithm = match crate::totp::TotpAlgorithm::try_from(data) {
                            Ok(a) => a,
                            Err(_) => return Err(TOTPSerializationError::BadAlgorithm),
                        }
                    }
                    "notes" => pr.notes.push_str(data),
                    "digits" => {
                        if let Ok(digits) = u32::from_str_radix(data, 10) {
                            pr.digits = digits;
                        } else {
                            log::warn!("digits error");
                            return Err(TOTPSerializationError::BadDigitsAmount);
                        }
                    }
                    "ctime" => {
                        if let Ok(ctime) = u64::from_str_radix(data, 10) {
                            pr.ctime = ctime;
                        } else {
                            log::warn!("ctime error");
                            return Err(TOTPSerializationError::BadCtime);
                        }
                    }
                    "timestep" => {
                        if let Ok(timestep) = u64::from_str_radix(data, 10) {
                            pr.timestep = timestep;
                        } else {
                            log::warn!("timestep error");
                            return Err(TOTPSerializationError::BadTimestep);
                        }
                    }
                    _ => {
                        log::warn!(
                            "unexpected tag {} encountered parsing TOTP info, ignoring",
                            tag
                        );
                    }
                }
            } else {
                log::trace!("invalid line skipped: {:?}", line);
            }
        }

        Ok(pr)
    }
}

impl From<TotpRecord> for Vec<u8> {
    fn from(tr: TotpRecord) -> Self {
        let ta: String = tr.algorithm.into();
        format!(
            "{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n",
            "version",
            tr.version,
            "secret",
            tr.secret,
            "name",
            tr.name,
            "algorithm",
            ta,
            "notes",
            tr.notes,
            "digits",
            tr.digits,
            "timestep",
            tr.timestep,
            "ctime",
            tr.ctime,
        )
        .into_bytes()
    }
}

#[derive(Debug)]
pub enum PasswordSerializationError {
    MalformedInput,
    BadVersion,
    BadCount,
    BadCtime,
    BadAtime,
}

pub struct PasswordRecord {
    pub version: u32,
    pub description: String,
    pub username: String,
    pub password: String,
    pub notes: String,
    pub ctime: u64,
    pub atime: u64,
    pub count: u64,
}

impl TryFrom<Vec<u8>> for PasswordRecord {
    type Error = PasswordSerializationError;

    fn try_from(data: Vec<u8>) -> Result<Self, Self::Error> {
        let desc_str =
            String::from_utf8(data).or(Err(PasswordSerializationError::MalformedInput))?;

        let mut pr = PasswordRecord {
            version: 0,
            description: String::new(),
            username: String::new(),
            password: String::new(),
            notes: String::new(),
            ctime: 0,
            atime: 0,
            count: 0,
        };

        let lines = desc_str.split('\n');
        for line in lines {
            if let Some((tag, data)) = line.split_once(':') {
                match tag {
                    "version" => {
                        if let Ok(ver) = u32::from_str_radix(data, 10) {
                            pr.version = ver
                        } else {
                            log::warn!("ver error");
                            return Err(PasswordSerializationError::BadVersion);
                        }
                    }
                    "description" => pr.description.push_str(data),
                    "username" => pr.username.push_str(data),
                    "password" => pr.password.push_str(data),
                    "notes" => pr.notes.push_str(data),
                    "ctime" => {
                        if let Ok(ctime) = u64::from_str_radix(data, 10) {
                            pr.ctime = ctime;
                        } else {
                            log::warn!("ctime error");
                            return Err(PasswordSerializationError::BadCtime);
                        }
                    }
                    "atime" => {
                        if let Ok(atime) = u64::from_str_radix(data, 10) {
                            pr.atime = atime;
                        } else {
                            log::warn!("atime error");
                            return Err(PasswordSerializationError::BadAtime);
                        }
                    }
                    "count" => {
                        if let Ok(count) = u64::from_str_radix(data, 10) {
                            pr.count = count;
                        } else {
                            log::warn!("count error");
                            return Err(PasswordSerializationError::BadCount);
                        }
                    }
                    _ => {
                        log::warn!(
                            "unexpected tag {} encountered parsing password info, ignoring",
                            tag
                        );
                    }
                }
            } else {
                log::trace!("invalid line skipped: {:?}", line);
            }
        }
        Ok(pr)
    }
}

impl From<PasswordRecord> for Vec<u8> {
    fn from(pr: PasswordRecord) -> Self {
        format!(
            "{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n",
            "version",
            pr.version,
            "description",
            pr.description,
            "username",
            pr.username,
            "password",
            pr.password,
            "notes",
            pr.notes,
            "ctime",
            pr.ctime,
            "atime",
            pr.atime,
            "count",
            pr.count,
        )
        .into_bytes()
    }
}

/// because we don't get Utc::now, as the crate checks your architecture and xous is not recognized as a valid target
fn utc_now() -> DateTime<Utc> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before Unix epoch");
    let naive = NaiveDateTime::from_timestamp(now.as_secs() as i64, now.subsec_nanos() as u32);
    DateTime::from_utc(naive, Utc)
}
