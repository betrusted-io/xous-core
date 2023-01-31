// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

pub const MAX_THREAD: TID = 31;
use crate::services::ProcessInner;
use core::cell::RefCell;
use std::io::Write;
use std::net::TcpStream;
use std::thread_local;
use xous_kernel::{ProcessInit, ProcessKey, ProcessStartup, ThreadInit, PID, TID};

pub const INITIAL_TID: usize = 2;
pub const EXCEPTION_TID: usize = 1;
pub const MAX_PROCESS_COUNT: usize = 32;

pub struct Process {
    pid: PID,
}

#[derive(Debug)]
struct ProcessImpl {
    /// Global parameters used by the operating system
    pub inner: ProcessInner,

    /// A 16-byte key used to register a process when it first starts
    key: ProcessKey,

    /// The network connection to the client process.
    conn: Option<TcpStream>,

    /// Memory that may need to be returned to the caller for each thread
    memory_to_return: [Option<Vec<u8>>; MAX_THREAD + 1],

    /// This enables the kernel to keep track of threads in the
    /// target process, and know which threads are ready to
    /// receive messages.
    threads: [Thread; MAX_THREAD + 1],

    /// The currently-active thread for this process
    current_thread: TID,
}

impl PartialEq for Process {
    fn eq(&self, other: &Process) -> bool {
        self.pid == other.pid
    }
}

struct ProcessTable {
    /// The process upon which the current syscall is operating
    current: PID,

    /// The number of processes that exist
    total: usize,

    /// The actual table contents
    table: Vec<Option<ProcessImpl>>,
}

thread_local!(
    static PROCESS_TABLE: RefCell<ProcessTable> = RefCell::new(ProcessTable {
        current: unsafe { PID::new_unchecked(1) },
        total: 0,
        table: Vec::new(),
    })
);

pub fn current_pid() -> PID {
    PROCESS_TABLE.with(|pt| pt.borrow().current)
}

pub fn set_current_pid(pid: PID) {
    PROCESS_TABLE.with(|pt| {
        let pid_idx = (pid.get() - 1) as usize;
        let mut pt = pt.borrow_mut();

        // // If the PID doesn't exist, only allow it if the table is
        // // currently empty.
        // for (idx, i) in pt.table.iter().enumerate() {
        //     println!("pt.table[{}]: {:?}", idx, i);
        // }
        match pt.table.get_mut(pid_idx) {
            None | Some(None) => {
                // if pid.get() != 1 || pt.total > 0 {
                panic!("PID {} does not exist", pid);
                // }
            }
            Some(_) => {}
        }
        pt.current = pid
    });
}

pub fn register_connection_for_key(
    mut conn: TcpStream,
    key: ProcessKey,
) -> Result<PID, xous_kernel::Error> {
    PROCESS_TABLE.with(|pt| {
        let mut process_table = pt.borrow_mut();
        for (pid_minus_1, process) in process_table.table.iter_mut().enumerate() {
            if let Some(process) = process.as_mut() {
                if process.key == key && process.conn.is_none() {
                    conn.write_all(&[pid_minus_1 as u8 + 1]).unwrap();
                    process.conn = Some(conn);
                    return Ok(PID::new(pid_minus_1 as u8 + 1).unwrap());
                }
            }
        }
        Err(xous_kernel::Error::ProcessNotFound)
    })
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
/// Everything required to keep track of a single thread of execution.
/// In a `std` environment, we can't manage threads so this is a no-op.
pub struct Thread {
    allocated: bool,
}

impl Default for Thread {
    fn default() -> Self {
        Thread { allocated: false }
    }
}

// /// Everything required to initialize a process on this platform
// pub struct ProcessInit {
//     /// A network connection to the client
//     conn: TcpStream,
// }

// impl ProcessInit {
//     pub fn new(conn: TcpStream) -> ProcessInit {
//         ProcessInit { conn }
//     }
// }

impl Process {
    pub fn current() -> Process {
        let current_pid = PROCESS_TABLE.with(|pt| pt.borrow().current);
        Process { pid: current_pid }
    }

    /// Mark this process as running (on the current core?!)
    pub fn activate(&mut self) -> Result<(), xous_kernel::Error> {
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

    /// Calls the provided function with the current inner process state.
    pub fn with_current_mut<F, R>(f: F) -> R
    where
        F: FnOnce(&mut Process) -> R,
    {
        let mut process = Self::current();
        f(&mut process)
    }

    #[allow(dead_code)]
    pub fn current_tid(&self) -> TID {
        1
    }

    fn setup_thread_inner(thread: TID, process_table: &mut ProcessTable) {
        let current_pid_idx = process_table.current.get() as usize - 1;
        let process = &mut process_table.table[current_pid_idx].as_mut().unwrap();

        assert!(!process.threads[thread - 1].allocated);
        process.threads[thread - 1].allocated = true;
    }

    pub fn retry_instruction(&mut self, _tid: TID) -> Result<(), xous_kernel::Error> {
        Ok(())
    }

    pub fn setup_process(pid: PID, setup: ThreadInit) -> Result<(), xous_kernel::Error> {
        let mut tmp = Process { pid };
        tmp.setup_thread(INITIAL_TID, setup)
    }

    pub fn setup_thread(
        &mut self,
        thread: TID,
        _setup: ThreadInit,
    ) -> Result<(), xous_kernel::Error> {
        // println!(
        //     "KERNEL({}): Setting up thread {} @ {:?}",
        //     self.pid,
        //     thread,
        //     std::thread::current()
        // );
        assert!(thread > 0);
        PROCESS_TABLE.with(|pt| {
            let process_table = &mut *pt.borrow_mut();
            Self::setup_thread_inner(thread, process_table);
        });
        Ok(())
    }

    /// Set the current thread ID.
    pub fn set_tid(&mut self, thread: TID) -> Result<(), xous_kernel::Error> {
        assert!(thread > 0);
        PROCESS_TABLE.with(|pt| {
            let mut process_table = pt.borrow_mut();
            let current_pid = process_table.current.get();
            let current_pid_idx = current_pid as usize - 1;
            let process = &mut process_table.table[current_pid_idx].as_mut().unwrap();
            assert!(
                process.threads[thread - 1].allocated,
                "process {} tried to switch to thread {} which wasn't allocated",
                current_pid,
                thread
            );
            process.current_thread = thread;
        });
        Ok(())
    }

    #[allow(dead_code)]
    pub fn find_free_thread(&self) -> Option<TID> {
        PROCESS_TABLE.with(|pt| {
            let mut process_table = pt.borrow_mut();
            let current_pid_idx = process_table.current.get() as usize - 1;
            let process = &mut process_table.table[current_pid_idx].as_mut().unwrap();
            for (index, thread) in process.threads.iter().enumerate() {
                if !thread.allocated {
                    return Some(index as TID + 1);
                }
            }
            None
        })
    }

    pub fn thread_exists(&self, tid: TID) -> bool {
        if tid == 0 {
            return false;
        }
        PROCESS_TABLE.with(|pt| {
            let process_table = pt.borrow();
            let current_pid = process_table.current.get();
            let current_pid_idx = current_pid as usize - 1;
            if let Some(Some(process)) = process_table.table.get(current_pid_idx) {
                process.threads.get(tid - 1).map(|t| t.allocated).unwrap_or(false)
            } else {
                false
            }
        })
    }

    pub fn set_thread_result(&mut self, tid: TID, result: xous_kernel::Result) {
        assert!(tid > 0);
        PROCESS_TABLE.with(|pt| {
            let mut process_table = pt.borrow_mut();
            let current_pid_idx = process_table.current.get() as usize - 1;
            let process = &mut process_table.table[current_pid_idx].as_mut().unwrap();
            // assert!(
            //     process.threads[tid - 1].allocated,
            //     "thread {} is not allocated",
            //     tid,
            // );

            let mut response = vec![];
            // Add the destination thread ID to the start of the packet.
            response.extend_from_slice(&tid.to_le_bytes());

            // Append the contents of the response packet.
            for word in result.to_args().iter_mut() {
                response.extend_from_slice(&word.to_le_bytes());
            }

            if let Some(mem) = result.memory() {
                let s = unsafe { core::slice::from_raw_parts(mem.as_ptr(), mem.len()) };
                klog!("adding {} additional bytes from result", s.len());
                response.extend_from_slice(s);
            }

            // If there is memory to return for this thread, also return that.
            if let Some(buf) = process
                .memory_to_return
                .get_mut(tid - 1)
                .and_then(|v| v.take())
            {
                if result.memory().is_some() {
                    panic!("Result has memory and we're also returning memory!");
                }
                klog!(
                    "adding {} additional bytes from memory being returned",
                    buf.len()
                );
                klog!("data: {:?}", buf);
                response.extend_from_slice(&buf);
            }

            klog!("setting thread return value to {} bytes", response.len());
            let conn = process.conn.as_mut().unwrap();
            conn.write_all(&response).expect("Disconnection");
            conn.flush().expect("Disconnection");
        });
    }

    pub fn return_memory(&mut self, tid: TID, buf: &[u8]) {
        PROCESS_TABLE.with(|pt| {
            let mut process_table = pt.borrow_mut();
            let current_pid_idx = process_table.current.get() as usize - 1;
            let process = &mut process_table.table[current_pid_idx].as_mut().unwrap();
            assert!(process.memory_to_return[tid - 1].is_none());
            process.memory_to_return[tid - 1] = Some(buf.to_vec());
        });
    }

    /// Initialize this process with the given memory space. THIS DOES NOT
    /// INITIALIZE A MAIN THREAD. You must call `setup_thread()` in order to
    /// select a main thread.
    pub fn create(
        pid: PID,
        init_data: ProcessInit,
        _services: &mut crate::SystemServices,
    ) -> Result<ProcessStartup, xous_kernel::Error> {
        PROCESS_TABLE.with(|process_table| {
            let mut process_table = process_table.borrow_mut();
            let pid_idx = (pid.get() - 1) as usize;
            use crate::filled_array;
            let process = ProcessImpl {
                inner: Default::default(),
                conn: None,
                key: init_data.key,
                memory_to_return: filled_array![None; 32 /* MAX_THREAD */],
                current_thread: INITIAL_TID,
                threads: [Thread { allocated: false }; MAX_THREAD + 1],
            };

            process_table.total += 1;
            if pid_idx >= process_table.table.len() {
                process_table.table.push(Some(process));
            } else if process_table.table[pid_idx].is_none() {
                process_table.table[pid_idx] = Some(process);
            } else {
                panic!("pid already allocated!");
            }
            Ok(ProcessStartup::new(pid))
        })
    }

    pub fn destroy(pid: PID) -> Result<(), xous_kernel::Error> {
        PROCESS_TABLE.with(|pt| {
            let mut process_table = pt.borrow_mut();
            let pid_idx = pid.get() as usize - 1;
            if pid_idx >= process_table.table.len() {
                panic!("attempted to destroy PID that exceeds table index: {}", pid);
            }
            let process = process_table.table[pid_idx].as_mut().unwrap();
            process
                .conn
                .as_mut()
                .unwrap()
                .shutdown(std::net::Shutdown::Both)
                .ok();
            process_table.table[pid_idx] = None;
            process_table.total -= 1;
            Ok(())
        })
    }

    pub fn send(&mut self, bytes: &[u8]) -> Result<(), xous_kernel::Error> {
        // eprintln!("KERNEL: Sending syscall response: {:?}", bytes);
        PROCESS_TABLE.with(|pt| {
            let mut process_table = pt.borrow_mut();
            let current_pid_idx = process_table.current.get() as usize - 1;
            let process = &mut process_table.table[current_pid_idx].as_mut().unwrap();
            let conn = process.conn.as_mut().unwrap();
            conn.write_all(bytes).unwrap();
            // conn.flush().unwrap();
        });
        Ok(())
    }
}

impl Thread {}
