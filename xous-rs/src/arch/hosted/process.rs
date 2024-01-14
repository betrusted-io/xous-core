use core::convert::{TryFrom, TryInto};

use super::CHILD_PROCESS_ADDRESS;
pub use crate::PID;

/// A 16-byte random nonce that identifies this process to the kernel. This
/// is usually provided through the environment variable `XOUS_PROCESS_KEY`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ProcessKey(pub(crate) [u8; 16]);
impl ProcessKey {
    pub fn new(key: [u8; 16]) -> ProcessKey { ProcessKey(key) }
}

impl core::fmt::Display for ProcessKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for i in self.0 {
            write!(f, "{:02x}", i)?;
        }

        Ok(())
    }
}

impl From<&str> for ProcessKey {
    fn from(v: &str) -> ProcessKey {
        let mut key = [0u8; 16];
        for (src, dest) in v.as_bytes().chunks(2).zip(key.iter_mut()) {
            *dest = u8::from_str_radix(core::str::from_utf8(src).unwrap(), 16).unwrap();
        }
        ProcessKey::new(key)
    }
}

/// Describes all parameters that are required to start a new process
/// on this platform.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ProcessInit {
    pub key: ProcessKey,
}

pub struct ProcessArgs {
    command: String,
    name: String,
}

impl ProcessArgs {
    pub fn new(name: &str, command: String) -> ProcessArgs { ProcessArgs { command, name: name.to_owned() } }
}

impl From<&ProcessInit> for [usize; 7] {
    fn from(init: &ProcessInit) -> [usize; 7] {
        [
            u32::from_le_bytes(init.key.0[0..4].try_into().unwrap()) as _,
            u32::from_le_bytes(init.key.0[4..8].try_into().unwrap()) as _,
            u32::from_le_bytes(init.key.0[8..12].try_into().unwrap()) as _,
            u32::from_le_bytes(init.key.0[12..16].try_into().unwrap()) as _,
            0,
            0,
            0,
        ]
    }
}

impl TryFrom<[usize; 7]> for ProcessInit {
    type Error = crate::Error;

    fn try_from(src: [usize; 7]) -> core::result::Result<ProcessInit, crate::Error> {
        let mut exploded = vec![];
        for word in src[0..4].iter() {
            exploded.extend_from_slice(&(*word as u32).to_le_bytes());
        }
        let mut key = [0u8; 16];
        key.copy_from_slice(&exploded);
        Ok(ProcessInit { key: ProcessKey(key) })
    }
}

/// This is returned when a process is created
#[derive(Debug, PartialEq)]
pub struct ProcessStartup {
    /// The process ID of the new process
    pid: crate::PID,
}

impl ProcessStartup {
    pub fn new(pid: crate::PID) -> Self { ProcessStartup { pid } }

    pub fn pid(&self) -> crate::PID { self.pid }
}

impl core::fmt::Display for ProcessStartup {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { write!(f, "{}", self.pid) }
}

impl From<&[usize; 7]> for ProcessStartup {
    fn from(src: &[usize; 7]) -> ProcessStartup {
        ProcessStartup { pid: crate::PID::new(src[0] as _).unwrap() }
    }
}

impl From<[usize; 8]> for ProcessStartup {
    fn from(src: [usize; 8]) -> ProcessStartup {
        let pid = crate::PID::new(src[1] as _).unwrap();
        ProcessStartup { pid }
    }
}

impl From<&ProcessStartup> for [usize; 7] {
    fn from(startup: &ProcessStartup) -> [usize; 7] { [startup.pid.get() as _, 0, 0, 0, 0, 0, 0] }
}

#[derive(Debug)]
pub struct ProcessHandle(std::process::Child);

/// If no connection exists, create a new connection to the server. This means
/// our parent PID will be PID1. Otherwise, reuse the same connection.
pub fn create_process_pre(_args: &ProcessArgs) -> core::result::Result<ProcessInit, crate::Error> {
    unimplemented!()
}

/// Launch a new process with the current PID as the parent.
pub fn create_process_post(
    args: ProcessArgs,
    init: ProcessInit,
    startup: ProcessStartup,
) -> core::result::Result<ProcessHandle, crate::Error> {
    use std::process::Command;
    let server_env = format!("{}", CHILD_PROCESS_ADDRESS.lock().unwrap());
    let pid_env = format!("{}", startup.pid);
    let process_name_env = args.name.to_string();
    let process_key_env: String = format!("{}", init.key);
    let (shell, args) = if cfg!(windows) {
        ("cmd", ["/C", &args.command])
    } else if cfg!(unix) {
        ("sh", ["-c", &args.command])
    } else {
        panic!("unrecognized platform -- don't know how to shell out");
    };

    // println!("Launching process...");
    Command::new(shell)
        .args(args)
        .env("XOUS_SERVER", server_env)
        .env("XOUS_PID", pid_env)
        .env("XOUS_PROCESS_NAME", process_name_env)
        .env("XOUS_PROCESS_KEY", process_key_env)
        .spawn()
        .map(ProcessHandle)
        .map_err(|_| {
            // eprintln!("couldn't start command: {}", e);
            crate::Error::InternalError
        })
}

pub fn wait_process(mut joiner: ProcessHandle) -> crate::SysCallResult {
    joiner
        .0
        .wait()
        .or(Err(crate::Error::InternalError))
        .and_then(|e| if e.success() { Ok(crate::Result::Ok) } else { Err(crate::Error::UnknownError) })
}
