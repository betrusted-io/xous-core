// SPDX-FileCopyrightText: 2024 bunnie <bunnie@kosagi.com>
// SPDX-License-Identifier: Apache-2.0

use core::cmp::Ordering;

use bao1x_hal::udma::FLASH_SECTOR_LEN;
use loader::SWAP_FLG_WIRED;
use xous_kernel::SWAPPER_PID;
use xous_kernel::arch::EXCEPTION_STACK_TOP;
use xous_kernel::arch::MMAP_VIRT_BASE;
use xous_kernel::arch::PAGE_SIZE;
use xous_kernel::arch::SWAP_RPT_VADDR;
use xous_kernel::{PID, SysCallResult, TID};

use crate::arch::current_pid;
use crate::arch::mem::MMUFlags;
use crate::mem::MemoryManager;
use crate::services::SystemServices;

/// This might change depending on the target, link options, and version of LLVM.
/// The good news is it's static with every version, and constant after the first
/// invocation. So, if it works once, it should keep on working for that particular
/// combination of target/link options/compiler version.
#[cfg(any(feature = "debug-swap", feature = "debug-swap-verbose"))] // more stack needed for debug
const BACKUP_STACK_SIZE_WORDS: usize = 1536 / core::mem::size_of::<usize>();
#[cfg(not(any(feature = "debug-swap", feature = "debug-swap-verbose")))]
const BACKUP_STACK_SIZE_WORDS: usize = 1536 / core::mem::size_of::<usize>();

/// userspace swapper -> kernel ABI (see kernel/src/syscall.rs)
/// This ABI is copy-paste synchronized with what's in the userspace handler. It's left out of
/// xous-rs so that we can change it without having to push crates to crates.io.
/// Since there is only one place the ABI could be used, we're going to stick with
/// this primitive method of synchronization because it reduces the activation barrier
/// to fix bugs.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SwapAbi {
    Invalid = 0,
    ClearMemoryNow = 1,
    GetFreePages = 2,
    RetrievePage = 3,
    HardOom = 4,
    StealPage = 5,
    ReleaseMemory = 6,
    WritePage = 7,
    BlockErase = 8,
}
/// SYNC WITH `xous-swapper/src/main.rs`
impl SwapAbi {
    pub fn from(val: usize) -> SwapAbi {
        use SwapAbi::*;
        match val {
            1 => ClearMemoryNow,
            2 => GetFreePages,
            3 => RetrievePage,
            4 => HardOom,
            5 => StealPage,
            6 => ReleaseMemory,
            7 => WritePage,
            8 => BlockErase,
            _ => Invalid,
        }
    }
}

/// kernel -> swapper handler ABI
/// This is the response ABI from the kernel into the swapper. It also tracks intermediate state
/// necessary for proper return from these calls.
#[derive(Debug, Copy, Clone)]
pub enum BlockingSwapOp {
    /// PID of the target block, current paddr of block, original vaddr in the space of target block PID,
    /// physical address of block. Returns to PID of the target block.
    ReadFromSwap(PID, TID, usize, usize, usize),
    /// PID of the target block, current paddr of block, original vaddr in the space of target block PID,
    /// physical address of block. Returns to PID of the target block.
    WriteToFlash(PID, TID, PID, usize, usize),
    /// PID/TID tuple; PID of the caller; block offset (with vaddr prefix stripped) and length in bytes, but
    /// the length must be a multiple of block length
    BulkErase(PID, TID, PID, usize, usize),
    /// Immediate OOM. Drop everything and try to recover; from here until exit, everything runs in
    /// an un-interruptable context, no progress allowed. This currently can only originate from one
    /// location, if we need multi-location origin then we have to also track the re-entry point in the
    /// kernel memory cycle. Arguments are the TID/PID of the context running that triggered the HardOom,
    /// as well as the virtual address that triggered the hard OOM.
    HardOomSyscall(TID, PID),
}

/// This structure is a copy of what's defined in the loader's swap module. The reason
/// we can't condense the two APIs is because PID is actually defined differently in the
/// kernel than in the loader: a PID for the loader is a `u8`. A PID for the kernel is a
/// NonZeroU8. Thus the APIs are not compatible, and we have to maintain two sets of
/// calls.
///
/// Also, critically, `SwapAlloc` in the kernel needs an `update` and `reparent` call,
/// which increments `next_epoch` -- these can't be migrated into the swapper, and Rust
/// does not allow extensions to types that aren't native to your crate. So, we're left
/// with redundant copies of the structure definition.
#[derive(Debug)]
#[repr(C)]
pub struct SwapAlloc {
    timestamp: u32,
    /// virtual_page_number[19:0] | flags[3:0] | pid[7:0]
    vpn: u32,
}

impl SwapAlloc {
    pub fn is_pid(&self, pid: PID) -> bool { self.vpn as u8 == pid.get() }

    #[allow(dead_code)]
    pub fn is_some(&self) -> bool { self.vpn as u8 != 0 }

    pub fn is_none(&self) -> bool { self.vpn as u8 == 0 }

    pub fn get_pid(&self) -> Option<PID> {
        if (self.vpn & 0xFF) as u8 == 0 { None } else { Some(PID::new(self.vpn as u8).unwrap()) }
    }

    pub unsafe fn update(&mut self, pid: Option<PID>, vaddr: Option<usize>) {
        crate::swap::Swap::with_mut(|s| {
            self.timestamp = s.next_epoch();
            if let Some(pid) = pid {
                s.track_alloc(true);
                if let Some(va) = vaddr {
                    // ensure swapper pages are always hard wired
                    let hard_wired = if pid.get() == 2 { SWAP_FLG_WIRED } else { 0 };
                    #[cfg(feature = "debug-swap-verbose")]
                    println!(
                        "-- update {}/{:x} <- {:x} @ {:x} (in pid{})",
                        pid.get(),
                        va,
                        (va as u32) & !0xFFF | pid.get() as u32 | self.vpn & SWAP_FLG_WIRED | hard_wired,
                        ((self as *const Self as usize
                            - crate::mem::MemoryManager::with(|mm| mm.rpt_base()))
                            / core::mem::size_of::<SwapAlloc>())
                            * PAGE_SIZE,
                        crate::arch::process::Process::with_current(|p| p.pid().get()),
                    );
                    // preserve the wired flag if it was set previously by the loader
                    self.vpn =
                        (va as u32) & !0xFFF | pid.get() as u32 | self.vpn & SWAP_FLG_WIRED | hard_wired;
                } else {
                    self.vpn = SWAP_FLG_WIRED | pid.get() as u32;
                }
            } else {
                #[cfg(feature = "debug-swap-verbose")]
                println!("-- release of pid{}/{:x}", self.vpn as u8, self.vpn & !0xfff);
                s.track_alloc(false);
                // allow the wired flag to clear if the page is actively de-allocated
                self.vpn = 0;
            }
        });
    }

    pub unsafe fn touch(&mut self) {
        crate::swap::Swap::with_mut(|s| {
            self.timestamp = s.next_epoch();
        });
    }

    pub fn set_wired(&mut self) { self.vpn |= SWAP_FLG_WIRED; }

    #[cfg(feature = "debug-swap-verbose")]
    pub fn get_raw_vpn(&self) -> u32 { self.vpn }

    pub fn get_timestamp(&self) -> u32 { self.timestamp }

    pub fn set_timestamp(&mut self, val: u32) { self.timestamp = val }

    pub unsafe fn reparent(&mut self, pid: PID) {
        self.timestamp = crate::swap::Swap::with_mut(|s| s.next_epoch());
        self.vpn = self.vpn & !&0xFFu32 | pid.get() as u32;
    }
}

impl PartialEq for SwapAlloc {
    fn eq(&self, other: &Self) -> bool {
        self.timestamp == other.timestamp && self.timestamp == other.timestamp
    }
}

impl Eq for SwapAlloc {}

impl PartialOrd for SwapAlloc {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { self.timestamp.partial_cmp(&other.timestamp) }
}

impl Ord for SwapAlloc {
    fn cmp(&self, other: &Self) -> Ordering { self.partial_cmp(other).unwrap() }
}

#[cfg(baremetal)]
#[no_mangle]
static mut SWAP: Swap = Swap {
    pc: 0,
    prev_op: None,
    nested_op: None,
    swapper_state: 0,
    swapper_args: [0usize; 8],
    epoch: 0,
    used_pages: 0,
    mem_alloc_tracker_paddr: 0,
    mem_alloc_tracker_pages: 0,
    oom_irq_backing: None,
    clearmem_irq_backing: None,
    unmap_rpt_after_hard_oom: false,
    oom_thread_backing: None,
    // hand-tuned based on feedback from actual runtime data
    oom_stack_backing: [0usize; BACKUP_STACK_SIZE_WORDS],
    oom_stashed_pid: None,
};

pub struct Swap {
    /// PC for blocking handler
    pc: usize,
    /// previous op
    prev_op: Option<BlockingSwapOp>,
    /// previous previous op - I think that structurally, we can only get a nesting of depth = 2, and this
    /// is due to things like an alloc advisory pulling in a new superpage while handling the advisory...?
    /// Anyways, there is an assert that checks for the overflow condition of nesting.
    nested_op: Option<BlockingSwapOp>,
    /// Userspace state pointer for the swapper. This is a PID-2 local virtual address, passed from the
    /// swapper on registration
    swapper_state: usize,
    /// storage for args
    swapper_args: [usize; 8],
    /// swap epoch tracker
    epoch: u32,
    /// approximate used memory, in pages
    used_pages: usize,
    /// physical base of memory allocation tracker
    mem_alloc_tracker_paddr: usize,
    /// number of pages to map when handling the tracker
    mem_alloc_tracker_pages: usize,
    /// state of IRQ backing before hard OOM
    oom_irq_backing: Option<usize>,
    /// state of IRQ backing before a ClearMem syscall
    clearmem_irq_backing: Option<usize>,
    /// Keep track of if we should unmap the RPT after a hard OOM. In the case that we hard-OOM while the
    /// soft-OOM handler is running, the RPT will already be mapped into the swapper's space. This is not
    /// uncommon, because the soft-OOM handler is interruptable and can only keep up with gradual memory
    /// demand.
    unmap_rpt_after_hard_oom: bool,
    /// backing for the thread state that is smashed by the OOM handler
    oom_thread_backing: Option<crate::arch::process::Thread>,
    /// backing for the stack that is smashed by the OOM handler
    oom_stack_backing: [usize; BACKUP_STACK_SIZE_WORDS],
    /// address space to restore after an OOM, if necessary
    oom_stashed_pid: Option<PID>,
}
impl Swap {
    pub fn with_mut<F, R>(f: F) -> R
    where
        F: FnOnce(&mut Swap) -> R,
    {
        #[cfg(baremetal)]
        unsafe {
            f(&mut *core::ptr::addr_of_mut!(SWAP))
        }

        #[cfg(not(baremetal))]
        SWAP.with(|ss| f(&mut ss.borrow_mut()))
    }

    #[cfg(baremetal)]
    pub fn with<F, R>(f: F) -> R
    where
        F: FnOnce(&Swap) -> R,
    {
        #[cfg(baremetal)]
        unsafe {
            f(&*core::ptr::addr_of!(SWAP))
        }

        #[cfg(not(baremetal))]
        SWAP.with(|ss| f(&ss.borrow_mut()))
    }

    pub fn next_epoch(&mut self) -> u32 {
        // we save u32::MAX as a search-stop sentinel for the renormalization algorithm
        // we can set this even lower if we want to stress-test epoch renormalization
        if self.epoch < u32::MAX - 1 {
            #[cfg(feature = "debug-swap-verbose")]
            println!("Epoch {}", self.epoch);
            self.epoch += 1;
        } else {
            println!("Epoch before renormalization: {:x}", self.epoch);
            // disable interrupts prior to renormalize
            let sim_backing = sim_read();
            sim_write(0x0);
            // safety: this happens only within an exception handler, with interrupts disabled, and thus there
            // should no concurrent access to the underlying data structure. The function itself
            // promises not to create page faults, as it is coded to use only limited stack allocations.
            self.epoch = unsafe {
                crate::mem::renormalize_allocs()
                // enable interrupts after renormalize
            };
            sim_write(sim_backing);
            // the returned value is the greatest used epoch timestamp, so we have to add one for correctness
            self.epoch += 1;
            println!("Epoch after renormalization: {:x}", self.epoch);
        }
        self.epoch
    }

    pub fn track_alloc(&mut self, is_alloc: bool) {
        if is_alloc {
            self.used_pages = self.used_pages.saturating_add(1);
        } else {
            self.used_pages = self.used_pages.saturating_sub(1);
        }
    }

    pub fn register_handler(
        &mut self,
        _s0: u32,
        _s1: u32,
        _s2: u32,
        _s3: u32,
        handler: usize,
        state: usize,
    ) -> Result<xous_kernel::Result, xous_kernel::Error> {
        // initialize used_pages with a comprehensive count of RAM usage
        let mut total_bytes = 0;
        crate::services::SystemServices::with(|system_services| {
            crate::mem::MemoryManager::with(|mm| {
                for process in &system_services.processes {
                    if !process.free() {
                        let bytes_used = mm.ram_used_by(process.pid);
                        total_bytes += bytes_used;
                    }
                }
            });
        });
        self.used_pages = total_bytes / PAGE_SIZE;

        // now register the swapper
        if self.pc == 0 && self.swapper_state == 0 {
            self.pc = handler;
            self.swapper_state = state;
            #[cfg(feature = "debug-swap")]
            println!("handler registered: pc {:?} state {:?}", self.pc, self.swapper_state);
            Ok(xous_kernel::Result::Scalar5(0, 0, 0, 0, 0))
        } else {
            // someone is trying to steal the swapper's privileges!
            #[cfg(feature = "debug-swap")]
            println!("Handler double-register detected!");
            Err(xous_kernel::Error::AccessDenied)
        }
    }

    pub fn init_rpt(&mut self, base: usize, pages: usize) {
        self.mem_alloc_tracker_paddr = crate::arch::mem::virt_to_phys(base).unwrap() as usize;
        self.mem_alloc_tracker_pages = pages;
    }

    /// This is a non-divergent syscall (handled entirely within the kernel)
    pub fn get_free_mem(&self) -> SysCallResult {
        // #[cfg(feature = "debug-swap")]
        {
            println!("RAM usage:");
            let mut total_bytes = 0;
            crate::services::SystemServices::with(|system_services| {
                crate::mem::MemoryManager::with(|mm| {
                    for process in &system_services.processes {
                        if !process.free() {
                            let bytes_used = mm.ram_used_by(process.pid);
                            total_bytes += bytes_used;
                            println!(
                                "    PID {:>3}: {:>4} k {}",
                                process.pid,
                                bytes_used / 1024,
                                system_services.process_name(process.pid).unwrap_or("")
                            );
                        }
                    }
                });
            });
            println!("Via count: {} k total", total_bytes / 1024);
            println!("Via tracked alloc: {} k total", self.used_pages * PAGE_SIZE / 1024);
        }
        let ram_size = crate::mem::MemoryManager::with(|mm| mm.memory_size());
        Ok(xous_kernel::Result::Scalar5(
            ram_size / PAGE_SIZE - self.used_pages,
            ram_size / PAGE_SIZE,
            0,
            0,
            0,
        ))
    }

    pub fn hard_oom_syscall(&mut self) -> SysCallResult {
        let original_pid = crate::arch::process::current_pid();
        let original_tid = crate::arch::process::current_tid();

        // move into the swapper's memory space & map the RPT into the swapper's space so it can
        // make decisions about what to move out.
        SystemServices::with(|system_services| {
            let swapper_pid = PID::new(xous_kernel::SWAPPER_PID).unwrap();
            // swap to the swapper space
            let swapper_map = system_services.get_process(swapper_pid).unwrap().mapping;
            swapper_map.activate().unwrap();

            // map the RPT into userspace
            // note that this technically makes the RPT shared with kernel and userspace, but,
            // the userspace version is read-only, and the next section runs in an interrupt context,
            // so I think it's safe for it to be shared for that duration.
            let mut mapped_pages = false;
            let mut already_mapped = false;
            MemoryManager::with_mut(|mm| {
                for page in 0..self.mem_alloc_tracker_pages {
                    let virt = SWAP_RPT_VADDR + page * PAGE_SIZE;
                    let entry = unsafe {
                        // assume that the soft-OOM handler has run at least once already, so that
                        // the L1 PTEs exist. If we hard-OOM before the soft-OOM handler ever runs,
                        // the below would panic. We can solve this by having the user space swapper
                        // force at least one fetch of the RPT on boot (this is probably a good idea
                        // anyways as it ensures that any heap space it needs to handle this is
                        // allocated).
                        crate::arch::mem::pagetable_entry(virt)
                            .expect("Couldn't access PTE for SWAP_RPT_VADDR; ensure soft-OOM handler runs once before hard-OOM")
                            .read_volatile()
                    };
                    // only map pages if they aren't already mapped
                    if entry & MMUFlags::VALID.bits() == 0 {
                        mapped_pages = true;
                        crate::arch::mem::map_page_inner(
                            mm,
                            swapper_pid,
                            self.mem_alloc_tracker_paddr + page * PAGE_SIZE,
                            virt,
                            xous_kernel::MemoryFlags::R,
                            true,
                        )
                        .ok();
                        // unsafe { crate::arch::mem::flush_mmu() }; // redundant?
                    } else {
                        already_mapped = true;
                    }
                }
                if mapped_pages && already_mapped {
                    // I *think* that whether we map or don't map is always going to be all-or-nothing.
                    // However, in the case that only some pages have to be mapped, it probably means that
                    // somehow, we hard-OOM'd right in the middle of the page map/unmap routine within
                    // the soft-OOM handler. Shouldn't be possible -- `fetch_allocs` immediately disables
                    // interrupts -- but let's sanity check this assumption anyways.
                    todo!("Need to handle partial RPT maps -- tracking vector required in swapper");
                } else if mapped_pages {
                    self.unmap_rpt_after_hard_oom = true;
                }
            });
        });

        #[cfg(feature = "debug-swap")]
        println!("hard_oom - userspace activate");
        // this is safe because we're now in the swapper memory context, thanks to the previous call
        unsafe {
            self.blocking_activate_swapper(BlockingSwapOp::HardOomSyscall(original_tid, original_pid));
        }
    }

    pub fn swap_stop_irq(&mut self) {
        // disable all IRQs; no context swapping is allowed
        self.oom_irq_backing = Some(sim_read());
        sim_write(0x0);
        #[cfg(feature = "debug-swap-verbose")]
        println!("Swap stored SIM: {:x?}", self.oom_irq_backing);
    }

    pub fn swap_restore_irq(&mut self) {
        // restore IRQ state (don't borrow the system handler's state tracker, so we don't smash
        // it by accident)
        let sim = self.oom_irq_backing.take().expect("Someone stole our IRQ backing!");
        #[cfg(feature = "debug-swap-verbose")]
        println!("Swap restoring IRQ: {:x}", sim);
        sim_write(sim);
    }

    pub fn clearmem_stop_irq(&mut self) {
        self.clearmem_irq_backing = Some(sim_read());
        sim_write(0x0);
        #[cfg(feature = "debug-swap")]
        println!("Clearmem stored SIM: {:x?}", self.clearmem_irq_backing);
    }

    pub fn clearmem_restore_irq(&mut self) {
        if let Some(sim) = self.clearmem_irq_backing.take() {
            #[cfg(feature = "debug-swap")]
            println!("Clearmem restoring IRQ: {:x}", sim);
            sim_write(sim);
        }
    }

    pub fn swap_reentrant_syscall(&mut self, call: xous_kernel::SysCall) {
        self.swap_stop_irq();

        // Check that our address spaces are consistent. There is an edge case where
        // if we entered this via an OOM during a move or lend, our hardware address space
        // is in the target process but the process is still in the source. Return the address
        // space to the source for the duration of the OOM processing.
        //
        // Ideally, we could also check that our syscall is an OOM but it seems that touching
        // this variable mucks with other optimizations, and makes the stack saving fail.
        {
            let pid = crate::arch::process::current_pid();
            let hardware_pid = crate::arch::current_pid();
            if pid != hardware_pid {
                self.oom_stashed_pid = Some(hardware_pid);
                println!(
                    "OOM in move or lend. Reverting to previous address space {}->{}",
                    hardware_pid.get(),
                    pid.get()
                );
                SystemServices::with(|ss| {
                    ss.get_process(pid).unwrap().mapping.activate().unwrap();
                })
            }
        }

        #[cfg(feature = "debug-swap")]
        crate::arch::process::Process::with_current(|p| {
            println!(
                "BEF HANDLER {:x?} {}.{} sepc: {:x} sstatus: {:x?} satp: {:x?}",
                call,
                p.pid().get(),
                p.current_tid(),
                riscv::register::sepc::read(),
                riscv::register::sstatus::read().spp(),
                riscv::register::satp::read(),
            )
        });

        // we are in the pid/tid of the invoking process
        // stash the thread state of the current pid/tid because the syscall return trampoline will smash it
        self.oom_thread_backing =
            Some(crate::arch::process::Process::with_current(|p| p.current_thread().clone()));

        // about to be smashed by the syscall invocation. Save the stack with a specially crafted routine
        // that has the following properties:
        //   1. It does not modify the stack
        //   2. It copies all of the current stack to a backup location
        //   3. It can restore stack after the call is done, also without modifying stack
        //
        // Thus the routine is coded to use just temporary registers (in this case, the argument registers
        // that we know we won't be using in the upcoming re-entrant syscall), and manually checked for
        // no stack manipulations by disassembling the code. It may be necessary to disassemble the code
        // again with future Rust/llvm updates because the behavior of no-stack mods is not guaranteed.

        // Print the stack pointer for checking purposes
        let mut current_sp: usize;
        #[cfg(feature = "debug-swap-verbose")]
        {
            unsafe {
                core::arch::asm!(
                    "mv {current_sp}, sp",
                    current_sp = out(reg) current_sp,
                )
            }
            println!(" --> HANDLER SP extent d'{} bytes", EXCEPTION_STACK_TOP - current_sp);
        }
        // make a backup copy of the stack
        let backup_stack_ptr: usize = self.oom_stack_backing.as_ptr() as usize;
        let working_stack_end: usize = EXCEPTION_STACK_TOP;
        let working_stack_ptr: usize =
            EXCEPTION_STACK_TOP - self.oom_stack_backing.len() * core::mem::size_of::<usize>();
        unsafe {
            #[rustfmt::skip]
            core::arch::asm!(
                "mv   a7, {working_stack_end}",
                "mv   a6, {working_stack_ptr}",
                "mv   a5, {backup_stack_ptr}",
            "100:",
                "lw   a4, 0(a6)",
                "sw   a4, 0(a5)",
                "addi  a6, a6, 4",
                "addi  a5, a5, 4",
                "blt   a6, a7, 100b",
                "mv    {current_sp}, sp",

                working_stack_end = in(reg) working_stack_end,
                working_stack_ptr = in(reg) working_stack_ptr,
                backup_stack_ptr = in(reg) backup_stack_ptr,
                current_sp = out(reg) current_sp,
            );
        }
        // this assert will hopefully save us during CI testing if parameters changed and we have to fix them
        // the assert itself would modify stack, but, we're on the path to a panic so that's OK
        assert!(
            EXCEPTION_STACK_TOP - current_sp <= self.oom_stack_backing.len() * core::mem::size_of::<usize>(),
            "Backing stack not large enough to handle current kernel stack utilization."
        );
        // invoke this as a nested syscall so it returns here. No stack mods are allowed between the
        // backup and this call
        xous_kernel::rsyscall(call).ok();

        // restore the stack immediately after the syscall return (no debug info etc. allowed as it may
        // rely on stack).
        let restore_stack_ptr: usize = self.oom_stack_backing.as_ptr() as usize;
        let working_restore_end: usize = EXCEPTION_STACK_TOP;
        let working_restore_ptr: usize =
            EXCEPTION_STACK_TOP - self.oom_stack_backing.len() * core::mem::size_of::<usize>();
        unsafe {
            #[rustfmt::skip]
            core::arch::asm!(
                "mv   a7, {working_restore_end}",
                "mv   a6, {working_restore_ptr}",
                "mv   a5, {restore_stack_ptr}",
            "100:",
                "lw   a4, 0(a5)",
                "sw   a4, 0(a6)",
                "addi  a6, a6, 4",
                "addi  a5, a5, 4",
                "blt   a6, a7, 100b",

                working_restore_end = in(reg) working_restore_end,
                working_restore_ptr = in(reg) working_restore_ptr,
                restore_stack_ptr = in(reg) restore_stack_ptr,
            );
        }

        #[cfg(feature = "debug-swap-verbose")]
        crate::arch::process::Process::with_current(|p| {
            println!(
                "AFT HANDLER {}.{} sepc: {:x} sstatus: {:x?} satp: {:x?}",
                p.pid().get(),
                p.current_tid(),
                riscv::register::sepc::read(),
                riscv::register::sstatus::read().spp(),
                riscv::register::satp::read(),
            )
        });

        // then restore the smashed thread backing state on return from the syscall
        crate::arch::process::Process::with_current_mut(|p| {
            *p.current_thread_mut() =
                self.oom_thread_backing.take().expect("No thread backing was set prior to OOM handler")
        });

        // recover from OOM in move or lend
        if let Some(pid) = self.oom_stashed_pid.take() {
            println!("Restoring address space to {}", pid.get());
            SystemServices::with(|ss| {
                ss.get_process(pid).unwrap().mapping.activate().unwrap();
            })
        }

        self.swap_restore_irq();
    }

    /// The address space on entry to `retrieve_page` is `target_pid`; it must ensure
    /// that the address space is still `target_pid` on return.
    ///
    /// Also takes as argument the virtual address of the target page in the target PID,
    /// as well as the physical address of the page.
    ///
    /// Note that the originating caller to this will update the epoch of the memory
    /// alloc counter, so, we don't have to update it explicitly here (as we do with
    /// evict_page)
    ///
    /// This call diverges into the userspace swapper.
    /// Divergent calls must turn off IRQs before memory spaces are changed.
    pub fn retrieve_page_syscall(&mut self, target_vaddr_in_pid: usize, paddr: usize) -> ! {
        let target_pid = crate::arch::process::current_pid();
        let target_tid = crate::arch::process::current_tid();

        let block_vaddr_in_swap =
            crate::arch::mem::map_page_to_swapper(paddr).expect("couldn't map target page to swapper");
        // we are now in the swapper's memory space

        #[cfg(feature = "debug-swap-verbose")]
        println!(
            "retrieve_page - userspace activate from pid{:?}/tid{:?} for vaddr {:x?} -> paddr {:x?}",
            target_pid, target_tid, target_vaddr_in_pid, paddr
        );
        // this is safe because map_page_to_swapper() leaves us in the swapper memory context
        unsafe {
            self.blocking_activate_swapper(BlockingSwapOp::ReadFromSwap(
                target_pid,
                target_tid,
                target_vaddr_in_pid,
                block_vaddr_in_swap,
                paddr,
            ));
        }
    }

    pub fn write_page_syscall(
        &mut self,
        src_pid: PID,
        flash_offset: usize,
        page_vaddr_in_swapper: usize,
    ) -> ! {
        let target_pid = crate::arch::process::current_pid();
        let target_tid = crate::arch::process::current_tid();

        #[cfg(feature = "debug-swap")]
        println!(
            "write_page - userspace activate for pid{:?}/tid{:?} for vaddr {:x?} -> offset {:x?}",
            target_pid, target_tid, page_vaddr_in_swapper, flash_offset
        );
        // prevent context switching to avoid re-entrant calls while handling a call
        self.swap_stop_irq();
        // this is safe because the syscall pre-amble checks that we're in the swapper context
        unsafe {
            self.blocking_activate_swapper(BlockingSwapOp::WriteToFlash(
                target_pid,
                target_tid,
                src_pid,
                page_vaddr_in_swapper,
                flash_offset,
            ));
        }
    }

    pub fn block_erase_syscall(&mut self, src_pid: PID, offset: usize, len: usize) -> ! {
        let target_pid = crate::arch::process::current_pid();
        let target_tid = crate::arch::process::current_tid();

        #[cfg(feature = "debug-swap")]
        println!(
            "block_erase - userspace activate for pid{:?}/tid{:?} for offset {:x?}",
            target_pid, target_tid, offset
        );
        // prevent context switching to avoid re-entrant calls while handling a call
        self.swap_stop_irq();
        // this is safe because the syscall pre-amble checks that we're in the swapper context
        unsafe {
            self.blocking_activate_swapper(BlockingSwapOp::BulkErase(
                target_pid, target_tid, src_pid, offset, len,
            ));
        }
    }

    /// Safety:
    ///   - the current page table mapping context must be PID 2 (the swapper's PID) for this to work
    ///   - interrupts must have been disabled prior to setting the context to PID 2
    /// `op` contains the opcode data
    /// `payload_ptr` is the pointer to the virtual address of the swapped block in PID2 space
    unsafe fn blocking_activate_swapper(&mut self, op: BlockingSwapOp) -> ! {
        // setup the argument block
        match op {
            BlockingSwapOp::ReadFromSwap(pid, _tid, vaddr_in_pid, vaddr_in_swap, _paddr) => {
                self.swapper_args[0] = self.swapper_state;
                self.swapper_args[1] = 1; // ReadFromSwap
                self.swapper_args[2] = pid.get() as usize;
                self.swapper_args[3] = vaddr_in_pid;
                self.swapper_args[4] = vaddr_in_swap;
            }
            BlockingSwapOp::WriteToFlash(_pid, _tid, _src_pid, vaddr_in_swap, flash_offset) => {
                self.swapper_args[0] = self.swapper_state;
                self.swapper_args[1] = 4; // WriteToFlash
                self.swapper_args[2] = vaddr_in_swap;
                self.swapper_args[3] = flash_offset;
            }
            BlockingSwapOp::BulkErase(_pid, _tid, _src_pid, offset, len) => {
                self.swapper_args[0] = self.swapper_state;
                self.swapper_args[1] = 5; // BulkErase
                self.swapper_args[2] = offset;
                self.swapper_args[3] = len;
            }
            BlockingSwapOp::HardOomSyscall(_tid, _pid) => {
                self.swapper_args[0] = self.swapper_state;
                self.swapper_args[1] = 3; // HardOom
            }
        }
        if let Some(op) = self.prev_op.take() {
            if let Some(dop) = self.nested_op {
                println!("ERR: nesting depth of 2 exceeded! {:x?}", dop);
                panic!("Nesting depth of 2 exceeded!");
            }
            println!("Nesting {:x?}", op);
            self.nested_op = Some(op);
            panic!(
                "Nesting should not happen - this code is vestigial but remains to see if this edge case remains"
            );
        }
        // println!("Setting prev_op to {:?}", op);
        self.prev_op = Some(op);
        let swapper_pid: PID = PID::new(xous_kernel::SWAPPER_PID).unwrap();

        // Prepare to enter into the swapper space "as if it were an IRQ"
        // Disable all other IRQs and redirect into userspace
        SystemServices::with_mut(|ss| {
            if !crate::arch::irq::is_handling_irq() {
                ss.make_callback_to(
                    swapper_pid,
                    self.pc as *const usize,
                    crate::services::CallbackType::Swap(self.swapper_args),
                )
                .expect("couldn't switch to handler");
            } else {
                ss.make_callback_to(
                    swapper_pid,
                    self.pc as *const usize,
                    crate::services::CallbackType::SwapInIrq(self.swapper_args),
                )
                .expect("couldn't switch to handler");
            };
        });

        // The current process/TID is now setup, "resume" into the handler
        crate::services::ArchProcess::with_current_mut(|process| {
            crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
        })
        // the call above diverges; the return end of things is inside the IRQ handler, where we
        // conduct business as if we're returning from a syscall.
    }

    /// Cleanup after `blocking_activate_swapper()` - called on return from the divergence at the end
    /// of the previous call.
    ///
    /// Safety: this call must only be invoked in the swapper's memory context
    pub unsafe fn exit_blocking_call(&mut self) -> Result<xous_kernel::Result, xous_kernel::Error> {
        let (pid, tid) = match self.prev_op.take() {
            // Called from any process. Resume as if recovering from a page fault; absorb the rest of the
            // page fault handler code into this routine at the point where it would have diverged.
            Some(BlockingSwapOp::ReadFromSwap(pid, tid, _vaddr_in_pid, vaddr_in_swap, _paddr)) => {
                MemoryManager::with_mut(|mm| {
                    // we are in the swapper's memory space a this point
                    // unmap the page from the swapper
                    crate::arch::mem::unmap_page_inner(mm, vaddr_in_swap)
                        .expect("couldn't unmap page lent to swapper");
                    // the page map into the target space happens after the syscall returns
                });
                (pid, tid)
            }
            Some(BlockingSwapOp::BulkErase(pid, tid, src_pid, offset, len)) => {
                for page in (offset..offset + len).step_by(FLASH_SECTOR_LEN) {
                    // this works because the V:P mapping for the flash memory is 1:1 for the LSBs
                    let flash_vaddr = MMAP_VIRT_BASE + page;
                    // evict_page_inner marks the page as swapped/invalid in src_pid, but also maps the page
                    // into the swapper's address space
                    match crate::arch::mem::evict_page_inner(src_pid, flash_vaddr) {
                        Ok(swap_vaddr) => {
                            // release the page from the swapper's address space
                            MemoryManager::with_mut(|mm| {
                                let paddr = crate::arch::mem::virt_to_phys(swap_vaddr).unwrap() as usize;
                                #[cfg(feature = "debug-swap")]
                                println!("BulkErase releasing flash backing page - paddr {:x}", paddr);
                                // this call unmaps the virtual page from the page table
                                crate::arch::mem::unmap_page_inner(mm, swap_vaddr)
                                    .expect("couldn't unmap page");
                                // This call releases the physical page from the RPT - the pid has to match
                                // that of the original owner. This is the
                                // "pointy end" of the stick; after this call,
                                // the memory is now back into the free pool.
                                mm.release_page_swap(paddr as *mut usize, src_pid)
                                    .expect("couldn't free page that was swapped out");
                            });
                        }
                        Err(xous_kernel::Error::BadAddress) => {
                            #[cfg(feature = "debug-swap")]
                            println!("BulkErase page wasn't mapped {:x}", page);
                            // in this case, it wasn't mapped into memory. We have been returned to the
                            // swapper's memroy space, and we can just move to
                            // checking the next page
                        }
                        _ => {
                            panic!("Unexpected error in BulkErase page free")
                        }
                    }
                }

                // Unhalt IRQs
                self.swap_restore_irq();
                (pid, tid)
            }
            // Called from any process. Clear the dirty bit on the RPT when exiting. No pages are unmapped
            // by this routine, that would be handled by the OOMer, if at all.
            Some(BlockingSwapOp::WriteToFlash(pid, tid, src_pid, _vpage_addr_in_swapper, flash_offset)) => {
                // this works because the V:P mapping for the flash memory is 1:1 for the LSBs
                let flash_vaddr = MMAP_VIRT_BASE + flash_offset;
                // evict_page_inner marks the page as swapped/invalid in src_pid, but also maps the page
                // into the swapper's address space
                match crate::arch::mem::evict_page_inner(src_pid, flash_vaddr) {
                    Ok(swap_vaddr) => {
                        // release the page from the swapper's address space
                        MemoryManager::with_mut(|mm| {
                            let paddr = crate::arch::mem::virt_to_phys(swap_vaddr).unwrap() as usize;
                            #[cfg(feature = "debug-swap-verbose")]
                            println!("Release flash backing page - paddr {:x}", paddr);
                            // this call unmaps the virtual page from the page table
                            crate::arch::mem::unmap_page_inner(mm, swap_vaddr).expect("couldn't unmap page");
                            // This call releases the physical page from the RPT - the pid has to match that
                            // of the original owner. This is the "pointy end" of
                            // the stick; after this call, the memory is now back
                            // into the free pool.
                            mm.release_page_swap(paddr as *mut usize, src_pid)
                                .expect("couldn't free page that was swapped out");
                        });
                    }
                    Err(xous_kernel::Error::BadAddress) => {
                        #[cfg(feature = "debug-swap")]
                        println!("Written page wasn't mapped {:x}", flash_vaddr);
                        // in this case, it wasn't mapped into memory. We have been returned to the
                        // swapper's memory space, and we can just move to
                        // checking the next page
                    }
                    _ => {
                        panic!("Unexpected error in WriteToFlash page free")
                    }
                }

                // Unhalt IRQs
                self.swap_restore_irq();
                (pid, tid)
            }
            Some(BlockingSwapOp::HardOomSyscall(tid, pid)) => {
                // We enter this from the swapper's memory space
                assert!(
                    crate::arch::process::current_pid().get() == SWAPPER_PID,
                    "Hard OOM syscall did not return from swapper's space"
                );
                // unmap RPT from swapper's userspace, if it was previously unmapped
                if self.unmap_rpt_after_hard_oom {
                    MemoryManager::with_mut(|mm| {
                        for page in 0..self.mem_alloc_tracker_pages {
                            crate::arch::mem::unmap_page_inner(mm, SWAP_RPT_VADDR + page * PAGE_SIZE).ok();
                        }
                    });
                }
                (pid, tid)
            }
            None => panic!("No previous swap op was set"),
        };

        // return to the original pid memory space, now that we have memory
        SystemServices::with_mut(|system_services| {
            // Cleanup the swapper
            system_services.finish_swap();

            if !crate::arch::irq::is_handling_irq() {
                system_services.swap_resume_to_userspace(pid, tid).expect("couldn't swap_resume");
            }

            // Switch to target process
            let process = system_services.get_process_mut(pid).unwrap();
            process.mapping.activate().unwrap();
            process.activate().unwrap();
            // Activate the current context
            crate::arch::process::Process::current().set_tid(tid).unwrap();
            process.current_thread = tid;
        });

        // return to kernel space -- these calls can only be originated from kernel space
        unsafe { riscv::register::sstatus::set_spp(riscv::register::sstatus::SPP::Supervisor) };

        if let Some(op) = self.nested_op.take() {
            println!("Popping nested_op into prev_op with {:?}", op);
            self.prev_op = Some(op);
        }
        Ok(xous_kernel::Result::Scalar5(0, 0, 0, 0, 0))
    }
}

// Various in-line assembly thunks go below this line.
fn sim_read() -> usize {
    let existing: usize;
    unsafe { core::arch::asm!("csrrs {0}, 0x9C0, zero", out(reg) existing) };
    existing
}

fn sim_write(new: usize) { unsafe { core::arch::asm!("csrrw zero, 0x9C0, {0}", in(reg) new) }; }
