use std::time::Instant;

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;

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

#[derive(Debug, Subcommand)]
enum Commands {
    /// Backup data from device
    #[clap(arg_required_else_help = true)]
    Backup(SubcommandFields),

    /// Restore data to device
    #[clap(arg_required_else_help = true)]
    Restore(SubcommandFields),
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

fn main() -> Result<()> {
    env_logger::init();
    let wp = CLI::parse();

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

    let start = Instant::now();

    match wp.command {
        Commands::Backup(params) => {
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
                            ctaphid::error::Error::DeviceError(
                                ctaphid::error::DeviceError::Unknown(value),
                            ) => {
                                if value == 88 {
                                    break;
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

            let json = unmarshal_backup_data( bd)?;
            std::fs::write(params.path, json)?;

            log::info!("done! elapsed: {:?}", start.elapsed());
            Ok(())
        }
        Commands::Restore(params) => {
            log::info!("sending data...");
            let hbf = read_human_backup_file(&params.path, params.target)?;
            let chunks: backup::Wires = backup::Wires::from(hbf);

            log::debug!("preparing to send {} chunks", chunks.len());

            for (idx, chunk) in chunks.into_iter().enumerate() {
                log::debug!("sending chunk {}", idx);
                let chunk = &chunk;
                let chunk_bytes: Vec<u8> = chunk.into();
                let vcres =
                    device.vendor_command(ctaphid::command::VendorCommand::H71, &chunk_bytes)?;

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
