use argh::FromArgs;
use cbor::{
    self, cbor_array_vec, cbor_int, cbor_key_int, cbor_map, cbor_unsigned, destructure_cbor_map,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, error::Error};

#[derive(FromArgs, PartialEq, Debug)]
#[argh(description = "A tool to backup/restore TOTP settings for the vault app via USB.")]
struct WithPositional {
    #[argh(option)]
    #[argh(description = "backup TOTP settings from device to file")]
    backup: Option<String>,

    #[argh(option)]
    #[argh(description = "restore TOTP settings from file to device")]
    restore: Option<String>,
}

enum CLIAction {
    Backup(String),
    Restore(String),
}

impl TryInto<CLIAction> for WithPositional {
    type Error = ProgramError;

    fn try_into(self) -> Result<CLIAction, Self::Error> {
        if self.backup.is_some() && self.restore.is_some() {
            return Err(ProgramError::CantBackupAndRestoreAtTheSameTime);
        }

        if self.backup.is_some() {
            return Ok(CLIAction::Backup(self.backup.unwrap()));
        }

        if self.restore.is_some() {
            return Ok(CLIAction::Restore(self.restore.unwrap()));
        }

        panic!("impossible!")
    }
}

#[derive(Debug)]
enum CborConversionError {
    BadCbor,
    UnknownAlgorithm(u64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum HashAlgorithms {
    SHA1,
    SHA256,
    SHA512,
}

impl From<HashAlgorithms> for cbor::Value {
    fn from(te: HashAlgorithms) -> Self {
        match te {
            HashAlgorithms::SHA1 => cbor_unsigned!(1),
            HashAlgorithms::SHA256 => cbor_unsigned!(2),
            HashAlgorithms::SHA512 => cbor_unsigned!(3),
        }
    }
}

impl TryFrom<cbor::Value> for HashAlgorithms {
    fn try_from(value: cbor::Value) -> Result<Self, Self::Error> {
        match value {
            cbor::Value::KeyValue(cbor::KeyType::Unsigned(unsigned)) => match unsigned {
                1 => Ok(HashAlgorithms::SHA1),
                2 => Ok(HashAlgorithms::SHA256),
                3 => Ok(HashAlgorithms::SHA512),
                other => Err(CborConversionError::UnknownAlgorithm(other)),
            },
            _ => Err(CborConversionError::BadCbor),
        }
    }

    type Error = CborConversionError;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TotpEntry {
    step_seconds: u64,
    shared_secret: String,
    digit_count: u64,
    algorithm: HashAlgorithms,
    name: String,
}

impl From<TotpEntry> for cbor::Value {
    fn from(te: TotpEntry) -> Self {
        cbor_map! {
            cbor_key_int!(1) => te.step_seconds as i64,
            cbor_key_int!(2) => te.shared_secret,
            cbor_key_int!(3) => te.digit_count as i64,
            cbor_key_int!(4) => te.algorithm,
            cbor_key_int!(5) => te.name,
        }
    }
}

impl TryFrom<cbor::Value> for TotpEntry {
    fn try_from(value: cbor::Value) -> Result<Self, Self::Error> {
        let rawmap = match value {
            cbor::Value::Map(m) => m,
            _ => return Err(CborConversionError::BadCbor),
        };

        destructure_cbor_map! {
            let {
                1 => step_seconds,
                2 => shared_secret,
                3 => digit_count,
                4 => algorithm,
                5 => name,
            } = rawmap;
        }

        let step_seconds = extract_unsigned(step_seconds.unwrap())?;
        let shared_secret = extract_string(shared_secret.unwrap())?;
        let digit_count = extract_unsigned(digit_count.unwrap())?;
        let algorithm: HashAlgorithms = algorithm.unwrap().try_into()?;
        let name = extract_string(name.unwrap())?;

        Ok(TotpEntry {
            step_seconds,
            shared_secret,
            digit_count,
            algorithm,
            name,
        })
    }

    type Error = CborConversionError;
}

#[derive(Serialize, Deserialize)]
struct TotpEntries(Vec<TotpEntry>);

impl From<&TotpEntries> for cbor::Value {
    fn from(te: &TotpEntries) -> Self {
        cbor_array_vec!(te.0.clone())
    }
}

impl TryFrom<cbor::Value> for TotpEntries {
    fn try_from(value: cbor::Value) -> Result<Self, Self::Error> {
        let raw = extract_array(value)?;
        let mut ret = vec![];

        for e in raw {
            ret.push(e.try_into().unwrap())
        }

        Ok(Self(ret))
    }

    type Error = CborConversionError;
}

impl TotpEntries {
    fn bytes(&self) -> Vec<u8> {
        let mut ret = vec![];
        cbor::write(self.into(), &mut ret);
        ret
    }
}

#[derive(Debug)]
enum ProgramError {
    NoDevicesFound,
    CantBackupAndRestoreAtTheSameTime,
    DeviceError(Vec<u8>),
}

impl std::fmt::Display for ProgramError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProgramError::NoDevicesFound => write!(f, "no CTAP2 devices found"),
            ProgramError::CantBackupAndRestoreAtTheSameTime => {
                write!(f, "can't backup and restore at the same time")
            }
            ProgramError::DeviceError(code) => write!(f, "device returned code {:?}", code),
        }
    }
}

impl std::error::Error for ProgramError {}

const OKAY_CANARY: &[u8] = &[0xca, 0xfe, 0xba, 0xbe];

fn main() -> Result<(), Box<dyn Error>> {
    let wp: WithPositional = argh::from_env();
    let argument: CLIAction = wp.try_into()?;

    let ha = hidapi::HidApi::new()?;
    let dl = ha.device_list();

    let mut precursor: Option<&hidapi::DeviceInfo> = None;
    for i in dl {
        if i.product_string().unwrap() == "Precursor" {
            precursor = Some(i);
            break;
        }
    }

    if precursor.is_none() {
        return Err(Box::new(ProgramError::NoDevicesFound));
    }

    let device = ctaphid::Device::connect(&ha, precursor.unwrap())?;

    match argument {
        CLIAction::Backup(path) => todo!(),
        CLIAction::Restore(path) => {
            let hbf = read_human_backup_file(&path)?;
            let vcres = device.vendor_command(ctaphid::command::VendorCommand::H41, &hbf)?;

            if vcres.ne(OKAY_CANARY) {
                return Err(Box::new(ProgramError::DeviceError(vcres)));
            }

            Ok(())
        }
    }
}

fn read_human_backup_file(path: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let f = std::fs::File::open(path)?;

    let backup_json: TotpEntries = serde_json::from_reader(f)?;

    Ok(backup_json.bytes())
}

fn extract_unsigned(cbor_value: cbor::Value) -> Result<u64, CborConversionError> {
    match cbor_value {
        cbor::Value::KeyValue(cbor::KeyType::Unsigned(unsigned)) => Ok(unsigned),
        _ => Err(CborConversionError::BadCbor),
    }
}

fn extract_byte_string(cbor_value: cbor::Value) -> Result<Vec<u8>, CborConversionError> {
    match cbor_value {
        cbor::Value::KeyValue(cbor::KeyType::ByteString(byte_string)) => Ok(byte_string),
        _ => Err(CborConversionError::BadCbor),
    }
}

fn extract_array(cbor_value: cbor::Value) -> Result<Vec<cbor::Value>, CborConversionError> {
    match cbor_value {
        cbor::Value::Array(array) => Ok(array),
        _ => Err(CborConversionError::BadCbor),
    }
}

fn extract_string(cbor_value: cbor::Value) -> Result<String, CborConversionError> {
    match cbor_value {
        cbor::Value::KeyValue(cbor::KeyType::TextString(string)) => Ok(string),
        _ => Err(CborConversionError::BadCbor),
    }
}