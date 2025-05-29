use std::fmt::Display;

use cbor::{self, cbor_array_vec, cbor_int, cbor_map, cbor_unsigned, destructure_cbor_map};
use serde::{Deserialize, Serialize};

pub const CONTINUE_RESPONSE: &[u8] = &[42, 43, 44, 45];
pub const OKAY_CANARY: &[u8] = &[0xca, 0xfe, 0xba, 0xbe];
pub const ERROR_VENDOR_HANDLING: u8 = 0x35;
pub const VENDOR_SESSION_ERROR: u8 = 0x36;

#[derive(Debug)]
pub enum CborConversionError {
    BadCbor,
    UnknownAlgorithm(u64),
    UnknownPayloadType(u8),
    WrongPayloadSize,
}

impl Display for CborConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CborConversionError::BadCbor => write!(f, "bad cbor"),
            CborConversionError::UnknownAlgorithm(algo) => write!(f, "unknown algorithm {}", algo),
            CborConversionError::UnknownPayloadType(pt) => write!(f, "unknown payload type {}", pt),
            CborConversionError::WrongPayloadSize => write!(f, "wrong payload size"),
        }
    }
}

impl std::error::Error for CborConversionError {}

#[derive(Debug)]
pub enum HashFromStrError {
    UnknownHash,
}

impl Display for HashFromStrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HashFromStrError::UnknownHash => write!(f, "unknown hash type"),
        }
    }
}

impl std::error::Error for HashFromStrError {}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum HashAlgorithms {
    #[default]
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
    type Error = CborConversionError;

    fn try_from(value: cbor::Value) -> Result<Self, Self::Error> {
        let v = extract_unsigned(value)?;
        match v {
            1 => Ok(HashAlgorithms::SHA1),
            2 => Ok(HashAlgorithms::SHA256),
            3 => Ok(HashAlgorithms::SHA512),
            _ => Err(CborConversionError::UnknownAlgorithm(v)),
        }
    }
}

impl std::str::FromStr for HashAlgorithms {
    type Err = HashFromStrError;

    fn from_str(input: &str) -> Result<HashAlgorithms, Self::Err> {
        match input {
            "SHA1" => Ok(HashAlgorithms::SHA1),
            "SHA256" => Ok(HashAlgorithms::SHA256),
            "SHA512" => Ok(HashAlgorithms::SHA512),
            _ => Err(HashFromStrError::UnknownHash),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TotpEntry {
    pub step_seconds: u64,
    pub shared_secret: String,
    pub digit_count: u32,
    pub algorithm: HashAlgorithms,
    pub name: String,
    #[serde(default)] // if hotp is missing from the JSON representation, it's assumed to be false.
    pub hotp: bool,
}

impl From<TotpEntry> for cbor::Value {
    fn from(te: TotpEntry) -> Self {
        cbor_map! {
            cbor_int!(1) => te.step_seconds as i64,
            cbor_int!(2) => te.shared_secret,
            cbor_int!(3) => te.digit_count as i64,
            cbor_int!(4) => te.algorithm,
            cbor_int!(5) => te.name,
        }
    }
}

impl TryFrom<cbor::Value> for TotpEntry {
    type Error = CborConversionError;

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
                6 => hotp,
            } = rawmap;
        }

        let step_seconds = extract_unsigned(step_seconds.unwrap())?;
        let shared_secret = extract_string(shared_secret.unwrap())?;
        let digit_count = extract_unsigned(digit_count.unwrap())? as u32;
        let algorithm: HashAlgorithms = algorithm.unwrap().try_into()?;
        let name = extract_string(name.unwrap())?;
        let hotp =
            extract_bool(hotp.unwrap_or(cbor::Value::Simple(cbor::SimpleValue::FalseValue))).unwrap_or(false);

        Ok(TotpEntry { step_seconds, shared_secret, digit_count, algorithm, name, hotp })
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct TotpEntries(pub Vec<TotpEntry>);

impl From<&TotpEntries> for cbor::Value {
    fn from(te: &TotpEntries) -> Self { cbor_array_vec!(te.0.clone()) }
}

impl TryFrom<cbor::Value> for TotpEntries {
    type Error = CborConversionError;

    fn try_from(value: cbor::Value) -> Result<Self, Self::Error> {
        let raw = extract_array(value)?;
        let mut ret = vec![];

        for e in raw {
            ret.push(e.try_into().unwrap())
        }

        Ok(Self(ret))
    }
}

impl From<&TotpEntries> for Vec<u8> {
    fn from(te: &TotpEntries) -> Self {
        let mut ret = vec![];
        let te_cbor: cbor::Value = te.into();
        cbor::write(te_cbor, &mut ret).ok();
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
            cbor_int!(1) => te.description,
            cbor_int!(2) => te.username,
            cbor_int!(3) => te.password,
            cbor_int!(4) => te.notes,
        }
    }
}

impl TryFrom<cbor::Value> for PasswordEntry {
    type Error = CborConversionError;

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

        Ok(PasswordEntry { description, username, password, notes })
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct PasswordEntries(pub Vec<PasswordEntry>);

impl From<&PasswordEntries> for cbor::Value {
    fn from(te: &PasswordEntries) -> Self { cbor_array_vec!(te.0.clone()) }
}

impl From<&PasswordEntries> for Vec<u8> {
    fn from(te: &PasswordEntries) -> Self {
        let mut ret = vec![];
        let te_cbor: cbor::Value = te.into();
        cbor::write(te_cbor, &mut ret).ok();
        ret
    }
}

impl TryFrom<cbor::Value> for PasswordEntries {
    type Error = CborConversionError;

    fn try_from(value: cbor::Value) -> Result<Self, Self::Error> {
        let raw = extract_array(value)?;
        let mut ret = vec![];

        for e in raw {
            ret.push(e.try_into().unwrap())
        }

        Ok(Self(ret))
    }
}

#[derive(Debug, Serialize, Deserialize)]
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
        cbor::write(te_cbor, &mut ret).ok();
        ret
    }
}

impl From<DataPacket> for cbor::Value {
    fn from(dp: DataPacket) -> Self {
        let dpt = dp.type_to_int();
        match dp {
            DataPacket::Password(ref p) => {
                cbor_map! {
                    cbor_int!(1) => dpt,
                    cbor_int!(2) => p,
                }
            }
            DataPacket::TOTP(ref t) => {
                cbor_map! {
                    cbor_int!(1) => dpt,
                    cbor_int!(2) => t,
                }
            }
        }
    }
}

impl TryFrom<cbor::Value> for DataPacket {
    type Error = CborConversionError;

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
            others => panic!("cannot convert from data packet type {} from cbor::Value!", others),
        };

        Ok(dp)
    }
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

#[derive(Debug)]
pub enum PayloadTypeError {
    BadType,
}

impl TryFrom<&Vec<u8>> for PayloadType {
    type Error = PayloadTypeError;

    fn try_from(u: &Vec<u8>) -> Result<PayloadType, Self::Error> {
        if u.is_empty() {
            return Err(PayloadTypeError::BadType);
        }

        match u[0] {
            1 => Ok(PayloadType::TOTP),
            2 => Ok(PayloadType::Password),
            _ => Err(PayloadTypeError::BadType),
        }
    }
}

pub struct PayloadSize(pub u64);

impl TryFrom<cbor::Value> for PayloadSize {
    type Error = CborConversionError;

    fn try_from(value: cbor::Value) -> Result<Self, Self::Error> {
        match extract_unsigned(value) {
            Ok(value) => Ok(PayloadSize(value)),
            Err(error) => Err(error),
        }
    }
}

impl From<&PayloadSize> for cbor::Value {
    fn from(te: &PayloadSize) -> Self {
        cbor_map! {
            cbor_int!(0) => te.0,
        }
    }
}

impl From<&PayloadSize> for Vec<u8> {
    fn from(te: &PayloadSize) -> Self {
        let mut ret = vec![];
        let te_cbor: cbor::Value = te.into();
        cbor::write(te_cbor, &mut ret).ok();
        ret
    }
}

#[derive(Debug)]
pub struct Wire {
    pub index: u64,
    pub size: u64,
    pub more_data: bool,
    pub data: Vec<u8>,
}

impl Wire {
    const MAX_DATA: u16 = 7588;
}

pub type Wires = Vec<Wire>;

impl From<DataPacket> for Wires {
    fn from(dp: DataPacket) -> Self {
        let mut ret = vec![];
        let bytes: Vec<u8> = dp.into();

        for (idx, chunk) in bytes.chunks(Wire::MAX_DATA.into()).into_iter().enumerate() {
            ret.push(Wire {
                index: idx as u64,
                size: chunk.len() as u64,
                more_data: true,
                data: chunk.to_vec(),
            })
        }

        if let Some(last) = ret.last_mut() {
            last.more_data = false;
        }
        ret
    }
}

impl TryFrom<Vec<Wire>> for DataPacket {
    type Error = CborConversionError;

    fn try_from(vw: Vec<Wire>) -> Result<Self, Self::Error> {
        let mut orig_data = vec![];
        for chunk in vw {
            orig_data.push(chunk.data[chunk.size as usize]);
        }

        let dp = cbor::read(&orig_data).map_err(|_| CborConversionError::BadCbor)?;

        DataPacket::try_from(dp)
    }
}

impl From<&Wire> for cbor::Value {
    fn from(te: &Wire) -> Self {
        cbor_map! {
            cbor_int!(1) => te.index,
            cbor_int!(2) => te.size,
            cbor_int!(3) => te.more_data,
            cbor_int!(4) => te.data.clone(),
        }
    }
}

impl From<&Wire> for Vec<u8> {
    fn from(te: &Wire) -> Self {
        let mut ret = vec![];
        let te_cbor: cbor::Value = te.into();
        cbor::write(te_cbor, &mut ret).ok();
        ret
    }
}

impl TryFrom<cbor::Value> for Wire {
    type Error = CborConversionError;

    fn try_from(value: cbor::Value) -> Result<Self, Self::Error> {
        let rawmap = match value {
            cbor::Value::Map(m) => m,
            _ => return Err(CborConversionError::BadCbor),
        };

        destructure_cbor_map! {
            let {
                1 => index,
                2 => size,
                3 => more_data,
                4 => data,
            } = rawmap;
        }

        let index = extract_unsigned(index.unwrap())?;
        let size = extract_unsigned(size.unwrap())?;
        let data = extract_byte_string(data.unwrap())?;
        let more_data = extract_bool(more_data.unwrap())?;

        Ok(Self { index, more_data, size, data })
    }
}

fn extract_bool(cbor_value: cbor::Value) -> Result<bool, CborConversionError> {
    match cbor_value {
        cbor::Value::Simple(cbor::SimpleValue::FalseValue) => Ok(false),
        cbor::Value::Simple(cbor::SimpleValue::TrueValue) => Ok(true),
        _ => Err(CborConversionError::BadCbor),
    }
}

fn extract_unsigned(cbor_value: cbor::Value) -> Result<u64, CborConversionError> {
    match cbor_value {
        cbor::Value::Unsigned(unsigned) => Ok(unsigned),
        _ => Err(CborConversionError::BadCbor),
    }
}

fn extract_byte_string(cbor_value: cbor::Value) -> Result<Vec<u8>, CborConversionError> {
    match cbor_value {
        cbor::Value::ByteString(byte_string) => Ok(byte_string),
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
        cbor::Value::TextString(text_string) => Ok(text_string),
        _ => Err(CborConversionError::BadCbor),
    }
}
