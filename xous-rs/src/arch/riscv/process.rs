use core::convert::TryFrom;

/// ProcessArgs are the arguments that are created by the user. These
/// will be turned into `ProcessInit` by this library prior to sending
/// them into the kernel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessArgs {
    name: [u8; 16],
}

/// ProcessInit describes the values that are passed to the
/// kernel. This value will only be used internally inside
/// `xous-rs`, as well as inside the kernel itself.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ProcessInit {
    // 0,1 -- Stack Base, Stack Size
    stack: crate::MemoryRange,
    // 2,3 -- Text Start, Text Size
    text: crate::MemoryRange,
    // 4
    start: crate::MemoryAddress,
    // 5
    // 6
}

impl Into<[usize; 7]> for &ProcessInit {
    fn into(self) -> [usize; 7] {
        [
            self.stack.addr.get(),
            self.stack.size.get(),
            self.text.addr.get(),
            self.text.size.get(),
            self.start.get(),
            0,
            0,
        ]
    }
}

impl TryFrom<[usize; 7]> for ProcessInit {
    type Error = crate::Error;
    fn try_from(src: [usize; 7]) -> Result<ProcessInit, Self::Error> {
        Ok(ProcessInit {
            stack: unsafe {
                crate::MemoryRange::new(src[0], src[1]).or(Err(crate::Error::OutOfMemory))?
            },
            text: unsafe {
                crate::MemoryRange::new(src[2], src[3]).or(Err(crate::Error::OutOfMemory))?
            },
            start: crate::MemoryAddress::new(src[4]).ok_or(crate::Error::OutOfMemory)?,
        })
    }
}

/// When a new process is created, this platform-specific structure is returned.
#[derive(Debug, PartialEq)]
pub struct ProcessStartup {
    /// The process ID of the new process
    pid: crate::PID,

    /// A server that the parent process can connect to in order to send further
    /// initialization messages
    server: crate::SID,
}

impl From<&[usize; 7]> for ProcessStartup {
    fn from(src: &[usize; 7]) -> ProcessStartup {
        let server = crate::SID::from_array([src[1] as _, src[2] as _, src[3] as _, src[4] as _]);
        ProcessStartup {
            pid: crate::PID::new(src[0] as _).unwrap(),
            server,
        }
    }
}

impl From<[usize; 8]> for ProcessStartup {
    fn from(src: [usize; 8]) -> ProcessStartup {
        let pid = crate::PID::new(src[1] as _).unwrap();
        let server = crate::SID::from_array([src[2] as _, src[3] as _, src[4] as _, src[5] as _]);
        ProcessStartup { pid, server }
    }
}

impl Into<[usize; 7]> for &ProcessStartup {
    fn into(self) -> [usize; 7] {
        let server = self.server.to_array();
        [
            self.pid.get() as _,
            server[0] as _,
            server[1] as _,
            server[2] as _,
            server[3] as _,
            0,
            0,
        ]
    }
}

pub struct ProcessHandle(crate::PID);

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ProcessKey([u8; 8]);
impl ProcessKey {
    pub fn new(key: [u8; 8]) -> ProcessKey {
        ProcessKey(key)
    }
}

/// Convert the `ProcessArgs` structure passed by the user into a `ProcessInit`
/// structure suitable for consumption by the kernel.
pub fn create_process_pre(_args: &ProcessArgs) -> core::result::Result<ProcessInit, crate::Error> {
    todo!()
}

/// Any post-processing required to set up this process.
pub fn create_process_post(
    _args: ProcessArgs,
    _init: ProcessInit,
    startup: ProcessStartup,
) -> core::result::Result<ProcessHandle, crate::Error> {
    Ok(ProcessHandle(startup.pid))
}

/// Wait for a process to terminate.
pub fn wait_process(_joiner: ProcessHandle) -> crate::SysCallResult {
    loop {
        crate::wait_event();
    }
}
