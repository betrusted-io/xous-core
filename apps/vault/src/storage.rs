use std::convert::TryFrom;
use std::{
    io::Read,
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, NaiveDateTime, Utc};
use ctap_crypto::Hash256;

use crate::totp::TotpAlgorithm;

const VAULT_PASSWORD_DICT: &'static str = "vault.passwords";
const VAULT_TOTP_DICT: &'static str = "vault.totp";
const VAULT_TOTP_ALLOC_HINT: usize = 128;
const VAULT_PASSWORD_REC_VERSION: u32 = 1;

// Version history TOTP record:
//  - v1 created, basic record for TOTP
//  - v2 add HOTP support:
//    - `hotp` field added. If 1, then HOTP record. If not existent or not 1, then TOTP
//    - If HOTP, then the `timestep` field is re-purposed as the `count` field.
//    - v1 records read directly onto v2 records, and `hotp` is always `false` for v1 records
const VAULT_TOTP_REC_VERSION: u32 = 2;

#[derive(Debug)]
pub enum Error {
    IoError(std::io::Error),
    TotpSerError(TOTPSerializationError),
    PasswordSerError(PasswordSerializationError),
    KeyExists,
    DupesExist(Vec<usize>),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self { Self::IoError(e) }
}

impl From<TOTPSerializationError> for Error {
    fn from(e: TOTPSerializationError) -> Self { Self::TotpSerError(e) }
}

impl From<PasswordSerializationError> for Error {
    fn from(e: PasswordSerializationError) -> Self { Self::PasswordSerError(e) }
}

pub struct Manager {
    pddb: pddb::Pddb,
}

pub trait StorageContent {
    fn settings(&self) -> ContentPDDBSettings;

    fn set_ctime(&mut self, value: u64);

    fn from_vec(&mut self, data: Vec<u8>) -> Result<(), Error>;
    fn to_vec(&self) -> Vec<u8>;

    fn hash(&self) -> Vec<u8>;
}

#[derive(Clone)]
pub struct ContentPDDBSettings {
    dict: String,
    alloc_hint: Option<usize>,
}

pub enum ContentKind {
    TOTP,
    Password,
}

impl ContentKind {
    fn settings(&self) -> ContentPDDBSettings {
        match self {
            ContentKind::TOTP => TotpRecord::default().settings(),
            ContentKind::Password => PasswordRecord::default().settings(),
        }
    }
}

impl Manager {
    pub fn new(_xns: &xous_names::XousNames) -> Manager { Manager { pddb: pddb::Pddb::new() } }

    fn pddb_exists(&self, dict: &str, key_name: &str, basis: Option<String>) -> bool {
        match self.pddb.get(dict, &key_name, basis.as_deref(), false, false, None, Some(vault::basis_change))
        {
            Ok(_) => return true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => {
                log::error!("error while trying to lookup if a key exists, {}", e);
                false
            }
        }
    }

    fn pddb_store(
        &self,
        payload: &[u8],
        dict: &str,
        key_name: &str,
        alloc_hint: Option<usize>,
        basis: Option<String>,
        sync: bool,
        overwrite: bool,
    ) -> Result<(), Error> {
        if !overwrite && self.pddb_exists(dict, key_name, basis.clone()) {
            return Err(Error::KeyExists);
        }

        match self.pddb.get(
            dict,
            &key_name,
            basis.as_deref(),
            true, // if overwrite, we wanna create both the dict and the key if they don't
            // exist
            true,
            alloc_hint,
            Some(vault::basis_change),
        ) {
            Ok(mut data) => match data.write(payload) {
                Ok(_) => match sync {
                    true => Ok(self.pddb.sync().unwrap_or(())),
                    false => Ok(()),
                },
                Err(e) => Err(Error::IoError(e)),
            },
            Err(e) => Err(Error::IoError(e)),
        }
    }

    fn pddb_get(&self, dict: &str, key_name: &str) -> Result<Vec<u8>, Error> {
        match self.pddb.get(dict, key_name, None, false, false, None, Some(vault::basis_change)) {
            Ok(mut record) => {
                let mut data = Vec::<u8>::new();
                record.read_to_end(&mut data)?;
                Ok(data)
            }
            Err(e) => return Err(Error::IoError(e)),
        }
    }

    fn basis_for_key(&self, dict: &str, key_name: &str) -> Result<String, Error> {
        match self.pddb.get(dict, key_name, None, false, false, None, Some(vault::basis_change)) {
            Ok(record) => Ok(record.attributes().expect("couldn't get key attributes").basis),
            Err(e) => return Err(Error::IoError(e)),
        }
    }

    pub fn new_record(
        &mut self,
        record: &mut dyn StorageContent,
        basis: Option<String>,
        overwrite: bool,
    ) -> Result<(), Error> {
        let record = record;
        let settings = record.settings();
        record.set_ctime(utc_now().timestamp() as u64);
        let serialized_record: Vec<u8> = record.to_vec();

        self.pddb_store(
            &serialized_record,
            &settings.dict,
            &hex(record.hash()),
            settings.alloc_hint,
            basis.clone(),
            true,
            overwrite,
        )
    }

    pub fn new_records(
        &mut self,
        records: Vec<Box<dyn StorageContent>>,
        basis: Option<String>,
        overwrite: bool,
    ) -> Result<(), Error> {
        let mut precords = records.into_iter().peekable();
        let mut current_idx = 0; // idk how to use peekable + enumerate
        let mut dupes = vec![];
        while let Some(record) = precords.next() {
            let mut record = record;
            let settings = record.settings();
            record.set_ctime(utc_now().timestamp() as u64);
            let serialized_record: Vec<u8> = record.to_vec();
            let should_sync = precords.peek().is_none() || current_idx % 10 == 0;

            log::debug!(
                "current_idx: {}, should_sync: {}, is_none: {}",
                current_idx,
                should_sync,
                precords.peek().is_none()
            );

            match self.pddb_store(
                &serialized_record,
                &settings.dict,
                &hex(record.hash()),
                settings.alloc_hint,
                basis.clone(),
                should_sync,
                overwrite,
            ) {
                Ok(()) => (),
                Err(error) => match error {
                    Error::KeyExists => {
                        dupes.push(current_idx);
                        ()
                    }
                    _ => return Err(error),
                },
            }

            current_idx += 1;
        }

        if !dupes.is_empty() {
            return Err(Error::DupesExist(dupes));
        }

        Ok(())
    }

    pub fn all<T: StorageContent + std::default::Default>(&self, kind: ContentKind) -> Result<Vec<T>, Error> {
        let settings = kind.settings();

        let keylist = self.pddb.list_keys(&settings.dict, None)?;

        let mut ret = vec![];

        for key in keylist {
            let mut record = T::default();
            record.from_vec(self.pddb_get(&settings.dict, &key)?)?;
            ret.push(record);
        }

        Ok(ret)
    }

    pub fn get_record<T: StorageContent + std::default::Default>(
        &self,
        kind: &ContentKind,
        key_name: &str,
    ) -> Result<T, Error> {
        let settings = kind.settings();
        let mut record = T::default();
        record.from_vec(self.pddb_get(&settings.dict, &key_name)?)?;

        Ok(record)
    }

    pub fn update(
        &mut self,
        kind: &ContentKind,
        key_name: &str,
        record: &mut dyn StorageContent,
    ) -> Result<(), Error> {
        let settings = kind.settings();

        let basis = self.basis_for_key(&settings.dict, key_name)?;
        self.pddb.delete_key(&settings.dict, key_name, Some(&basis))?;

        self.new_record(&mut *record, Some(basis), true)
    }

    pub fn delete(&mut self, kind: ContentKind, key_name: &str) -> Result<(), Error> {
        let settings = kind.settings();

        let basis = self.basis_for_key(&settings.dict, key_name)?;
        self.pddb.delete_key(&settings.dict, key_name, Some(&basis)).map_err(|error| Error::IoError(error))
    }
}

#[derive(Default)]
pub struct TotpRecord {
    pub version: u32,
    // as base32, RFC4648 no padding
    pub secret: String,
    pub name: String,
    pub algorithm: TotpAlgorithm,
    pub notes: String,
    pub digits: u32,
    pub timestep: u64,
    pub ctime: u64,
    pub is_hotp: bool,
}

#[derive(Debug)]
pub enum TOTPSerializationError {
    BadVersion,
    BadAlgorithm,
    BadDigitsAmount,
    BadCtime,
    BadTimestep,
    BadHotp,
    MalformedInput,
}

impl StorageContent for TotpRecord {
    fn settings(&self) -> ContentPDDBSettings {
        ContentPDDBSettings { dict: VAULT_TOTP_DICT.to_string(), alloc_hint: Some(VAULT_TOTP_ALLOC_HINT) }
    }

    fn set_ctime(&mut self, value: u64) { self.ctime = value; }

    fn from_vec(&mut self, data: Vec<u8>) -> Result<(), Error> {
        let desc_str = std::str::from_utf8(&data).or(Err(TOTPSerializationError::MalformedInput))?;

        let mut pr = TotpRecord::default();

        let lines = desc_str.split('\n');
        for line in lines {
            if let Some((tag, data)) = line.split_once(':') {
                match tag {
                    "version" => {
                        if let Ok(ver) = u32::from_str_radix(data, 10) {
                            pr.version = ver
                        } else {
                            log::warn!("ver error");
                            return Err(TOTPSerializationError::BadVersion)?;
                        }
                    }
                    "secret" => pr.secret.push_str(data),
                    "name" => pr.name.push_str(data),
                    "algorithm" => {
                        pr.algorithm = match TotpAlgorithm::try_from(data) {
                            Ok(a) => a,
                            Err(_) => return Err(TOTPSerializationError::BadAlgorithm)?,
                        }
                    }
                    "notes" => pr.notes.push_str(data),
                    "digits" => {
                        if let Ok(digits) = u32::from_str_radix(data, 10) {
                            pr.digits = digits;
                        } else {
                            log::warn!("digits error");
                            return Err(TOTPSerializationError::BadDigitsAmount)?;
                        }
                    }
                    "ctime" => {
                        if let Ok(ctime) = u64::from_str_radix(data, 10) {
                            pr.ctime = ctime;
                        } else {
                            log::warn!("ctime error");
                            return Err(TOTPSerializationError::BadCtime)?;
                        }
                    }
                    "timestep" => {
                        if let Ok(timestep) = u64::from_str_radix(data, 10) {
                            pr.timestep = timestep;
                        } else {
                            log::warn!("timestep error");
                            return Err(TOTPSerializationError::BadTimestep)?;
                        }
                    }
                    "hotp" => {
                        if let Ok(setting) = u8::from_str_radix(data, 10) {
                            if setting != 0 {
                                pr.is_hotp = true;
                            } else {
                                pr.is_hotp = false;
                            }
                        } else {
                            log::warn!("hotp variant error");
                            return Err(TOTPSerializationError::BadHotp)?;
                        }
                    }
                    _ => {
                        log::warn!("unexpected tag {} encountered parsing TOTP info, ignoring", tag);
                    }
                }
            } else {
                log::trace!("invalid line skipped: {:?}", line);
            }
        }

        *self = pr;

        Ok(())
    }

    fn to_vec(&self) -> Vec<u8> {
        format!(
            "{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n",
            "version",
            self.version,
            "secret",
            self.secret,
            "name",
            self.name,
            "algorithm",
            self.algorithm,
            "notes",
            self.notes,
            "digits",
            self.digits,
            "timestep",
            self.timestep,
            "hotp",
            if self.is_hotp { 1 } else { 0 },
            "ctime",
            self.ctime,
        )
        .into_bytes()
    }

    fn hash(&self) -> Vec<u8> {
        let mut h = ctap_crypto::sha256::Sha256::new();
        h.update(self.name.as_bytes());
        h.finalize().to_vec()
    }
}

impl TryFrom<Vec<u8>> for TotpRecord {
    type Error = TOTPSerializationError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        let desc_str = std::str::from_utf8(&value).or(Err(TOTPSerializationError::MalformedInput))?;

        let mut pr = TotpRecord {
            version: VAULT_TOTP_REC_VERSION,
            secret: String::new(),
            name: String::new(),
            algorithm: TotpAlgorithm::HmacSha1,
            notes: String::new(),
            digits: 0,
            ctime: 0,
            timestep: 0,
            is_hotp: false,
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
                        pr.algorithm = match TotpAlgorithm::try_from(data) {
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
                    "hotp" => {
                        if let Ok(setting) = u8::from_str_radix(data, 10) {
                            if setting != 0 {
                                pr.is_hotp = true;
                            } else {
                                pr.is_hotp = false;
                            }
                        } else {
                            log::warn!("hotp error");
                            return Err(TOTPSerializationError::BadHotp);
                        }
                    }
                    _ => {
                        log::warn!("unexpected tag {} encountered parsing TOTP info, ignoring", tag);
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
        format!(
            "{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n",
            "version",
            tr.version,
            "secret",
            tr.secret,
            "name",
            tr.name,
            "algorithm",
            tr.algorithm,
            "notes",
            tr.notes,
            "digits",
            tr.digits,
            "timestep",
            tr.timestep,
            "hotp",
            if tr.is_hotp { 1 } else { 0 },
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

#[derive(Default)]
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
impl PasswordRecord {
    pub fn alloc() -> Self {
        // The intent is only one of these is allocated, and it is re-used.
        // Sizes picked to be big enough to probably avoid re-allocs,
        // yet small enough to not be unreasonable for a temporary buffer.
        PasswordRecord {
            version: 0,
            description: String::with_capacity(256),
            username: String::with_capacity(256),
            password: String::with_capacity(256),
            notes: String::with_capacity(1024),
            ctime: 0,
            atime: 0,
            count: 0,
        }
    }

    pub fn clear(&mut self) {
        self.description.clear();
        self.username.clear();
        self.password.clear();
        self.notes.clear();
        self.version = 0;
        self.ctime = 0;
        self.atime = 0;
        self.count = 0;
    }
}

impl StorageContent for PasswordRecord {
    fn settings(&self) -> ContentPDDBSettings {
        ContentPDDBSettings { dict: VAULT_PASSWORD_DICT.to_string(), alloc_hint: Some(VAULT_TOTP_ALLOC_HINT) }
    }

    fn set_ctime(&mut self, value: u64) { self.ctime = value; }

    fn from_vec(&mut self, data: Vec<u8>) -> Result<(), Error> {
        self.clear();
        // use `std::str` so we're allocating this temporary on the stack
        let desc_str = std::str::from_utf8(&data).or(Err(PasswordSerializationError::MalformedInput))?;

        let lines = desc_str.split('\n');
        for line in lines {
            if let Some((tag, data)) = line.split_once(':') {
                match tag {
                    "version" => {
                        if let Ok(ver) = u32::from_str_radix(data, 10) {
                            self.version = ver
                        } else {
                            log::warn!("ver error");
                            return Err(PasswordSerializationError::BadVersion)?;
                        }
                    }
                    "description" => self.description.push_str(data),
                    "username" => self.username.push_str(data),
                    "password" => self.password.push_str(data),
                    "notes" => self.notes.push_str(data),
                    "ctime" => {
                        if let Ok(ctime) = u64::from_str_radix(data, 10) {
                            self.ctime = ctime;
                        } else {
                            log::warn!("ctime error");
                            return Err(PasswordSerializationError::BadCtime)?;
                        }
                    }
                    "atime" => {
                        if let Ok(atime) = u64::from_str_radix(data, 10) {
                            self.atime = atime;
                        } else {
                            log::warn!("atime error");
                            return Err(PasswordSerializationError::BadAtime)?;
                        }
                    }
                    "count" => {
                        if let Ok(count) = u64::from_str_radix(data, 10) {
                            self.count = count;
                        } else {
                            log::warn!("count error");
                            return Err(PasswordSerializationError::BadCount)?;
                        }
                    }
                    _ => {
                        log::warn!("unexpected tag {} encountered parsing password info, ignoring", tag);
                    }
                }
            } else {
                log::trace!("invalid line skipped: {:?}", line);
            }
        }
        Ok(())
    }

    fn to_vec(&self) -> Vec<u8> {
        format!(
            "{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n",
            "version",
            self.version,
            "description",
            self.description,
            "username",
            self.username,
            "password",
            self.password,
            "notes",
            self.notes,
            "ctime",
            self.ctime,
            "atime",
            self.atime,
            "count",
            self.count,
        )
        .into_bytes()
    }

    fn hash(&self) -> Vec<u8> {
        let mut h = ctap_crypto::sha256::Sha256::new();
        h.update(self.description.as_bytes());
        h.update(self.username.as_bytes());
        h.finalize().to_vec()
    }
}

impl TryFrom<Vec<u8>> for PasswordRecord {
    type Error = PasswordSerializationError;

    fn try_from(data: Vec<u8>) -> Result<Self, Self::Error> {
        let desc_str = String::from_utf8(data).or(Err(PasswordSerializationError::MalformedInput))?;

        let mut pr = PasswordRecord {
            version: VAULT_PASSWORD_REC_VERSION,
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
                        log::warn!("unexpected tag {} encountered parsing password info, ignoring", tag);
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

/// because we don't get Utc::now, as the crate checks your architecture and xous is not recognized as a valid
/// target
fn utc_now() -> DateTime<Utc> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).expect("system time before Unix epoch");
    let naive = NaiveDateTime::from_timestamp(now.as_secs() as i64, now.subsec_nanos() as u32);
    DateTime::from_utc(naive, Utc)
}

pub fn hex(data: Vec<u8>) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(2 * data.len());
    for byte in data {
        write!(s, "{:02X}", byte).unwrap();
    }

    s
}
