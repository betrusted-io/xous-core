mod authenticator;
mod bitwarden;
mod csvpass;
use std::io::BufRead;
use std::time::Instant;

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;

include!(concat!(env!("OUT_DIR"), "/protos/mod.rs"));

#[derive(Debug, Parser)]
#[clap(name = "vaultbackup")]
#[clap(about = "A backup/restore tool for the Precursor vault app.", long_about = None)]
struct CLI {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, PartialEq, clap::ValueEnum, Clone)]
enum Target {
    TOTP,
    Password,
}

impl From<&Target> for backup::PayloadType {
    fn from(t: &Target) -> backup::PayloadType {
        match t {
            Target::TOTP => backup::PayloadType::TOTP,
            Target::Password => backup::PayloadType::Password,
        }
    }
}

#[derive(Debug, PartialEq, clap::Args, Clone)]
struct SubcommandFields {
    /// The target for which to run the action
    #[clap(required = true, value_enum)]
    target: Target,

    /// The path to read backup data from
    #[clap(required = true, value_parser)]
    path: String,
}

#[derive(Debug, PartialEq, clap::ValueEnum, Clone)]
enum FormatTargets {
    Bitwarden,
    Authenticator,
    CsvPass,
}

#[derive(Debug, PartialEq, clap::Args, Clone)]
struct FormatFields {
    /// The target for which to run the action
    #[clap(required = true, value_enum)]
    target: FormatTargets,

    /// The path to read input data from
    #[clap(required = true, value_parser)]
    path: String,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Backup data from device
    #[clap(arg_required_else_help = true)]
    Backup(SubcommandFields),

    /// Restore data to device
    #[clap(arg_required_else_help = true)]
    Restore(SubcommandFields),

    /// Format a known password manager export for Vault.
    #[clap(arg_required_else_help = true)]
    Format(FormatFields),
}

#[derive(Debug)]
enum ProgramError {
    NoDevicesFound,
    DeviceError(Vec<u8>),
}

impl std::fmt::Display for ProgramError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProgramError::NoDevicesFound => write!(f, "no CTAP2 devices found"),
            ProgramError::DeviceError(code) => write!(f, "device returned code {:?}", code),
        }
    }
}

impl std::error::Error for ProgramError {}

const PRECURSOR_VENDOR_ID: u16 = 0x1209;
const PRECURSOR_PRODUCT_ID: u16 = 0x3613;

fn open_precursor() -> Result<ctaphid::Device> {
    let ha = hidapi::HidApi::new()?;
    let dl = ha.device_list();

    let mut precursor: Option<&hidapi::DeviceInfo> = None;
    for i in dl {
        if i.product_id() == PRECURSOR_PRODUCT_ID && i.vendor_id() == PRECURSOR_VENDOR_ID {
            precursor = Some(i);
            break;
        }
    }

    if precursor.is_none() {
        return Err(ProgramError::NoDevicesFound)?;
    }

    log::info!("connecting to device...");
    let device = ctaphid::Device::connect(&ha, precursor.unwrap())?;
    log::info!("connected!");

    device.vendor_command(ctaphid::command::VendorCommand::H74, &vec![])?;
    log::debug!("sent session reset command");

    Ok(device)
}

fn main() -> Result<()> {
    env_logger::init();
    let wp = CLI::parse();

    let start = Instant::now();

    match wp.command {
        Commands::Format(params) => {
            let f = std::fs::File::open(params.path)?;

            match params.target {
                FormatTargets::CsvPass => {
                    println!("Formatting CSV file to uploadable JSON -> csv_to_vault_passwords.json");
                    let items = csvpass::Items::try_from(f)?;
                    let items = items.logins();

                    let mut passwords = backup::PasswordEntries::default();

                    for (idx, item) in items.into_iter().enumerate() {
                        let mut pw = backup::PasswordEntry::default();

                        if item.username.is_none() || item.site.is_none() {
                            log::error!(
                                "(non-fatal) entry {} is missing username and/or site. Ignoring entry.",
                                idx
                            );
                            continue;
                        }
                        pw.password = item.password.as_ref().unwrap_or(&String::new()).clone();
                        pw.username = item.username.as_ref().unwrap().clone();
                        pw.description = item.site.as_ref().unwrap().clone();
                        pw.notes = item.notes.as_ref().unwrap_or(&String::new()).clone();

                        passwords.0.push(pw);
                    }

                    std::fs::write("csv_to_vault_passwords.json", serde_json::ser::to_vec(&passwords)?)?;

                    Ok(())
                }
                FormatTargets::Bitwarden => {
                    let items = bitwarden::Items::try_from(f)?;
                    let items = items.logins();

                    let mut passwords = backup::PasswordEntries::default();
                    let mut totps = backup::TotpEntries::default();

                    for (idx, item) in items.into_iter().enumerate() {
                        let mut pw = backup::PasswordEntry::default();
                        let login = item.login.as_ref().unwrap();

                        match login.sane() {
                            Ok(()) => (),
                            Err(err) => {
                                log::error!("entry {} is invalid: {}", idx, err);
                                continue;
                            }
                        }

                        pw.password = login.password.as_ref().unwrap().clone();
                        pw.username = login.username.as_ref().unwrap().clone();
                        pw.description = item.name.clone();
                        pw.notes = item.notes.as_ref().unwrap_or(&String::new()).clone();

                        passwords.0.push(pw);

                        if login.totp.is_some() {
                            let totp = login.totp.as_ref().unwrap();

                            let mut t = backup::TotpEntry::default();
                            t.name = item.name.clone();
                            t.algorithm = backup::HashAlgorithms::SHA256;
                            t.digit_count = 6;
                            t.step_seconds = 30;
                            t.shared_secret =
                                totp.strip_prefix("otpauth://totp/").unwrap_or(&totp).to_string();

                            totps.0.push(t);
                        }
                    }

                    std::fs::write(
                        "bitwarden_to_vault_passwords.json",
                        serde_json::ser::to_vec(&passwords)?,
                    )?;

                    if totps.0.len() > 0 {
                        std::fs::write("bitwarden_to_vault_totps.json", serde_json::ser::to_vec(&totps)?)?;
                    }

                    Ok(())
                }
                FormatTargets::Authenticator => {
                    let mut totps = backup::TotpEntries::default();

                    for uri in std::io::BufReader::new(f).lines() {
                        let uri = url::Url::parse(&uri?)?;
                        match (uri.scheme(), uri.host_str()) {
                            ("otpauth", Some("totp")) => totps.0.push(authenticator::otpauth_to_entry(&uri)?),
                            ("otpauth-migration", Some("offline")) => {
                                for t in authenticator::otpauth_migration_to_entries(&uri)? {
                                    totps.0.push(t);
                                }
                            }
                            _ => {
                                log::error!("unsupported URI: {}", uri)
                            }
                        }
                    }

                    std::fs::write("authenticator_to_vault_totps.json", serde_json::ser::to_vec(&totps)?)?;

                    Ok(())
                }
            }
        }
        Commands::Backup(params) => {
            let device = open_precursor()?;
            log::info!("receiving data...");
            let bd = {
                let mut data = vec![];
                let mut idx = 0;
                loop {
                    let pt: backup::PayloadType = (&params.target).into();
                    let ops: u8 = (&pt).into();

                    let wire_data = match device
                        .vendor_command(ctaphid::command::VendorCommand::H72, &vec![ops])
                    {
                        Ok(we) => we,

                        Err(error) => match error {
                            ctaphid::error::Error::DeviceError(ctaphid::error::DeviceError::Unknown(
                                value,
                            )) => {
                                if value == 88 {
                                    break;
                                }
                                if value == 44 {
                                    return Err(std::io::Error::new(
                                        std::io::ErrorKind::PermissionDenied,
                                        "Host readout is not enabled, unable to proceed!\nPlease select 'Enable host readout' from the vault context menu first.",
                                    ))?;
                                }

                                return Err(error)?;
                            }
                            _ => return Err(error)?,
                        },
                    };

                    let raw_cbor = cbor::read(&wire_data).unwrap();
                    let mut wire_data: backup::Wire = backup::Wire::try_from(raw_cbor)?;

                    data.append(&mut wire_data.data);
                    log::debug!("received chunk {}", idx);
                    idx += 1;

                    if !wire_data.more_data {
                        break;
                    }
                }

                data
            };

            let json = unmarshal_backup_data(bd)?;
            std::fs::write(params.path, json)?;

            log::info!("done! elapsed: {:?}", start.elapsed());
            Ok(())
        }
        Commands::Restore(params) => {
            let device = open_precursor()?;
            log::info!("sending data...");
            let hbf = read_human_backup_file(&params.path, params.target)?;
            let chunks: backup::Wires = backup::Wires::from(hbf);

            log::debug!("preparing to send {} chunks", chunks.len());

            for (idx, chunk) in chunks.into_iter().enumerate() {
                log::debug!("sending chunk {}", idx);
                let chunk = &chunk;
                let chunk_bytes: Vec<u8> = chunk.into();
                let vcres = match device.vendor_command(ctaphid::command::VendorCommand::H71, &chunk_bytes) {
                    Ok(vcres) => vcres,
                    Err(error) => match error {
                        ctaphid::error::Error::DeviceError(ctaphid::error::DeviceError::Unknown(value)) => {
                            if value == 44 {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::PermissionDenied,
                                    "Host readout is not enabled, unable to proceed!\nPlease select 'Enable host readout' from the vault context menu first.",
                                ))?;
                            }

                            return Err(error)?;
                        }
                        _ => return Err(error)?,
                    },
                };

                if vcres.eq(backup::CONTINUE_RESPONSE) {
                    log::debug!("received CONTINUE response");
                    continue;
                }

                if vcres.ne(backup::OKAY_CANARY) {
                    log::debug!("finished!");
                    return Err(ProgramError::DeviceError(vcres))?;
                }
            }

            log::info!("done! elapsed: {:?}", start.elapsed());
            Ok(())
        }
    }
}

fn read_human_backup_file(path: &str, target: Target) -> Result<backup::DataPacket> {
    let f = std::fs::File::open(path)?;

    match target {
        Target::TOTP => {
            let backup_json: backup::TotpEntries = serde_json::from_reader(f)?;

            Ok(backup::DataPacket::TOTP(backup_json))
        }
        Target::Password => {
            let backup_json: backup::PasswordEntries = serde_json::from_reader(f)?;

            Ok(backup::DataPacket::Password(backup_json))
        }
    }
}

fn unmarshal_backup_data(data: Vec<u8>) -> Result<Vec<u8>> {
    let raw_cbor = cbor::read(&data).unwrap();

    let dp = backup::DataPacket::try_from(raw_cbor).unwrap();

    match dp {
        backup::DataPacket::Password(pw) => Ok(serde_json::ser::to_vec(&pw).unwrap()),
        backup::DataPacket::TOTP(t) => Ok(serde_json::ser::to_vec(&t).unwrap()),
    }
}
