pub const MAX_CONTEXT: CtxID = 31;
use crate::services::ProcessInner;
use std::io::Write;
use std::net::TcpStream;
use std::sync::Mutex;
use xous::{CtxID, PID};

use lazy_static::lazy_static;

pub type ContextInit = ();
pub const INITIAL_CONTEXT: usize = 1;

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
pub struct Context {}

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
    pub fn activate(&mut self) -> Result<(), xous::Error> {
        // let mut pt = PROCESS_TABLE.lock().unwrap();
        // assert!(pt.table[self.pid as usize - 1] == *self);
        // pt.current = self.pid as _;
        Ok(())
    }

    pub fn setup_context(
        &mut self,
        context: CtxID,
        _setup: ContextInit,
    ) -> Result<(), xous::Error> {
        if context != INITIAL_CONTEXT {
            return Err(xous::Error::ProcessNotFound);
        }
        Ok(())
    }

    pub fn current_context(&mut self) -> CtxID {
        INITIAL_CONTEXT
    }

    /// Set the current context number.
    pub fn set_context(&mut self, context: CtxID) -> Result<(), xous::Error> {
        if context != INITIAL_CONTEXT {
            panic!("context was {}, not 1", context);
        }
        Ok(())
    }

    pub fn find_free_context_nr(&self) -> Option<CtxID> {
        None
    }

    pub fn set_context_result(&mut self, context: CtxID, result: xous::Result) {
        assert!(context == INITIAL_CONTEXT);
        for word in result.to_args().iter_mut() {
            self.conn
                .write_all(&word.to_le_bytes())
                .expect("Disconnection");
        }
    }

    /// Initialize this process context with the given entrypoint and stack
    /// addresses.
    pub fn create(pid: PID, init_data: ProcessInit) -> PID {
        let mut process_table = PROCESS_TABLE.lock().unwrap();
        assert!(pid != 0, "PID is zero!");

        let pid_idx = (pid - 1) as usize;

        assert!(
            pid_idx >= process_table.table.len(),
            "PID {} already allocated",
            pid
        );
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

    pub fn send(&mut self, bytes: &[u8]) -> Result<(), xous::Error> {
        self.conn.write_all(bytes).unwrap();
        Ok(())
    }
}

impl Context {}

pub struct ProcessHandle<'a> {
    inner: std::sync::MutexGuard<'a, ProcessTable>,
}

/// Wraps the MemoryManager in a safe mutex.  Because of this, accesses
/// to the Memory Manager should only be made during interrupt contexts.
impl<'a> ProcessHandle<'a> {
    /// Get the singleton Process.
    pub fn get() -> ProcessHandle<'a> {
        ProcessHandle {
            inner: PROCESS_TABLE.lock().unwrap(),
        }
    }
    pub fn set(pid: PID) {
        PROCESS_TABLE.lock().unwrap().current = pid as usize - 1;
    }
}

// impl<'a> Drop for ProcessHandle<'a> {
//     fn drop(&mut self) {
//         println!("<<< Dropping ProcessHandle");
//     }
// }

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
