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

const OKAY_CANARY: &[u8] = &[0xca, 0xfe, 0xba, 0xbe];
const PRECURSOR_VENDOR_ID: u16 = 0x1209;
const PRECURSOR_PRODUCT_ID: u16 = 0x3613;

fn main() -> Result<()> {
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

    let device = ctaphid::Device::connect(&ha, precursor.unwrap())?;

    match wp.command {
        Commands::Backup(params) => {
            let pt: backup::PayloadType = (&params.target).into();
            let ops: u8 = (&pt).into();

            let bd = device.vendor_command(ctaphid::command::VendorCommand::H42, &vec![ops])?;

            let json = unmarshal_backup_data(bd, params.target)?;

            std::fs::write(params.path, json)?;

            Ok(())
        }
        Commands::Restore(params) => {
            let hbf = read_human_backup_file(&params.path, params.target)?;
            let vcres = device.vendor_command(ctaphid::command::VendorCommand::H41, &hbf)?;

            if vcres.ne(OKAY_CANARY) {
                return Err(ProgramError::DeviceError(vcres))?;
            }

            Ok(())
        }
    }
}

fn read_human_backup_file(path: &str, target: Target) -> Result<Vec<u8>> {
    let f = std::fs::File::open(path)?;

    match target {
        Target::TOTP => {
            let backup_json: backup::TotpEntries = serde_json::from_reader(f)?;

            let backup_object = backup::DataPacket::TOTP(backup_json);

            Ok(backup_object.into())
        }
        Target::Password => {
            let backup_json: backup::PasswordEntries = serde_json::from_reader(f)?;

            let backup_object = backup::DataPacket::Password(backup_json);

            Ok(backup_object.into())
        }
    }
}

fn unmarshal_backup_data(data: Vec<u8>, target: Target) -> Result<Vec<u8>> {
    let raw_cbor = cbor::read(&data).unwrap();

    match target {
        Target::TOTP => {
            let data: backup::TotpEntries = raw_cbor.try_into()?;

            Ok(serde_json::ser::to_vec(&data).unwrap())
        }
        Target::Password => {
            let data: backup::PasswordEntries = raw_cbor.try_into()?;

            Ok(serde_json::ser::to_vec(&data).unwrap())
        }
    }
}
