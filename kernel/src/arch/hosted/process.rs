pub const MAX_CONTEXT: CtxID = 31;
use crate::services::ProcessInner;
use std::net::TcpStream;
use std::sync::Mutex;
use xous;
use xous::{CtxID, PID};

use lazy_static::lazy_static;

pub type ContextInit = ();

pub struct Process {
    /// Global parameters used by the operating system
    pub inner: ProcessInner,

    /// The network connection to the client process.
    conn: TcpStream,

    /// Current process ID
    pid: PID,
}

impl PartialEq for Process {
    fn eq(&self, other: &Process) -> bool {
        self.pid == other.pid
    }
}

struct ProcessTable {
    current: usize,
    table: Vec<Process>,
}

lazy_static! {
    static ref PROCESS_TABLE: Mutex<ProcessTable> = Mutex::new(ProcessTable {
        current: 0,
        table: Vec::new(),
    });
}

pub fn current_pid() -> PID {
    PROCESS_TABLE.lock().unwrap().current as PID + 1
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
/// Everything required to keep track of a single thread of execution.
/// In a `std` environment, we can't manage threads so this is a no-op.
pub struct ProcessContext {}

/// Everything required to initialize a process on this platform
pub struct ProcessInit {
    /// A network connection to the client
    conn: TcpStream,
}

impl ProcessInit {
    pub fn new(conn: TcpStream) -> ProcessInit {
        ProcessInit { conn }
    }
}

impl Process {
    /// Mark this process as running (on the current core?!)
    pub fn activate(&self) {
        let mut pt = PROCESS_TABLE.lock().unwrap();
        assert!(pt.table[self.pid as usize - 1] == *self);
        pt.current = self.pid as _;
    }

    pub fn current_context(&mut self) -> &mut ProcessContext {
        unimplemented!()
    }

    /// Set the current context number.
    pub fn set_context_nr(&mut self, context: CtxID) {
        if context != 0 {
            panic!("context was {}, not 0", context);
            unimplemented!()
        }
    }

    pub fn context(&mut self, _context_nr: CtxID) -> &mut ProcessContext {
        unimplemented!()
    }

    pub fn find_free_context_nr(&self) -> Option<CtxID> {
        None
    }

    pub fn set_context_result(&mut self, _context_nr: CtxID, _result: xous::Result) {
        unimplemented!()
    }

    /// Initialize this process context with the given entrypoint and stack
    /// addresses.
    pub fn create(pid: PID, init_data: ProcessInit) -> PID {
        let mut process_table = PROCESS_TABLE.lock().unwrap();
        assert!(pid != 0, "PID is zero!");

        let pid_idx = (pid - 1) as usize;

        assert!(pid_idx >= process_table.table.len(), "PID {} already allocated", pid);
        let process = Process {
            inner: Default::default(),
            conn: init_data.conn,
            pid,
        };
        if pid_idx >= process_table.table.len() {
            process_table.table.push(process);
        } else {
            panic!("pid already allocated!");
        }

        pid
    }
}

impl ProcessContext {
}

pub struct ProcessHandle<'a> {
    inner: std::sync::MutexGuard<'a, ProcessTable>,
}

/// Wraps the MemoryManager in a safe mutex.  Because of this, accesses
/// to the Memory Manager should only be made during interrupt contexts.
impl<'a> ProcessHandle<'a> {
    /// Get the singleton Process.
    pub fn get() -> ProcessHandle<'a> {
        ProcessHandle {
            inner:         PROCESS_TABLE.lock().unwrap(),
        }
    }

    pub fn set(pid: PID) {
        PROCESS_TABLE.lock().unwrap().current = pid as usize - 1;
    }
}

use core::ops::{Deref, DerefMut};
impl Deref for ProcessHandle<'_> {
    type Target = Process;
    fn deref(&self) -> &Process {
        &self.inner.table[self.inner.current]
    }
}

impl DerefMut for ProcessHandle<'_> {
    fn deref_mut(&mut self) -> &mut Process {
        let current = self.inner.current;
        &mut self.inner.table[current]
    }
}
