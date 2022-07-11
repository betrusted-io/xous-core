use chrono::{DateTime, NaiveDateTime, Utc};
use std::convert::TryFrom;
use std::{
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

    pub fn new_totp_record(&mut self, record: TotpRecord) -> Result<(), std::io::Error> {
        let mut record = record;
        record.ctime = utc_now().timestamp() as u64;
        let serialized_record: Vec<u8> = record.into();
        let guid = self.gen_guid();

        match self.pddb.get(
            VAULT_TOTP_DICT,
            &guid,
            None,
            true,
            true,
            Some(VAULT_TOTP_ALLOC_HINT),
            Some(crate::basis_change),
        ) {
            Ok(mut data) => match data.write(&serialized_record) {
                Ok(_) => Ok(self.pddb.sync().unwrap_or(())),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        }
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

/// because we don't get Utc::now, as the crate checks your architecture and xous is not recognized as a valid target
fn utc_now() -> DateTime<Utc> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before Unix epoch");
    let naive = NaiveDateTime::from_timestamp(now.as_secs() as i64, now.subsec_nanos() as u32);
    DateTime::from_utc(naive, Utc)
}
