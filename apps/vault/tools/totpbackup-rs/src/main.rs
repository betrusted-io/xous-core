use argh::FromArgs;
use std::error::Error;

#[derive(FromArgs,  PartialEq, Debug)]
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
const PRECURSOR_VENDOR_ID: u16 = 0x1209;
const PRECURSOR_PRODUCT_ID: u16 = 0x3613;

fn main() -> Result<(), Box<dyn Error>> {
    let wp: WithPositional = argh::from_env();
    let argument: CLIAction = wp.try_into()?;

    let ha = hidapi::HidApi::new()?;
    let dl = ha.device_list();

    let mut precursor: Option<&hidapi::DeviceInfo> = None;
    for i in dl {
        println!("{:x?}", i);
        if i.product_id() == PRECURSOR_PRODUCT_ID && i.vendor_id() == PRECURSOR_VENDOR_ID {
            precursor = Some(i);
            break;
        }
    }

    if precursor.is_none() {
        return Err(Box::new(ProgramError::NoDevicesFound));
    }

    let device = ctaphid::Device::connect(&ha, precursor.unwrap())?;

    match argument {
        CLIAction::Backup(_) => todo!(),
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

    let backup_json: backup::TotpEntries = serde_json::from_reader(f)?;

    Ok(backup_json.bytes())
}