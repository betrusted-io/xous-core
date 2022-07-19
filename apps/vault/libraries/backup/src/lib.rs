use std::fmt::Display;

use cbor::{self, cbor_array_vec, cbor_key_int, cbor_map, cbor_unsigned, destructure_cbor_map};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum CborConversionError {
    BadCbor,
    UnknownAlgorithm(u64),
    UnknownPayloadType(u8),
}

impl Display for CborConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CborConversionError::BadCbor => write!(f, "bad cbor"),
            CborConversionError::UnknownAlgorithm(algo) => write!(f, "unknown algorithm {}", algo),
            CborConversionError::UnknownPayloadType(pt) => write!(f, "unknown payload type {}", pt),
        }
    }
}

impl std::error::Error for CborConversionError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HashAlgorithms {
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
pub struct TotpEntry {
    pub step_seconds: u64,
    pub shared_secret: String,
    pub digit_count: u32,
    pub algorithm: HashAlgorithms,
    pub name: String,
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
        let digit_count = extract_unsigned(digit_count.unwrap())? as u32;
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
pub struct TotpEntries(pub Vec<TotpEntry>);

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

impl From<&TotpEntries> for Vec<u8> {
    fn from(te: &TotpEntries) -> Self {
        let mut ret = vec![];
        let te_cbor: cbor::Value = te.into();
        cbor::write(te_cbor, &mut ret);
        ret
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct PasswordEntry {
    pub description: String,
    pub username: String,
    pub password: String,
    pub notes: String,
}

impl From<PasswordEntry> for cbor::Value {
    fn from(te: PasswordEntry) -> Self {
        cbor_map! {
            cbor_key_int!(1) => te.description,
            cbor_key_int!(2) => te.username,
            cbor_key_int!(3) => te.password,
            cbor_key_int!(4) => te.notes,
        }
    }
}

impl TryFrom<cbor::Value> for PasswordEntry {
    fn try_from(value: cbor::Value) -> Result<Self, Self::Error> {
        let rawmap = match value {
            cbor::Value::Map(m) => m,
            _ => return Err(CborConversionError::BadCbor),
        };

        destructure_cbor_map! {
            let {
                1 => description,
                2 => username,
                3 => password,
                4 => notes,
            } = rawmap;
        }

        let description = extract_string(description.unwrap())?;
        let username = extract_string(username.unwrap())?;
        let password = extract_string(password.unwrap())?;
        let notes = extract_string(notes.unwrap())?;

        Ok(PasswordEntry {
            description,
            username,
            password,
            notes,
        })
    }

    type Error = CborConversionError;
}

#[derive(Serialize, Deserialize)]
pub struct PasswordEntries(pub Vec<PasswordEntry>);

impl From<&PasswordEntries> for cbor::Value {
    fn from(te: &PasswordEntries) -> Self {
        cbor_array_vec!(te.0.clone())
    }
}

impl From<&PasswordEntries> for Vec<u8> {
    fn from(te: &PasswordEntries) -> Self {
        let mut ret = vec![];
        let te_cbor: cbor::Value = te.into();
        cbor::write(te_cbor, &mut ret);
        ret
    }
}

impl TryFrom<cbor::Value> for PasswordEntries {
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

pub enum DataPacket {
    Password(PasswordEntries),
    TOTP(TotpEntries),
}

impl DataPacket {
    fn type_to_int(&self) -> u64 {
        match self {
            DataPacket::Password(_) => 1,
            DataPacket::TOTP(_) => 2,
        }
    }
}

impl From<DataPacket> for Vec<u8> {
    fn from(dp: DataPacket) -> Self {
        let mut ret = vec![];
        let te_cbor: cbor::Value = dp.into();
        cbor::write(te_cbor, &mut ret);
        ret
    }
}

impl From<DataPacket> for cbor::Value {
    fn from(dp: DataPacket) -> Self {
        let dpt = dp.type_to_int();
        match dp {
            DataPacket::Password(ref p) => {
                cbor_map! {
                    cbor_key_int!(1) => dpt,
                    cbor_key_int!(2) => p,
                }
            }
            DataPacket::TOTP(ref t) => {
                cbor_map! {
                    cbor_key_int!(1) => dpt,
                    cbor_key_int!(2) => t,
                }
            }
        }
    }
}

impl TryFrom<cbor::Value> for DataPacket {
    fn try_from(value: cbor::Value) -> Result<Self, Self::Error> {
        let rawmap = match value {
            cbor::Value::Map(m) => m,
            _ => return Err(CborConversionError::BadCbor),
        };

        destructure_cbor_map! {
            let {
                1 => data_packet_type,
                2 => shared_secret,
            } = rawmap;
        }

        let data_packet_type = extract_unsigned(data_packet_type.unwrap())?;
        let shared_secret = shared_secret.unwrap();
        let dp = match data_packet_type {
            1 => {
                // DataPacket::Password
                let pes: PasswordEntries = shared_secret.try_into()?;
                DataPacket::Password(pes)
            }
            2 => {
                // DataPacket::TOTP
                let pes: TotpEntries = shared_secret.try_into()?;
                DataPacket::TOTP(pes)
            }
            others => panic!(
                "cannot convert from data packet type {} from cbor::Value!",
                others
            ),
        };

        Ok(dp)
    }

    type Error = CborConversionError;
}

pub enum PayloadType {
    TOTP,
    Password,
}

impl From<&PayloadType> for u8 {
    fn from(t: &PayloadType) -> u8 {
        match t {
            PayloadType::TOTP => 1,
            PayloadType::Password => 2,
        }
    }
}

impl From<Vec<u8>> for PayloadType {
    fn from(u: Vec<u8>) -> PayloadType {
        match u[0] {
            1 => PayloadType::TOTP,
            2 => PayloadType::Password,
            _ => PayloadType::TOTP,
        }
    }
}

fn extract_unsigned(cbor_value: cbor::Value) -> Result<u64, CborConversionError> {
    match cbor_value {
        cbor::Value::KeyValue(cbor::KeyType::Unsigned(unsigned)) => Ok(unsigned),
        _ => Err(CborConversionError::BadCbor),
    }
}

// fn extract_byte_string(cbor_value: cbor::Value) -> Result<Vec<u8>, CborConversionError> {
//     match cbor_value {
//         cbor::Value::KeyValue(cbor::KeyType::ByteString(byte_string)) => Ok(byte_string),
//         _ => Err(CborConversionError::BadCbor),
//     }
// }

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
