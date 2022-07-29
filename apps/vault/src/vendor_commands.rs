use cbor::reader::DecoderError;
use core::convert::TryFrom;
use locales::t;

use crate::{
    ctap::hid::{send::HidPacketIterator, ChannelID, Message},
    storage::{Error, PasswordRecord, TotpRecord},
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
            cmd,
            payload: handle_benchmark(),
        },
        COMMAND_RESTORE_TOTP_CODES => match handle_restore(payload, &xns) {
            Ok(payload) => Message {
                cid: channel_id,
                cmd,
                payload,
            },
            Err(error) => {
                log::error!("error while restoring codes: {:?}", error);
                error_message(channel_id, 41)
            }
        },
        COMMAND_BACKUP_TOTP_CODES => match handle_backup(&xns, payload) {
            Ok(payload) => Message {
                cid: channel_id,
                cmd,
                payload,
            },
            Err(error) => {
                log::error!("error while restoring codes: {:?}", error);
                error_message(channel_id, 42)
            }
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

    let data = backup::DataPacket::try_from(c)?;

    match data {
        backup::DataPacket::TOTP(totp_entries) => {
            for elem in totp_entries.0 {
                let mut totp = TotpRecord {
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

                storage.new_record(&mut totp, None)?;
            }
        }
        backup::DataPacket::Password(password_entries) => {
            for elem in password_entries.0 {
                let mut password = PasswordRecord {
                    version: 1,
                    description: elem.description,
                    username: elem.username,
                    password: elem.password,
                    notes: elem.notes,
                    count: 0,
                    ctime: 0,
                    atime: 0,
                };

                storage.new_record(&mut password, None)?;
            }
        }
    };

    Ok(vec![0xca, 0xfe, 0xba, 0xbe])
}

fn handle_backup(
    xns: &xous_names::XousNames,
    payload_type: Vec<u8>,
) -> Result<Vec<u8>, BackupError> {
    let storage = crate::storage::Manager::new(xns);

    let payload_type: backup::PayloadType = backup::PayloadType::from(payload_type);

    match payload_type {
        backup::PayloadType::TOTP => {
            let totp_codes: Vec<crate::storage::TotpRecord> =
                storage.all(crate::storage::ContentKind::TOTP)?;

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
                        _ => panic!("invalid algorithm"),
                    },
                    name: raw_code.name,
                });
            }

            Ok((&backup::TotpEntries(ret)).into())
        }
        backup::PayloadType::Password => {
            let passwords: Vec<crate::storage::PasswordRecord> =
                storage.all(crate::storage::ContentKind::Password)?;

            let mut ret = vec![];

            for raw_pass in passwords {
                ret.push(backup::PasswordEntry {
                    description: raw_pass.description,
                    username: raw_pass.username,
                    password: raw_pass.password,
                    notes: raw_pass.notes,
                });
            }

            Ok((&backup::PasswordEntries(ret)).into())
        }
    }
}

fn error_message(cid: ChannelID, error_code: u8) -> Message {
    // This unwrap is safe because the payload length is 1 <= 7609 bytes.
    Message {
        cid,
        cmd: 0x3F, // COMMAND_ERROR
        payload: vec![error_code],
    }
}
