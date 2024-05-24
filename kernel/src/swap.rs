// SPDX-FileCopyrightText: 2024 bunnie <bunnie@kosagi.com>
// SPDX-License-Identifier: Apache-2.0

use core::cmp::Ordering;

use loader::swap::SWAP_FLG_WIRED;
use loader::swap::SWAP_RPT_VADDR;
use xous_kernel::SWAPPER_PID;
use xous_kernel::{SysCallResult, PID, SID, TID};

use crate::arch::current_pid;
use crate::arch::mem::flush_mmu;
use crate::arch::mem::MMUFlags;
use crate::arch::mem::EXCEPTION_STACK_TOP;
use crate::arch::mem::PAGE_SIZE;
use crate::mem::MemoryManager;
use crate::services::SystemServices;

/// userspace swapper -> kernel ABI (see kernel/src/syscall.rs)
/// This ABI is copy-paste synchronized with what's in the userspace handler. It's left out of
/// xous-rs so that we can change it without having to push crates to crates.io.
/// Since there is only one place the ABI could be used, we're going to stick with
/// this primitive method of synchronization because it reduces the activation barrier
/// to fix bugs.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SwapAbi {
    Invalid = 0,
    Evict = 1,
    GetFreePages = 2,
    FetchAllocs = 3,
    HardOom = 4,
    StealPage = 5,
    ReleaseMemory = 6,
}
/// SYNC WITH `xous-swapper/src/main.rs`
impl SwapAbi {
    pub fn from(val: usize) -> SwapAbi {
        use SwapAbi::*;
        match val {
            1 => Evict,
            2 => GetFreePages,
            3 => FetchAllocs,
            4 => HardOom,
            5 => StealPage,
            6 => ReleaseMemory,
            _ => Invalid,
        }
    }
}

/// kernel -> swapper handler ABI
/// This is the response ABI from the kernel into the swapper. It also tracks intermediate state
/// necessary for proper return from these calls.
#[derive(Debug, Copy, Clone)]
pub enum BlockingSwapOp {
    /// TID, PID of source, vaddr of source, vaddr in swap space (block must already be
    /// mapped into swap space). Returns to Swapper, as this can only originate from the Swapper.
    WriteToSwap(TID, PID, usize, usize),
    /// PID of the target block, current paddr of block, original vaddr in the space of target block PID,
    /// physical address of block. Returns to PID of the target block.
    ReadFromSwap(PID, TID, usize, usize, usize),
    /// Argument is the original thread ID of swapper. Returns to the swapper, as it can only
    /// originate from the Swapper. This is used by the swapper when OomDoom is imminent; basically,
    /// the userspace version of HardOom. This call allows progress in other processes while the
    /// swapper does its thing.
    FetchAllocs(TID),
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
                    #[cfg(feature = "debug-swap")]
                    println!(
                        "-- update {}/{:x} <- {}/{:x} @ {:x} (in pid{})",
                        pid.get(),
                        va,
                        va as u8,
                        va & !0xfff,
                        ((self as *const Self as usize
                            - crate::mem::MemoryManager::with(|mm| mm.rpt_base()))
                            / core::mem::size_of::<SwapAlloc>())
                            * PAGE_SIZE,
                        crate::arch::process::Process::with_current(|p| p.pid().get()),
                    );
                    // preserve the wired flag if it was set previously by the loader
                    self.vpn = (va as u32) & !0xFFF | pid.get() as u32 | self.vpn & SWAP_FLG_WIRED;
                } else {
                    self.vpn = SWAP_FLG_WIRED | pid.get() as u32;
                }
            } else {
                #[cfg(feature = "debug-swap")]
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
    sid: SID::from_u32(0, 0, 0, 0),
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
    unmap_rpt_after_hard_oom: false,
    oom_thread_backing: None,
    oom_stack_backing: [0usize; 512],
};

pub struct Swap {
    /// SID for the swapper
    sid: SID,
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
    /// Keep track of if we should unmap the RPT after a hard OOM. In the case that we hard-OOM while the
    /// soft-OOM handler is running, the RPT will already be mapped into the swapper's space. This is not
    /// uncommon, because the soft-OOM handler is interruptable and can only keep up with gradual memory
    /// demand.
    unmap_rpt_after_hard_oom: bool,
    /// backing for the thread state that is smashed by the OOM handler
    oom_thread_backing: Option<crate::arch::process::Thread>,
    oom_stack_backing: [usize; 512],
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
        s0: u32,
        s1: u32,
        s2: u32,
        s3: u32,
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
        if self.sid == SID::from_u32(0, 0, 0, 0) {
            self.sid = SID::from_u32(s0, s1, s2, s3);
            self.pc = handler;
            self.swapper_state = state;
            #[cfg(feature = "debug-swap")]
            println!(
                "handler registered: sid {:?} pc {:?} state {:?}",
                self.sid, self.pc, self.swapper_state
            );
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
        #[cfg(feature = "debug-swap")]
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
        // disable all IRQs; no context swapping is allowed
        self.oom_irq_backing = Some(sim_read());
        #[cfg(feature = "debug-swap")]
        println!("Hard OOM syscall stored SIM: {:x?}", self.oom_irq_backing);
        sim_write(0x0);

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
                        unsafe { crate::arch::mem::flush_mmu() };
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

    pub fn hard_oom_inline(&mut self) -> bool {
        // we are in the pid/tid of the invoking process
        // stash the thread state of the current pid/tid because the syscall return trampoline will smash it
        self.oom_thread_backing =
            Some(crate::arch::process::Process::with_current(|p| p.current_thread().clone()));

        println!("Entering inline hard OOM handler");
        crate::arch::process::Process::with_current(|p| {
            println!(
                "BEF HARD OOM HANDLER {}.{} sepc: {:x} sstatus: {:x?} satp: {:x?} reg {:08x?}",
                p.pid().get(),
                p.current_tid(),
                riscv::register::sepc::read(),
                riscv::register::sstatus::read().spp(),
                riscv::register::satp::read(),
                p.current_thread().registers,
            )
        });
        // assemble this upstream of the stack save, because this affects stack
        let call_precompute = xous_kernel::SysCall::SwapOp(SwapAbi::HardOom as usize, 0, 0, 0, 0, 0, 0);

        // at this point we are in user pid/tid, but supervisor mode. The current thread backing is
        // about to be smashed by the syscall invocation. we want to return to this backing.
        let mut current_sp: usize;

        unsafe {
            core::arch::asm!(
                "mv {current_sp}, sp",
                current_sp = out(reg) current_sp,
            )
        }
        println!(" --> SP extent d'{} bytes", EXCEPTION_STACK_TOP - current_sp);
        let backup_stack_ptr: usize = self.oom_stack_backing.as_ptr() as usize;
        let working_stack_end: usize = EXCEPTION_STACK_TOP;
        let working_stack_ptr: usize =
            EXCEPTION_STACK_TOP - self.oom_stack_backing.len() * core::mem::size_of::<usize>();
        unsafe {
            core::arch::asm!(
                "mv   a7, {working_stack_end}",
                "mv   a6, {working_stack_ptr}",
                "mv   a5, {backup_stack_ptr}",
            "100:",
                "lw   a4, 0(a6)",
                "sw   a4, 0(a5)",
                "addi  a6, a6, 4",
                "addi  a5, a5, 4",
                "bltu  a6, a7, 100b",
                "mv    {current_sp}, sp",

                working_stack_end = in(reg) working_stack_end,
                working_stack_ptr = in(reg) working_stack_ptr,
                backup_stack_ptr = in(reg) backup_stack_ptr,
                current_sp = out(reg) current_sp,
            )
        }
        assert!(
            EXCEPTION_STACK_TOP - current_sp <= self.oom_stack_backing.len() * core::mem::size_of::<usize>(),
            "Backing stack not large enough to handle current kernel stack utilization."
        );
        // invoke this as a nested syscall so it returns here
        xous_kernel::rsyscall(call_precompute).ok();

        let restore_stack_ptr: usize = self.oom_stack_backing.as_ptr() as usize;
        let working_restore_end: usize = EXCEPTION_STACK_TOP;
        let working_restore_ptr: usize =
            EXCEPTION_STACK_TOP - self.oom_stack_backing.len() * core::mem::size_of::<usize>();
        unsafe {
            core::arch::asm!(
                "mv   a7, {working_restore_end}",
                "mv   a6, {working_restore_ptr}",
                "mv   a5, {restore_stack_ptr}",
            "100:",
                "lw   a4, 0(a5)",
                "sw   a4, 0(a6)",
                "addi  a6, a6, 4",
                "addi  a5, a5, 4",
                "bltu  a6, a7, 100b",

                working_restore_end = in(reg) working_restore_end,
                working_restore_ptr = in(reg) working_restore_ptr,
                restore_stack_ptr = in(reg) restore_stack_ptr,
            )
        }

        crate::arch::process::Process::with_current(|p| {
            println!(
                "AFT HARD OOM HANDLER {}.{} sepc: {:x} sstatus: {:x?} satp: {:x?} reg {:08x?}",
                p.pid().get(),
                p.current_tid(),
                riscv::register::sepc::read(),
                riscv::register::sstatus::read().spp(),
                riscv::register::satp::read(),
                p.current_thread().registers,
            )
        });

        // then restore the thread state on return from the syscall
        crate::arch::process::Process::with_current_mut(|p| {
            println!("Returned from hard OOM handler, current pid {}/tid {}", p.pid().get(), p.current_tid());
            *p.current_thread_mut() =
                self.oom_thread_backing.take().expect("No thread backing was set prior to OOM handler")
        });

        true
    }

    /// This call diverges into the userspace swapper.
    /// Divergent calls must turn of IRQs before memory spaces are changed.
    pub fn fetch_allocs(&mut self) -> ! {
        crate::arch::irq::disable_all_irqs();
        let tid = crate::arch::process::Process::with_current(|p| p.current_tid());

        SystemServices::with(|system_services| {
            let swapper_pid = PID::new(xous_kernel::SWAPPER_PID).unwrap();
            // swap to the swapper space
            let swapper_map = system_services.get_process(swapper_pid).unwrap().mapping;
            swapper_map.activate().unwrap();

            // map the RPT into userspace
            // note that this technically makes the RPT shared with kernel and userspace, but,
            // the userspace version is read-only, and the next section runs in an interrupt context,
            // so I think it's safe for it to be shared for that duration.
            MemoryManager::with_mut(|mm| {
                for page in 0..self.mem_alloc_tracker_pages {
                    crate::arch::mem::map_page_inner(
                        mm,
                        swapper_pid,
                        self.mem_alloc_tracker_paddr + page * PAGE_SIZE,
                        SWAP_RPT_VADDR + page * PAGE_SIZE,
                        xous_kernel::MemoryFlags::R,
                        true,
                    )
                    .ok();
                    unsafe { crate::arch::mem::flush_mmu() };
                }
            });
        });

        #[cfg(feature = "debug-swap")]
        println!("fetch_allocs - userspace activate");
        // this is safe because we entered the swapper memory context above
        unsafe {
            self.blocking_activate_swapper(BlockingSwapOp::FetchAllocs(tid));
        }
    }

    /// This call normally diverges into the userspace swapper. It will return a SysCallResult
    /// if the requested pages fail sanity checks.
    /// Divergent calls must turn of IRQs before memory spaces are changed.
    pub fn evict_page(&mut self, target_pid: usize, vaddr: usize) -> SysCallResult {
        crate::arch::irq::disable_all_irqs();
        // This call can fail for legitimate reasons: for example, the address given was already swapped, or
        // invalid.
        let evicted_ptr =
            match crate::arch::mem::evict_page_inner(PID::new(target_pid as u8).expect("Invalid PID"), vaddr)
            {
                Ok(ptr) => ptr,
                Err(e) => {
                    #[cfg(feature = "debug-swap")]
                    println!("evict_page rejecting request for pid{}/{:x}: {:?}", target_pid, vaddr, e);
                    // evict_page_inner guarantees we are in the swapper PID even on an error edge case
                    crate::arch::irq::enable_all_irqs();
                    return Err(e);
                }
            };

        // remember the TID of the swapper we're returning to
        let tid = crate::arch::process::Process::with_current(|p| p.current_tid());
        #[cfg(feature = "debug-swap")]
        println!("evict_page from pid{}/{:x} - swapper activate", target_pid, vaddr);
        // this is safe because evict_page() leaves us in the swapper memory context
        unsafe {
            self.blocking_activate_swapper(BlockingSwapOp::WriteToSwap(
                tid,
                PID::new(target_pid as u8).expect("Invalid PID"),
                vaddr,
                evicted_ptr,
            ));
        }
    }

    /// The address space on entry to `retrieve_page` is `target_pid`; it must ensure
    /// that the address space is still `target_pid` on return.
    ///
    /// Also takes as argument the virtual address of the target page in the target PID,
    /// as well as the physical address of the page.
    ///
    /// Note that the originating caller to this will update the epoch of the memory
    /// alloc counter, so, we don't have to update it explicitly her (as we do with
    /// evict_page)
    ///
    /// This call diverges into the userspace swapper.
    /// Divergent calls must turn of IRQs before memory spaces are changed.
    pub fn retrieve_page(
        &mut self,
        target_pid: PID,
        target_tid: TID,
        target_vaddr_in_pid: usize,
        paddr: usize,
    ) -> ! {
        if crate::arch::irq::is_handling_irq() {
            #[cfg(feature = "debug-swap")]
            crate::arch::process::Process::with_current(|process| {
                println!("IRQ ENTRY: pid{}, {:x?}", current_pid().get(), process.current_thread());
            });
            #[cfg(feature = "debug-swap")]
            println!(
                "sstatus {:x?} spp: {:?}",
                riscv::register::sstatus::read(),
                riscv::register::sstatus::read().spp()
            );
            // crate::arch::mem::MemoryMapping::current().print_map();
        } else {
            crate::arch::irq::disable_all_irqs();
        }

        let block_vaddr_in_swap =
            crate::arch::mem::map_page_to_swapper(paddr).expect("couldn't map target page to swapper");
        // we are now in the swapper's memory space

        #[cfg(feature = "debug-swap")]
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

    /// Safety:
    ///   - the current page table mapping context must be PID 2 (the swapper's PID) for this to work
    ///   - interrupts must have been disabled prior to setting the context to PID 2
    /// `op` contains the opcode data
    /// `payload_ptr` is the pointer to the virtual address of the swapped block in PID2 space
    unsafe fn blocking_activate_swapper(&mut self, op: BlockingSwapOp) -> ! {
        // setup the argument block
        match op {
            // Note to self: get opcode number from `KernelOp` structure in services/xous-swapper/main.rs
            BlockingSwapOp::WriteToSwap(_tid, pid, vaddr_in_pid, vaddr_in_swap) => {
                self.swapper_args[0] = self.swapper_state;
                self.swapper_args[1] = 0; // WriteToSwap
                self.swapper_args[2] = pid.get() as usize;
                self.swapper_args[3] = vaddr_in_pid;
                self.swapper_args[4] = vaddr_in_swap;
            }
            BlockingSwapOp::ReadFromSwap(pid, _tid, vaddr_in_pid, vaddr_in_swap, _paddr) => {
                self.swapper_args[0] = self.swapper_state;
                self.swapper_args[1] = 1; // ReadFromSwap
                self.swapper_args[2] = pid.get() as usize;
                self.swapper_args[3] = vaddr_in_pid;
                self.swapper_args[4] = vaddr_in_swap;
            }
            BlockingSwapOp::FetchAllocs(_tid) => {
                self.swapper_args[0] = self.swapper_state;
                self.swapper_args[1] = 2; // ExecFetchAllocs
                self.swapper_args[2] = self.mem_alloc_tracker_pages;
            }
            BlockingSwapOp::HardOomSyscall(_tid, _pid) => {
                self.swapper_args[0] = self.swapper_state;
                self.swapper_args[1] = 3; // HardOom
            }
        }
        if let Some(op) = self.prev_op.take() {
            if let Some(dop) = self.nested_op {
                println!("ERR: nesting depth of 2 exceeded! {:?}", dop);
                panic!("Nesting depth of 2 exceeded!");
            }
            // println!("Nesting {:?}", op);
            self.nested_op = Some(op);
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
        let result = match self.prev_op.take() {
            // Called from any process. Resume as if recovering from a page fault; absorb the rest of the
            // page fault handler code into this routine at the point where it would have
            // divereged.
            Some(BlockingSwapOp::ReadFromSwap(pid, tid, vaddr_in_pid, vaddr_in_swap, paddr)) => {
                MemoryManager::with_mut(|mm| {
                    // we are in the swapper's memory space a this point
                    // unmap the page from the swapper
                    crate::arch::mem::unmap_page_inner(mm, vaddr_in_swap)?;

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

                    // Finish up the page table manipulations that were aborted by the original swap call
                    let entry = crate::arch::mem::pagetable_entry(vaddr_in_pid)
                        .or(Err(xous_kernel::Error::BadAddress))?;
                    let current_entry = entry.read_volatile();
                    // clear the swapped flag
                    let flags = current_entry & 0x3ff & !MMUFlags::P.bits();
                    let ppn1 = (paddr >> 22) & ((1 << 12) - 1);
                    let ppn0 = (paddr >> 12) & ((1 << 10) - 1);
                    // Map the retrieved page to the target memory space, and set valid. We rely
                    // on the fact that the USER bit, etc. was correctly setup and stored when the
                    // page was originally allocated.
                    *entry = (ppn1 << 20) | (ppn0 << 10) | (flags | crate::arch::mem::FLG_VALID);
                    flush_mmu();
                    flush_dcache();

                    // There is an edge case where we are exiting the OOM handler through the read
                    // from swap routine. If the backing is set, we took this path; we must copy
                    // the backing to the system backing to respect the assumptions of this epilogue.
                    if let Some(sim) = self.oom_irq_backing.take() {
                        #[cfg(feature = "debug-swap")]
                        println!("OOM->RFS restoring IRQ: {:x}", sim);
                        crate::arch::irq::set_sim_backing(sim);
                    }

                    if !crate::arch::irq::is_handling_irq() {
                        #[cfg(feature = "debug-swap")]
                        println!(
                            "RFS - handing page va {:x} -> pa {:x} to pid{}/hwpid{} tid{} entry {:x}",
                            vaddr_in_pid,
                            paddr,
                            pid.get(),
                            crate::arch::process::Process::with_current(|p| p.pid().get()),
                            tid,
                            *entry
                        );
                        // There is an edge case where we are exiting the OOM handler through the read
                        // from swap routine. If the backing is set, we took this path; we must restore
                        // interrupts now, or else we lose pre-emption forever.
                        if let Some(sim) = self.oom_irq_backing.take() {
                            #[cfg(feature = "debug-swap")]
                            println!("OOM->RFS restoring IRQ: {:x}", sim);
                            sim_write(sim);
                        }
                        Ok(xous_kernel::Result::ResumeProcess)
                    } else {
                        // Don't use the resume provided by the wrapper -- instead resume directly here,
                        // as we don't want to re-enable IRQs
                        crate::arch::process::Process::with_current(|process| {
                            #[cfg(feature = "debug-swap")]
                            println!(
                                "Swapper returning from page fault in IRQ: pid{}/tid{}-{:x?}",
                                pid.get(),
                                tid,
                                process.current_thread()
                            );
                            flush_mmu();
                            flush_dcache();
                            crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
                        })
                    }
                })
            }
            // Called only from PID2. Resume to the original TID within PID2, as a syscall return.
            Some(BlockingSwapOp::WriteToSwap(tid, pid, _addr, swapper_virt_page)) => {
                // Update the RPT: mark the physical memory as free. The virtual address is
                // in the swapper's context at this point, so free it there (it's already been
                // remapped as swapped in the target's context). Note that the swapped page now contains
                // encrypted data, as the encryption happens in-place.
                MemoryManager::with_mut(|mm| {
                    let paddr = crate::arch::mem::virt_to_phys(swapper_virt_page).unwrap() as usize;
                    #[cfg(feature = "debug-swap")]
                    println!("WTS releasing paddr {:x}", paddr);
                    // this call unmaps the virtual page from the page table
                    crate::arch::mem::unmap_page_inner(mm, swapper_virt_page).expect("couldn't unmap page");
                    // This call releases the physical page from the RPT - the pid has to match that of the
                    // original owner. This is the "pointy end" of the stick; after this call, the memory is
                    // now back into the free pool.
                    mm.release_page_swap(paddr as *mut usize, pid)
                        .expect("couldn't free page that was swapped out");
                    // clear the caches
                    flush_dcache();
                });

                // Switch back to the Swapper's userland thread
                SystemServices::with_mut(|ss| {
                    ss.finish_callback_and_resume(PID::new(SWAPPER_PID).unwrap(), tid)
                        .expect("Unable to resume to swapper");
                });
                // return as a SysCall
                Ok(xous_kernel::Result::Scalar5(0, 0, 0, 0, 0))
            }
            // Called only from PID2. Resume to the original TID within PID2, as a syscall return.
            Some(BlockingSwapOp::FetchAllocs(tid)) => {
                // unmap RPT from userspace
                MemoryManager::with_mut(|mm| {
                    for page in 0..self.mem_alloc_tracker_pages {
                        crate::arch::mem::unmap_page_inner(mm, SWAP_RPT_VADDR + page * PAGE_SIZE).ok();
                    }
                });
                // Switch back to the Swapper's userland thread
                SystemServices::with_mut(|ss| {
                    ss.finish_callback_and_resume(PID::new(SWAPPER_PID).unwrap(), tid)
                        .expect("Unable to resume to swapper");
                });
                // return as a SysCall
                Ok(xous_kernel::Result::Scalar5(0, 0, 0, 0, 0))
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

                // return to kernel space -- this call can only be originated from kernel space
                unsafe { riscv::register::sstatus::set_spp(riscv::register::sstatus::SPP::Supervisor) };

                // restore IRQ state (don't borrow the system handler's state tracker, so we don't smash
                // it by accident)
                let sim = self.oom_irq_backing.take().expect("Someone stole our IRQ backing!");
                #[cfg(feature = "debug-swap")]
                println!("OOM syscall restoring IRQ: {:x}", sim);
                sim_write(sim);

                Ok(xous_kernel::Result::Scalar5(0, 0, 0, 0, 0))
            }
            None => panic!("No previous swap op was set"),
        };
        if let Some(op) = self.nested_op.take() {
            // println!("Popping nested_op into prev_op with {:?}", op);
            self.prev_op = Some(op);
        }
        result
    }
}

// Various in-line assembly thunks go below this line.

#[inline]
fn flush_dcache() {
    unsafe {
        #[rustfmt::skip]
        core::arch::asm!(
            ".word 0x500F",
            "nop",
            "nop",
            "nop",
            "nop",
            "fence",
            "nop",
            "nop",
            "nop",
            "nop",
        );
    }
}

fn sim_read() -> usize {
    let existing: usize;
    unsafe { core::arch::asm!("csrrs {0}, 0x9C0, zero", out(reg) existing) };
    existing
}

fn sim_write(new: usize) { unsafe { core::arch::asm!("csrrw zero, 0x9C0, {0}", in(reg) new) }; }
