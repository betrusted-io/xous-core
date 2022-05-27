use core::convert::TryFrom;

/// ProcessArgs are the arguments that are created by the user. These
/// will be turned into `ProcessInit` by this library prior to sending
/// them into the kernel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessArgs<'a> {
    stub: &'a [u8],
}

impl<'a> ProcessArgs<'a> {
    pub fn new(stub: &[u8]) -> ProcessArgs {
        ProcessArgs { stub }
    }
}

/// ProcessInit describes the values that are passed to the
/// kernel. This value will only be used internally inside
/// `xous-rs`, as well as inside the kernel itself.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ProcessInit {
    // 0,1 -- Stack Base, Stack Size
    pub stack: crate::MemoryRange,
    // 2,3 -- Text Start, Text Size
    pub text: crate::MemoryRange,
    // 4 -- Text destination address
    pub text_destination: crate::MemoryAddress,
    // 5 -- Entrypoint
    pub start: crate::MemoryAddress,
    // 6
}

impl Into<[usize; 7]> for &ProcessInit {
    fn into(self) -> [usize; 7] {
        [
            self.stack.addr.get(),
            self.stack.size.get(),
            self.text.addr.get(),
            self.text.size.get(),
            self.text_destination.get(),
            self.start.get(),
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
            text_destination: crate::MemoryAddress::new(src[4]).ok_or(crate::Error::OutOfMemory)?,
            start: crate::MemoryAddress::new(src[5]).ok_or(crate::Error::OutOfMemory)?,
        })
    }
}

/// When a new process is created, this platform-specific structure is returned.
#[derive(Debug, PartialEq)]
pub struct ProcessStartup {
    /// The process ID of the new process
    pid: crate::PID,

    /// A connection to the initial server that is running in the new process.
    connection: crate::CID,
}

impl ProcessStartup {
    pub fn new(pid: crate::PID, connection: crate::CID) -> Self {
        ProcessStartup { pid, connection }
    }
}

impl From<&[usize; 7]> for ProcessStartup {
    fn from(src: &[usize; 7]) -> ProcessStartup {
        ProcessStartup {
            pid: crate::PID::new(src[0] as _).unwrap(),
            connection: src[1] as _,
        }
    }
}

impl From<[usize; 8]> for ProcessStartup {
    fn from(src: [usize; 8]) -> ProcessStartup {
        let pid = match crate::PID::new(src[1] as _) {
            Some(o) => o,
            None => unsafe { crate::PID::new_unchecked(255) },
        };
        let cid = src[2] as _;
        ProcessStartup {
            pid,
            connection: cid,
        }
    }
}

impl Into<[usize; 7]> for &ProcessStartup {
    fn into(self) -> [usize; 7] {
        [self.pid.get() as _, self.connection as _, 0, 0, 0, 0, 0]
    }
}

pub struct ProcessHandle {
    pub pid: crate::PID,
    pub cid: crate::CID,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ProcessKey([u8; 8]);
impl ProcessKey {
    pub fn new(key: [u8; 8]) -> ProcessKey {
        ProcessKey(key)
    }
}

/// Convert the `ProcessArgs` structure passed by the user into a `ProcessInit`
/// structure suitable for consumption by the kernel.
pub fn create_process_pre(args: &ProcessArgs) -> core::result::Result<ProcessInit, crate::Error> {
    let spawn_stub_rounded = (args.stub.len() + 4096 - 1) & !(4096 - 1);
    let mut spawn_memory = crate::map_memory(
        None,
        None,
        spawn_stub_rounded,
        crate::MemoryFlags::R | crate::MemoryFlags::W | crate::MemoryFlags::X,
    )?;
    for (dest, src) in spawn_memory.as_slice_mut().iter_mut().zip(args.stub) {
        *dest = *src;
    }
    Ok(ProcessInit {
        // 0,1 -- Stack Base, Stack Size
        stack: unsafe { crate::MemoryRange::new(0x7000_0000, 131072)? },
        // 2,3 -- Text Start, Text Size
        text: spawn_memory,
        // 4 -- Text destination address
        text_destination: crate::MemoryAddress::new(0x2050_1000).unwrap(),
        // 5 -- Entrypoint
        start: crate::MemoryAddress::new(0x2050_1000).unwrap(),
    })
}

/// Any post-processing required to set up this process.
pub fn create_process_post(
    _args: ProcessArgs,
    _init: ProcessInit,
    startup: ProcessStartup,
) -> core::result::Result<ProcessHandle, crate::Error> {
    Ok(ProcessHandle {
        pid: startup.pid,
        cid: startup.connection,
    })
}

/// Wait for a process to terminate.
pub fn wait_process(_joiner: ProcessHandle) -> crate::SysCallResult {
    loop {
        crate::wait_event();
    }
}
