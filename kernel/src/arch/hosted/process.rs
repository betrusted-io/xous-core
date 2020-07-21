pub const MAX_CONTEXT: TID = 31;
use crate::services::ProcessInner;
use core::cell::RefCell;
use std::io::Write;
use std::net::TcpStream;
use std::thread_local;
use xous::{ContextInit, TID, PID};

pub const INITIAL_CONTEXT: usize = 1;

pub struct Process {
    pid: PID,
}

struct ProcessImpl {
    /// Global parameters used by the operating system
    pub inner: ProcessInner,

    /// The network connection to the client process.
    conn: TcpStream,

    /// Memory that may need to be returned to the caller
    memory_to_return: Option<Vec<u8>>,

    /// This enables the kernel to keep track of threads in the
    /// target process, and know which threads are ready to
    /// receive messages.
    contexts: [Context; MAX_CONTEXT],

    /// The currently-active thread for this proces
    current_context: TID,
}

impl PartialEq for Process {
    fn eq(&self, other: &Process) -> bool {
        self.pid == other.pid
    }
}

struct ProcessTable {
    current: PID,
    table: Vec<Option<ProcessImpl>>,
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
#[derive(Copy, Clone, Debug)]
/// Everything required to keep track of a single thread of execution.
/// In a `std` environment, we can't manage threads so this is a no-op.
pub struct Context {
    allocated: bool,
}

impl Default for Context {
    fn default() -> Self {
        Context { allocated: false }
    }
}

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
        Process { pid: current_pid }
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
            let current = &process_table.table[process_table.current.get() as usize - 1]
                .as_ref()
                .unwrap();
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
            let current = &mut process_table.table[current_pid_idx].as_mut().unwrap();
            f(&mut current.inner)
        })
    }

    pub fn setup_context(
        &mut self,
        context: TID,
        _setup: ContextInit,
    ) -> Result<(), xous::Error> {
        println!(
            "KERNEL({}): Setting up context {} @ {:?}",
            self.pid,
            context,
            std::thread::current()
        );
        assert!(context > 0);
        PROCESS_TABLE.with(|pt| {
            let mut process_table = pt.borrow_mut();
            let current_pid_idx = process_table.current.get() as usize - 1;
            let process = &mut process_table.table[current_pid_idx].as_mut().unwrap();

            assert!(!process.contexts[context - 1].allocated);
            process.contexts[context - 1].allocated = true;
            println!(
                "KERNEL({}): self.contexts[{}].allocated = {}",
                current_pid_idx,
                context - 1,
                process.contexts[context - 1].allocated
            );
        });
        Ok(())
    }

    pub fn current_context(&mut self) -> TID {
        PROCESS_TABLE.with(|pt| {
            let mut process_table = pt.borrow_mut();
            let current_pid_idx = process_table.current.get() as usize - 1;
            let process = &mut process_table.table[current_pid_idx].as_mut().unwrap();
            process.current_context
        })
    }

    /// Set the current context number.
    pub fn set_context(&mut self, context: TID) -> Result<(), xous::Error> {
        assert!(context > 0);
        PROCESS_TABLE.with(|pt| {
            let mut process_table = pt.borrow_mut();
            let current_pid_idx = process_table.current.get() as usize - 1;
            let process = &mut process_table.table[current_pid_idx].as_mut().unwrap();
            assert!(process.contexts[context - 1].allocated);
            process.current_context = context;
        });
        Ok(())
    }

    #[allow(dead_code)]
    pub fn find_free_context_nr(&self) -> Option<TID> {
        PROCESS_TABLE.with(|pt| {
            let mut process_table = pt.borrow_mut();
            let current_pid_idx = process_table.current.get() as usize - 1;
            let process = &mut process_table.table[current_pid_idx].as_mut().unwrap();
            for (index, context) in process.contexts.iter().enumerate() {
                if index != 0 && !context.allocated {
                    return Some(index as TID + 1);
                }
            }
            None
        })
    }

    pub fn set_context_result(&mut self, context: TID, result: xous::Result) {
        assert!(context > 0);
        PROCESS_TABLE.with(|pt| {
            let mut process_table = pt.borrow_mut();
            let current_pid_idx = process_table.current.get() as usize - 1;
            let process = &mut process_table.table[current_pid_idx].as_mut().unwrap();
            assert!(
                process.contexts[context - 1].allocated,
                "context {} is not allocated",
                context,
            );

            let mut response = vec![];
            response.extend_from_slice(&context.to_le_bytes());
            for word in result.to_args().iter_mut() {
                response.extend_from_slice(&word.to_le_bytes());
            }

            if let Some(buf) = process.memory_to_return.take() {
                response.extend_from_slice(&buf);
            }

            process.conn.write_all(&response).expect("Disconnection");
        });
    }

    pub fn return_memory(&mut self, buf: &[u8]) {
        PROCESS_TABLE.with(|pt| {
            let mut process_table = pt.borrow_mut();
            let current_pid_idx = process_table.current.get() as usize - 1;
            let process = &mut process_table.table[current_pid_idx].as_mut().unwrap();
            assert!(process.memory_to_return.is_none());
            process.memory_to_return = Some(buf.to_vec());
        });
    }

    /// Initialize this process context with the given entrypoint and stack
    /// addresses.
    pub fn create(pid: PID, init_data: ProcessInit) -> PID {
        PROCESS_TABLE.with(|process_table| {
            let mut process_table = process_table.borrow_mut();
            let pid_idx = (pid.get() - 1) as usize;

            let process = ProcessImpl {
                inner: Default::default(),
                conn: init_data.conn,
                memory_to_return: None,
                current_context: INITIAL_CONTEXT,
                contexts: [Context {allocated: false}; MAX_CONTEXT],
            };
            if pid_idx >= process_table.table.len() {
                process_table.table.push(Some(process));
            } else if process_table.table[pid_idx].is_none() {
                process_table.table[pid_idx] = Some(process);
            } else {
                panic!("pid already allocated!");
            }
            pid
        })
    }

    pub fn destroy(pid: PID) -> Result<(), xous::Error> {
        PROCESS_TABLE.with(|pt| {
            let mut process_table = pt.borrow_mut();
            let pid_idx = pid.get() as usize - 1;
            if pid_idx >= process_table.table.len() {
                panic!("attempted to destroy PID that exceeds table index: {}", pid);
            }
            process_table.table[pid_idx] = None;
            Ok(())
        })
    }

    pub fn send(&mut self, bytes: &[u8]) -> Result<(), xous::Error> {
        PROCESS_TABLE.with(|pt| {
            let mut process_table = pt.borrow_mut();
            let current_pid_idx = process_table.current.get() as usize - 1;
            let process = &mut process_table.table[current_pid_idx].as_mut().unwrap();
            process.conn.write_all(bytes).unwrap();
        });
        Ok(())
    }
}

impl Context {}
