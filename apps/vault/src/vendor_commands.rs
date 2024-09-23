use core::convert::TryFrom;

use cbor::reader::DecoderError;
use locales::t;
use vault::ctap::hid::{ChannelID, CtapHidCommand, Message, send::HidPacketIterator};
use vault::vault_api::{COMMAND_BACKUP_TOTP_CODES, COMMAND_RESET_SESSION, COMMAND_RESTORE_TOTP_CODES};

use crate::storage::{Error, PasswordRecord, TotpRecord};
use crate::totp::TotpAlgorithm;
// TODO(gsora): add something that checks whether or not a command works.

pub enum SessionError {
    WrongIndex,
    WrongCommand,
}

impl SessionError {
    pub fn ctaphid_error(&self, channel_id: ChannelID) -> HidPacketIterator {
        HidPacketIterator::new(error_message(channel_id, backup::VENDOR_SESSION_ERROR)).unwrap()
    }
}

#[derive(Default)]
pub struct VendorSession {
    data: Vec<u8>,
    finished: bool,
    command: u8,
    channel_id: ChannelID,
    index: u64,

    // TODO: write backup kind here
    backup_kind: Option<backup::PayloadType>,
    is_backup: bool,
    backup_chunks: Option<backup::Wires>,
}

impl VendorSession {
    fn reset(&mut self) { *self = VendorSession::default() }

    fn read_from_wire(&mut self, w: backup::Wire, command: u8) -> Result<(), SessionError> {
        self.data.append(&mut w.data.clone());
        self.finished = !w.more_data;

        if self.command != 0 && self.command != command {
            return Err(SessionError::WrongCommand);
        }

        self.command = command;

        if self.index != 0 && self.index <= w.index {
            return Err(SessionError::WrongIndex);
        }

        Ok(())
    }

    fn load_backup_data(&mut self, data: backup::DataPacket) {
        let mut data = backup::Wires::from(data);
        data.reverse();
        self.backup_chunks = Some(data)
    }

    fn drain_backup(&mut self) -> Option<backup::Wire> {
        if self.backup_chunks.is_none() {
            return None;
        }

        let b = self.backup_chunks.as_mut().unwrap();

        b.pop()
    }

    pub fn is_backup(&self) -> bool { self.is_backup }

    pub fn has_backup_data(&self) -> bool { self.backup_chunks.is_some() }

    fn finished(&self) -> bool { self.finished }
}

pub fn handle_vendor_data(
    cmd: u8,
    channel_id: ChannelID,
    payload: Vec<u8>,
    current_state: &mut VendorSession,
) -> Result<Option<HidPacketIterator>, SessionError> {
    if cmd == COMMAND_RESET_SESSION {
        log::debug!("resetting session");
        current_state.reset();
        return Ok(Some(
            HidPacketIterator::new(Message {
                cid: channel_id,
                cmd: cmd.into(),
                payload: vec![0xca, 0xfe, 0xba, 0xbe],
            })
            .unwrap(),
        ));
    }
    current_state.command = cmd;
    current_state.channel_id = channel_id;

    // if we received a backup::PayloadType, we are being requested a backup.
    let backup_type = match backup::PayloadType::try_from(&payload) {
        Ok(t) => Some(t),
        Err(_) => None,
    };

    if backup_type.is_some() {
        log::debug!("vendor data is of backup kind");
        current_state.is_backup = true;
        current_state.backup_kind = backup_type;
        return Ok(None); // Handle Backup
    }

    log::debug!("vendor data is of restore kind, reading payload as cbor");
    let c = match cbor::read(&payload) {
        Ok(w) => w,
        Err(error) => {
            log::error!("error while handling vendor data, {:?}", error);
            return Ok(Some(
                HidPacketIterator::new(error_message(channel_id, backup::ERROR_VENDOR_HANDLING)).unwrap(),
            ));
        }
    };

    log::debug!("reading cbor as wire");
    let wire_data = match backup::Wire::try_from(c) {
        Ok(w) => w,
        Err(error) => {
            log::error!("error while handling vendor data, {:?}", error);
            return Ok(Some(
                HidPacketIterator::new(error_message(channel_id, backup::ERROR_VENDOR_HANDLING)).unwrap(),
            ));
        }
    };

    current_state.read_from_wire(wire_data, cmd)?;

    log::debug!("state.finished(): {:?}", current_state.finished());
    match current_state.finished() {
        true => Ok(None),
        false => Ok(Some(
            HidPacketIterator::new(Message {
                cid: channel_id,
                cmd: cmd.into(),
                payload: backup::CONTINUE_RESPONSE.to_vec(),
            })
            .unwrap(),
        )),
    }
}

pub fn handle_vendor_command(session: &mut VendorSession, allow_host: bool) -> HidPacketIterator {
    let cmd = session.command;
    let payload = session.data.clone();
    let channel_id = session.channel_id;

    log::debug!("got vendor command: {}", cmd);
    let xns = xous_names::XousNames::new().unwrap();

    let payload = if allow_host {
        match session.command {
            COMMAND_RESTORE_TOTP_CODES => match handle_restore(payload, &xns) {
                Ok(payload) => Message { cid: channel_id, cmd: cmd.into(), payload },
                Err(error) => {
                    log::error!("error while restoring codes: {:?}", error);
                    error_message(channel_id, 41)
                }
            },
            COMMAND_BACKUP_TOTP_CODES => match handle_backup(&xns, session) {
                Ok(payload) => {
                    log::debug!("sending over chunk: {:?}", payload);
                    Message { cid: channel_id, cmd: cmd.into(), payload }
                }
                Err(error) => {
                    match error {
                        BackupError::NoMoreChunks => {
                            log::debug!("no more chunks to send via backup!");
                            error_message(channel_id, 88) // TODO(gsora): make this a constant
                        }
                        _ => {
                            log::error!("error while restoring codes: {:?}", error);
                            error_message(channel_id, 42)
                        }
                    }
                }
            },
            _ => error_message(channel_id, 0x33),
        }
    } else {
        error_message(channel_id, 44)
    };

    HidPacketIterator::new(payload).unwrap()
}

#[derive(Debug)]
#[allow(dead_code)]
enum BackupError {
    CborError(DecoderError),
    CborConversionError(backup::CborConversionError),
    PddbError(std::io::Error),
    StorageError(crate::storage::Error),
    NoMoreChunks,
}

impl From<DecoderError> for BackupError {
    fn from(de: DecoderError) -> Self { BackupError::CborError(de) }
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
    fn from(cbe: backup::CborConversionError) -> Self { BackupError::CborConversionError(cbe) }
}

fn handle_restore(data: Vec<u8>, xns: &xous_names::XousNames) -> Result<Vec<u8>, BackupError> {
    log::debug!("handling restore");
    let mut storage = crate::storage::Manager::new(xns);

    let c = cbor::read(&data)?;

    let data = backup::DataPacket::try_from(c)?;

    let mut entries: Vec<Box<dyn crate::storage::StorageContent>> = vec![];

    match data {
        backup::DataPacket::TOTP(totp_entries) => {
            log::debug!("restoring totp");
            for (idx, elem) in totp_entries.0.into_iter().enumerate() {
                log::debug!("restoring element {}", idx);
                let totp = TotpRecord {
                    version: 1,
                    name: elem.name,
                    secret: elem.shared_secret,
                    algorithm: match elem.algorithm {
                        backup::HashAlgorithms::SHA1 => TotpAlgorithm::HmacSha1,
                        backup::HashAlgorithms::SHA256 => TotpAlgorithm::HmacSha256,
                        backup::HashAlgorithms::SHA512 => TotpAlgorithm::HmacSha512,
                    },
                    digits: elem.digit_count,
                    timestep: elem.step_seconds,
                    ctime: 0, // Will be filled in later by storage::new_totp_record();
                    notes: t!("vault.notes", locales::LANG).to_string(),
                    is_hotp: false,
                };
                entries.push(Box::new(totp));
            }
        }
        backup::DataPacket::Password(password_entries) => {
            log::debug!("restoring password");
            for (idx, elem) in password_entries.0.into_iter().enumerate() {
                log::debug!("restoring element {}", idx);
                let password = PasswordRecord {
                    version: 1,
                    description: elem.description,
                    username: elem.username,
                    password: elem.password,
                    notes: elem.notes,
                    count: 0,
                    ctime: 0,
                    atime: 0,
                };

                entries.push(Box::new(password));
            }
        }
    };

    match storage.new_records(entries, None, false) {
        Ok(()) => Ok(vec![0xca, 0xfe, 0xba, 0xbe]),
        Err(error) => match error {
            crate::storage::Error::DupesExist(dupes) => {
                // this is a non-fatal error, let's just print the dupes and continue
                log::info!("dupes detected while restoring! {:?}", dupes);
                Ok(vec![0xca, 0xfe, 0xba, 0xbe])
            }
            _ => Err(error)?,
        },
    }
}

fn handle_backup(xns: &xous_names::XousNames, session: &mut VendorSession) -> Result<Vec<u8>, BackupError> {
    log::debug!(
        "entering backup handler, is_backup: {} has_backup_data: {}",
        session.is_backup(),
        session.has_backup_data()
    );

    if session.is_backup() && session.has_backup_data() {
        log::debug!("there was some backup data, getting last chunk and sending it over");
        let new_chunk = session.drain_backup();
        if new_chunk.is_none() {
            log::debug!("finished chunks!");
            return Err(BackupError::NoMoreChunks);
        }

        let new_chunk = new_chunk.as_ref().unwrap();

        log::debug!("new chunk data: idx: {}, more_data: {}", new_chunk.index, new_chunk.more_data);
        return Ok(new_chunk.into());
    }

    let storage = crate::storage::Manager::new(xns);

    log::debug!("no backup data found, creating");
    let data = match session.backup_kind.as_ref().unwrap() {
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
                        TotpAlgorithm::HmacSha1 => backup::HashAlgorithms::SHA1,
                        TotpAlgorithm::HmacSha256 => backup::HashAlgorithms::SHA256,
                        TotpAlgorithm::HmacSha512 => backup::HashAlgorithms::SHA512,
                        _ => panic!("invalid algorithm"),
                    },
                    name: raw_code.name,
                    hotp: raw_code.is_hotp,
                });
            }

            backup::DataPacket::TOTP(backup::TotpEntries(ret))
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

            backup::DataPacket::Password(backup::PasswordEntries(ret))
        }
    };

    log::debug!("loading newly created backup data in session");
    session.load_backup_data(data);

    log::debug!("amount of chunks: {}", session.backup_chunks.as_ref().unwrap().len());

    let new_chunk = session.drain_backup();

    if new_chunk.is_none() {
        log::debug!("no more chunks after first load!");
        return Err(BackupError::NoMoreChunks);
    }

    let new_chunk = new_chunk.as_ref().unwrap();

    log::debug!("new chunk data: idx: {}, more_data: {}", new_chunk.index, new_chunk.more_data);

    return Ok(new_chunk.into());
}

pub(crate) fn error_message(cid: ChannelID, error_code: u8) -> Message {
    // This unwrap is safe because the payload length is 1 <= 7609 bytes.
    Message {
        cid,
        cmd: CtapHidCommand::Error, // COMMAND_ERROR
        payload: vec![error_code],
    }
}
