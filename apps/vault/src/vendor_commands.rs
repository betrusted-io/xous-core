use cbor::{destructure_cbor_map, reader::DecoderError};
use chrono::{DateTime, NaiveDateTime, Utc};
use locales::t;
use pddb::Pddb;
use std::{time::{SystemTime, UNIX_EPOCH}, io::Write};

const VAULT_TOTP_DICT: &'static str = "vault.totp";
/// bytes to reserve for a key entry. Making this slightly larger saves on some churn as stuff gets updated
const VAULT_TOTP_ALLOC_HINT: usize = 256;

use crate::{
    actions::{TotpRecord, serialize_totp},
    ctap::hid::{send::HidPacketIterator, ChannelID, CtapHid, Message},
};

// Vault-specific command to upload TOTP codes
pub const COMMAND_RESTORE_TOTP_CODES: u8 = 0x41;
pub const COMMAND_BENCHMARK: u8 = 0x42;

pub fn handle_vendor_command(
    cmd: u8,
    channel_id: ChannelID,
    payload: Vec<u8>,
) -> HidPacketIterator {
    log::debug!("got vendor command: {}, payload: {:?}", cmd, payload);
    let xns = xous_names::XousNames::new().unwrap();
    let mut trng = trng::Trng::new(&xns).unwrap();

    let payload = match cmd {
        COMMAND_BENCHMARK => Message {
            cid: channel_id,
            cmd: cmd,
            payload: handle_benchmark(),
        },
        COMMAND_RESTORE_TOTP_CODES => match handle_restore(payload, &mut trng) {
            Ok(payload) => Message {
                cid: channel_id,
                cmd: cmd,
                payload: payload,
            },
            Err(_) => error_message(channel_id, 45),
        },
        _ => error_message(channel_id, 0x33),
    };

    HidPacketIterator::new(payload).unwrap()
}

fn handle_benchmark() -> Vec<u8> {
    vec![0xDE, 0xAD, 0xBE, 0xEF]
}

#[derive(Debug)]
enum RestoreError {
    CborError(DecoderError),
    NotAnArray,
    NotAMap,
    PddbError(std::io::Error),
}

impl From<DecoderError> for RestoreError {
    fn from(de: DecoderError) -> Self {
        RestoreError::CborError(de)
    }
}

fn handle_restore(data: Vec<u8>, trng: &mut trng::Trng) -> Result<Vec<u8>, RestoreError> {
    // Unmarshal bytes to cbor map
    let c = cbor::read(&data)?;

    let array_c = match c {
        cbor::Value::Array(array) => array,
        _ => return Err(RestoreError::NotAnArray),
    };

    let pddb = Pddb::new();

    for elem in array_c {
        let rawmap = match elem {
            cbor::Value::Map(m) => m,
            _ => return Err(RestoreError::NotAMap),
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

        let totp = TotpRecord {
            version: 1,
            name: extract_string(name.unwrap()).unwrap(),
            secret: extract_string(shared_secret.unwrap()).unwrap(),
            algorithm: crate::totp::TotpAlgorithm::HmacSha1,
            digits: extract_unsigned(digit_count.unwrap()).unwrap() as u32,
            timestep: extract_unsigned(step_seconds.unwrap()).unwrap(),
            ctime: utc_now().timestamp() as u64,
            notes: t!("vault.notes", xous::LANG).to_string(),
        };

        let ser = serialize_totp(&totp);
        let guid = gen_guid(trng);
        log::debug!("storing into guid: {}", guid);
        match pddb.get(
            VAULT_TOTP_DICT,
            &guid,
            None,
            true,
            true,
            Some(VAULT_TOTP_ALLOC_HINT),
            Some(crate::basis_change),
        ) {
            Ok(mut data) => match data.write(&ser) {
                Ok(len) => log::debug!("wrote {} bytes", len),
                Err(e) => return Err(RestoreError::PddbError(e))
            },
            Err(e) => return Err(RestoreError::PddbError(e)),
        }
        log::debug!("syncing...");
        pddb.sync().ok();
    }

    Ok(vec![0xca, 0xfe, 0xba, 0xbe])
}

fn error_message(cid: ChannelID, error_code: u8) -> Message {
    // This unwrap is safe because the payload length is 1 <= 7609 bytes.
    Message {
        cid,
        cmd: 0x3F, // COMMAND_ERROR
        payload: vec![error_code],
    }
}

#[derive(Debug)]
enum CborConversionError {
    BadCbor,
    UnknownAlgorithm(u64),
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

fn extract_string(cbor_value: cbor::Value) -> Result<String, CborConversionError> {
    match cbor_value {
        cbor::Value::KeyValue(cbor::KeyType::TextString(string)) => Ok(string),
        _ => Err(CborConversionError::BadCbor),
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

fn gen_guid(trng: &mut trng::Trng) -> String {
    let mut guid = [0u8; 16];
    trng.fill_bytes(&mut guid);
    hex::encode(guid)
}