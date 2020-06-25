pub const MAX_CONTEXT: CtxID = 31;
use crate::services::ProcessInner;
use core::cell::RefCell;
use std::io::Write;
use std::net::TcpStream;
use std::thread_local;
use xous::{CtxID, PID};

pub type ContextInit = ();
pub const INITIAL_CONTEXT: usize = 1;

pub struct Process {
    pid: PID,
}

struct ProcessImpl {
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
    current: PID,
    table: Vec<ProcessImpl>,
}

thread_local!(
    static PROCESS_TABLE: RefCell<ProcessTable> = RefCell::new(ProcessTable {
        current: unsafe { PID::new_unchecked(1) },
        table: Vec::new(),
    })
);

pub fn current_pid() -> PID {
    PROCESS_TABLE.with(|pt| pt.borrow().current)
}

pub fn set_current_pid(pid: PID) {
    PROCESS_TABLE.with(|pt| (*pt.borrow_mut()).current = pid);
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
    pub fn current() -> Process {
        let current_pid = PROCESS_TABLE.with(|pt| pt.borrow().current);
        Process{pid: current_pid}
    }

    /// Mark this process as running (on the current core?!)
    pub fn activate(&mut self) -> Result<(), xous::Error> {
        // let mut pt = PROCESS_TABLE.lock().unwrap();
        // assert!(pt.table[self.pid as usize - 1] == *self);
        // pt.current = self.pid as _;
        Ok(())
    }

    /// Calls the provided function with the current inner process state.
    pub fn with_inner<F, R>(f: F) -> R
    where
        F: FnOnce(&ProcessInner) -> R,
    {
        PROCESS_TABLE.with(|pt| {
            let process_table = pt.borrow();
            let current = &process_table.table[process_table.current.get() as usize - 1];
            f(&current.inner)
        })
    }

    pub fn with_inner_mut<F, R>(f: F) -> R
    where
        F: FnOnce(&mut ProcessInner) -> R,
    {
        PROCESS_TABLE.with(|pt| {
            let mut process_table = pt.borrow_mut();
            let current_pid_idx = process_table.current.get() as usize - 1;
            let current = &mut process_table.table[current_pid_idx];
            f(&mut current.inner)
        })
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
        PROCESS_TABLE.with(|pt| {
            let mut process_table = pt.borrow_mut();
            let current_pid_idx = process_table.current.get() as usize - 1;
            let process = &mut process_table.table[current_pid_idx];
            for word in result.to_args().iter_mut() {
                process.conn
                    .write_all(&word.to_le_bytes())
                    .expect("Disconnection");
            }
        });
    }

    /// Initialize this process context with the given entrypoint and stack
    /// addresses.
    pub fn create(pid: PID, init_data: ProcessInit) -> PID {
        PROCESS_TABLE.with(|process_table| {
            let mut process_table = process_table.borrow_mut();
            let pid_idx = (pid.get() - 1) as usize;

            assert!(
                pid_idx >= process_table.table.len(),
                "PID {} already allocated",
                pid
            );
            let process = ProcessImpl {
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
        })
    }

    pub fn send(&mut self, bytes: &[u8]) -> Result<(), xous::Error> {
        PROCESS_TABLE.with(|pt| {
            let mut process_table = pt.borrow_mut();
            let current_pid_idx = process_table.current.get() as usize - 1;
            let process = &mut process_table.table[current_pid_idx];
            process.conn.write_all(bytes).unwrap();
        });
        Ok(())
    }
}

impl Context {}

// pub struct ProcessHandle {
//     inner: RefCell<ProcessTable>,
// }

// /// Wraps the MemoryManager in a safe mutex.  Because of this, accesses
// /// to the Memory Manager should only be made during interrupt contexts.
// impl ProcessHandle {
//     /// Get the singleton Process.
//     pub fn get() -> ProcessHandle {
//         ProcessHandle {
//             inner: *PROCESS_TABLE.with(|pt| pt.clone()),
//         }
//     }
// }

// // impl<'a> Drop for ProcessHandle<'a> {
// //     fn drop(&mut self) {
// //         println!("<<< Dropping ProcessHandle");
// //     }
// // }

// use core::ops::{Deref, DerefMut};
// impl Deref for ProcessHandle {
//     type Target = Process;
//     fn deref(&self) -> &Process {
//         &self.inner.table[self.inner.current]
//     }
// }

// impl DerefMut for ProcessHandle {
//     fn deref_mut(&mut self) -> &mut Process {
//         let current = self.inner.current;
//         &mut self.inner.table[current]
//     }
// }
