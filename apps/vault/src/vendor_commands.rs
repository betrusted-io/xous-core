use cbor::reader::DecoderError;
use core::convert::TryFrom;
use locales::t;

use crate::{
    ctap::hid::{send::HidPacketIterator, ChannelID, Message},
    storage::{Error, TotpRecord},
};

// Vault-specific command to upload TOTP codes
pub const COMMAND_RESTORE_TOTP_CODES: u8 = 0x41;
pub const COMMAND_BACKUP_TOTP_CODES: u8 = 0x42;
pub const COMMAND_BENCHMARK: u8 = 0x43;

pub fn handle_vendor_command(
    cmd: u8,
    channel_id: ChannelID,
    payload: Vec<u8>,
) -> HidPacketIterator {
    log::debug!("got vendor command: {}, payload: {:?}", cmd, payload);
    let xns = xous_names::XousNames::new().unwrap();

    let payload = match cmd {
        COMMAND_BENCHMARK => Message {
            cid: channel_id,
            cmd: cmd,
            payload: handle_benchmark(),
        },
        COMMAND_RESTORE_TOTP_CODES => match handle_restore(payload, &xns) {
            Ok(payload) => Message {
                cid: channel_id,
                cmd: cmd,
                payload: payload,
            },
            Err(_) => error_message(channel_id, 41),
        },
        COMMAND_BACKUP_TOTP_CODES => match handle_backup(&xns) {
            Ok(payload) => Message {
                cid: channel_id,
                cmd: cmd,
                payload: payload,
            },
            Err(_) => error_message(channel_id, 42),
        },
        _ => error_message(channel_id, 0x33),
    };

    HidPacketIterator::new(payload).unwrap()
}

fn handle_benchmark() -> Vec<u8> {
    vec![0xDE, 0xAD, 0xBE, 0xEF]
}

#[derive(Debug)]
enum BackupError {
    CborError(DecoderError),
    CborConversionError(backup::CborConversionError),
    PddbError(std::io::Error),
    StorageError(crate::storage::Error),
}

impl From<DecoderError> for BackupError {
    fn from(de: DecoderError) -> Self {
        BackupError::CborError(de)
    }
}

impl From<crate::storage::Error> for BackupError {
    fn from(e: crate::storage::Error) -> Self {
        match e {
            Error::IoError(pe) => Self::PddbError(pe),
            generic => Self::StorageError(generic),
        }
    }
}

impl From<backup::CborConversionError> for BackupError {
    fn from(cbe: backup::CborConversionError) -> Self {
        BackupError::CborConversionError(cbe)
    }
}

fn handle_restore(data: Vec<u8>, xns: &xous_names::XousNames) -> Result<Vec<u8>, BackupError> {
    let mut storage = crate::storage::Manager::new(xns);

    let c = cbor::read(&data)?;

    let data = backup::TotpEntries::try_from(c)?;

    for elem in data.0 {
        let totp = TotpRecord {
            version: 1,
            name: elem.name,
            secret: elem.shared_secret,
            algorithm: match elem.algorithm {
                backup::HashAlgorithms::SHA1 => crate::totp::TotpAlgorithm::HmacSha1,
                backup::HashAlgorithms::SHA256 => crate::totp::TotpAlgorithm::HmacSha256,
                backup::HashAlgorithms::SHA512 => crate::totp::TotpAlgorithm::HmacSha512,
            },
            digits: elem.digit_count,
            timestep: elem.step_seconds,
            ctime: 0, // Will be filled in later by storage::new_totp_record();
            notes: t!("vault.notes", xous::LANG).to_string(),
        };

        storage.new_totp_record(totp, None)?
    }

    Ok(vec![0xca, 0xfe, 0xba, 0xbe])
}

fn handle_backup(xns: &xous_names::XousNames) -> Result<Vec<u8>, BackupError> {
    let mut storage = crate::storage::Manager::new(xns);

    let totp_codes = storage.all_totp()?;

    let mut ret = vec![];

    for raw_code in totp_codes {
        ret.push(backup::TotpEntry {
            step_seconds: raw_code.timestep,
            shared_secret: raw_code.secret,
            digit_count: raw_code.digits,
            algorithm: match raw_code.algorithm {
                crate::totp::TotpAlgorithm::HmacSha1 => backup::HashAlgorithms::SHA1,
                crate::totp::TotpAlgorithm::HmacSha256 => backup::HashAlgorithms::SHA256,
                crate::totp::TotpAlgorithm::HmacSha512 => backup::HashAlgorithms::SHA512,
            },
            name: raw_code.name,
        });
    }

    Ok(backup::TotpEntries(ret).bytes())
}

fn error_message(cid: ChannelID, error_code: u8) -> Message {
    // This unwrap is safe because the payload length is 1 <= 7609 bytes.
    Message {
        cid,
        cmd: 0x3F, // COMMAND_ERROR
        payload: vec![error_code],
    }
}
