//! ==Architecture==
//!
//! The Xous philosophy is to leave the kernel lightweight and free of dependencies. The swap implementation
//! adheres to this by trying to move as much of the difficult algorithmic processing and performance tuning
//! outside of the kernel.
//!
//! Incidentally, the one thing swap does introduce to the kernel that is algorithm-y
//! is a renormalization routine for counting page accesses. We track page access frequency with a 32-bit
//! "epoch" counter, which is simply incremented whenever a page table interaction happens. We don't use
//! a 64-bit counter because greatly increases the memory used to track things due to the single 64-bit
//! record forcing the next item to also have 64-bit alignment, thus effectively wasting several bytes
//! per page. Anyways, when the epoch is about to roll-over, a mostly in-place sweep with no allocations
//! beyond a few dozen bytes in stack is done to the memory usage tracker to "compact" the epoch numbers down.
//! There is a #[test] in the kernel crate for this routine.
//!
//! In order to perform all the other processing outside of the kernel, the swapper introduces a special new
//! "blocking userspace handler". It's "IRQ-like", in that it borrows the same mechanism used for blocking
//! IRQ handlers, but with different entry and exit magic numbers so we can differentiate the two. Anyways,
//! the blocking userspace handler happens with interrupts disabled, giving it an atomic view of all of
//! memory for the duration of the handler.
//!
//! Time spent in this handler of course reduces the responsiveness of the system, so in order to minimize
//! that, we prefer to use the handler to do the minimal processing it has to in the atomic context, and then
//! fire a message off to a normal, preemptable userspace process to do all the fancy handling. As a result,
//! we have two types of OOM handlers.
//!
//! ==OOM Types==
//!
//! There are two types of OOMs handled by the swapper:
//!   - "OOM Doom": impending OOM. This is detected by this userspace process, which allows for more
//!     sophisticated processing and algorithms to predict an impending OOM than we can fit in the kernel. The
//!     OOM Doom handler is also interruptable, which allows other progresses to make progress while memory is
//!     being cleared up, hopefully allowing for a more graceful failure curve. The only processing done in
//!     the blocking handler is a copy of the kernel memory view, and then a message is issued to the
//!     preemptable userspace manager to deal with the rest.
//!   - "Hard OOM": Literally no more pages are available. This is triggered when the page allocator can't map
//!     a single more page. The hard-OOM handler turns off all interrupts, and all the OOM processing happens
//!     within the blocking userspace handler. This effectively turns the hard-OOM handler "inside out"
//!     compared to the OOM Doom handler. In order to support this, a set of calls that implement the kernel
//!     space pre-amble and post-amble (i.e. `StealPage` and `ReleaseMemory`) are introduced.
//!
//! ==Some Edge Cases==
//!
//! OOM Doom maybe interrupted by the Hard OOM handler (and in fact, this expected because a very greedy
//! process will likely trip a Hard OOM before the OOM Doom is avoided). In order to handle this, separate
//! RPT copy allocations are made for both handlers. This doubles the amount of memory we have to reserve
//! for OOM processing, so if it turns out the OOM Doom is never fast enough to avoid a Hard OOM, we should
//! consider removing it. We also detect when Hard-OOM happens during OOM Doom, and abort OOM Doom processing
//! because our view of the kernel memory maps are stale and invalid.
//!
//! There is an extra edge case in the exit path of the Hard-OOM, namely, if the hard-OOM failed on a page
//! that was swapped. Here, after clearing memory, we have an empty page, but no data; we have to again
//! re-enter the blocking userspace handler to fill that page. The twist here is we have to remember to
//! re-instate interrupts on this edge case otherwise we lose preemption. This is all handled in the kernel
//! proper.
//!
//! == Measuring Memory Usage ==
//!
//! The swapper needs to come up with an answer for which page to swap out, and it
//! also needs to know when to do it (OOM pressure).
//!
//! OOM pressure is handled with a syscall to the kernel to query the current `MEMORY_ALLOCATIONS`
//! table and return the available RAM. This is queried periodically with a timer, and if we
//! fall below a certain threshold, the swapper will try to suggest pages to the kernel to Evict.
//!
//! When `swap` is active, `MEMORY_ALLOCATIONS` is upgraded to be a table of
//! `timestamp | VPN | PID | FLAGS`, where the timestamp is a u32 that is monotonically
//! incremented with every modification to the page, and the VPN | PID | FLAGS portion
//! is condensed to fit into a u32. The FLAGS can specify if the address is `wired`, which
//! would be the case of e.g. a page table page, and the VPN would be considered invalid
//! in this case (and the page should never be swapped).
//!
//! A u32 is used instead of a u64 because due to alignment issues, if we used a u64 we'd
//! waste 4 bytes per tracking slot, and the penalty is not worth it in a memory-constrained
//! system. Instead, we have a callback to handle when the "epoch" rolls over.
//!
//! The `MEMORY_ALLOCATIONS` table is page-aligned, so that it can be mapped into PID 2 inside
//! an interrupt context. When the free memory level hits the low water mark, PID 2 invokes
//! a request to the kernel to call the swapper interrupt context with `MEMORY_ALLOCATIONS`
//! mapped into its memory space. At this point, PID 2 will copy the current `MEMORY_ALLOCATIONS`
//! table into a pre-allocated BinaryHeap in the shared state structure, indexed by the timestamp.
//! On completion of the interrupt context, a message is sent back into the swapper userspace
//! using `try_send_message` to trigger the Evict operation. At this point, the Evict operation
//! can work through a sorted vector of allocations to pick the pages it wants to remove.
//!
//! This trades off making the Evict computation a bit more expensive for making all other
//! allocation bookkepping a bit cheaper, because we no longer need to send AllocAdvice messages
//! to the userspace swapper. However, keep in mind that a 2MiB main memory means we have
//! a `MEMORY_ALLOCATIONS` table with only 512 entries, so processing a binary heap of this
//! should have a cost less than decrypting a single page of memory. By ensuring that every
//! Evict pushes out at least 10 or so pages, we can amortize the cost of computing the heap.
//! We also have the advantage that because we have an absolute sorted list of all allocations,
//! we can keep trying evictions from oldest entry to newest entry until we have the correct
//! number of *successes* -- meaning the kernel can override any requested Evictions without
//! impacting the amount of memory that ultimately gets freed up.
//!
//! One thing to note is that the Eviction process should be kicked off when there is sufficient
//! free memory available to complete the BinaryHeap generation + sorted Vec output. One option
//! is to pre-allocate those quantities, potentially with Heapless.
//!
//! A final feature that should be introduced is the kernel needs to have an API call into the
//! swapper, so that it can invoke a chain of events that clears out memory in case it sees
//! the OOM condition. This is done by having the kernel enter the swapper via the interrupt
//! context, and the swapper then issuing the a message that the userspace then handles.

// TODO:
//  - [done] refactor loader to have a swap allocation tracker (MSB of `count` table)
//    - move the alloc earlier in the boot process; update page map at the end
//    - have phase 1 mark the table
//  - [done] implement the WriteToSwap routine - look for the next free page, increment offset counter, write
//    memory to swap
//  - [done] refactor memory allocation tracking
//  - [done] add kernel OOM callback
//  - [done] add test command to force eviction of some variable number of pages
//  - [done] handle epoch rollovers
//  - [done] hook the kernel OOM callback
//  - [done] Implement Evictor routine with a thing that checks free memory level, and then more intelligently
//    swaps stuff out to create free space for the kernel
//  - [done] create test program that over provisions heap versus available memory and prove that swap really
//    works.
//  - tune the OOM handlers

mod debug;
mod platform;
use core::fmt::Write;
use std::collections::{BinaryHeap, VecDeque};
use std::fmt::Debug;
use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc};
use std::thread;
use std::thread::sleep;
use std::time::Duration;

use debug::*;
use loader::swap::{SwapAlloc, SwapSpec, SWAP_CFG_VADDR, SWAP_COUNT_VADDR, SWAP_PT_VADDR, SWAP_RPT_VADDR};
use num_traits::*;
use platform::{SwapHal, PAGE_SIZE};
use xous::{MemoryFlags, MemoryRange, Message, Result, CID, PID, SID};

/// Threshold for OomDoom callback
const OOM_THRESH_PAGES: usize = 16;
/// Target of free pages we want to get to after OomDoom call
const FREE_PAGE_TARGET: usize = 32;
/// Target of pages to free in case of a Hard OOM
const HARD_OOM_PAGE_TARGET: usize = 48;
/// Virtual address prefixes to de-prioritize in the OomDoom sweep
///   0 - text region
///   4 - message region
const KEEP_VADDR_PREFIXES: [u8; 2] = [0u8, 4u8];

/// userspace swapper -> kernel ABI
/// This ABI is copy-paste synchronized with what's in the kernel. It's left out of
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
    // SetOomThresh = 4,
    StealPage = 5,
    ReleaseMemory = 6,
}
/// SYNC WITH `kernel/src/swap.rs`
impl SwapAbi {
    pub fn from(val: usize) -> SwapAbi {
        use SwapAbi::*;
        match val {
            1 => Evict,
            2 => GetFreePages,
            3 => FetchAllocs,
            // 4 => SetOomThresh,
            5 => StealPage,
            6 => ReleaseMemory,
            _ => Invalid,
        }
    }
}

/// kernel -> swapper handler ABI
/// This structure mirrors the BlockingSwapOp's that the kernel can issue to userspace.
/// The actual numbers for the opcode are transcribed manually into the kernel, as the
/// kernel's encoding of its enum is composite to track call state.
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
#[repr(usize)]
pub enum KernelOp {
    /// Take the page lent to us, encrypt it and write it to swap
    WriteToSwap = 0,
    /// Find the requested page, decrypt it, and return it
    ReadFromSwap = 1,
    /// Kernel message advising us that a page of RAM was allocated
    ExecFetchAllocs = 2,
    /// Hard OOM invocation - stop everything and free memory!
    HardOom = 3,
}

/// public userspace & swapper handler -> swapper userspace ABI
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
#[repr(usize)]
pub enum Opcode {
    /// Trigger back to userspace to indicate that alloc fetching is done.
    FetchAllocsDone,
    /// Trigger the OOM routine
    HandleOomDoom,
    /// Test messages
    #[cfg(feature = "swap-userspace-testing")]
    Test0,
}

pub struct PtPage {
    pub entries: [u32; 1024],
}

/// An array of pointers to the root page tables of all the processes.
pub struct SwapPageTables {
    pub roots: &'static mut [PtPage],
}

pub struct RuntimePageTracker {
    pub allocs: &'static mut [Option<PID>],
}

pub struct SwapCountTracker {
    pub counts: &'static mut [u32],
}

/// Track if the hard OOM handler ram during the Oom Doom userspace routine
static MAYBE_HARD_OOM_DURING_OOM_DOOM: AtomicBool = AtomicBool::new(false);

/// Number of pages to reserve for hard OOM handling. In case of a hard OOM, there are 0 pages
/// available, which makes it impossible for the hard OOM handler to do things like allocate an L1
/// page table entry to track memory being swapped out. This places a hold on some memory that's
/// de-allocated on entry to the hard OOM handler, and re-allocated on exit.
///
/// Known things the swapper has to allocate memory for in hard-OOM:
///   - L1 page table entries for tracking swap
///   - A second page will be requested in case of panic in hard OOM for TLS, but these shouldn't happen so we
///     don't reserve it.
const HARD_OOM_RESERVED_PAGES: usize = 1;

/// This structure contains shared state accessible between the userspace code and the blocking swap call
/// handler.
pub struct SwapperSharedState {
    /// Mapping of (PID, virtual address) -> (physical offset in swap), organized as a table of page tables
    /// indexed by PID
    pub pts: SwapPageTables,
    /// Contains all the structures specific to the HAL for accessing swap memory
    pub hal: SwapHal,
    /// This is a table of `u32` per page of swap memory, which tracks the count of how many times
    /// the swap page has been used with a 31-bit count, and the remaining 1 MSB dedicated to tracking
    /// if the page is currently used at all. The purpose of this count is to drive nonces up in a
    /// deterministic factor to deter page-reuse attacks.
    pub sct: SwapCountTracker,
    /// Address of main RAM start
    pub sram_start: usize,
    /// Size of main RAM in bytes
    pub sram_size: usize,
    /// Starting point for a search for free swap pages. A simple linear ascending search is done,
    /// starting from the free swap search origin. The unit of this variable is in pages, so it
    /// can be used to directly index the `sct` `SwapCountTracker`.
    pub free_swap_search_origin: usize,
    /// alloc heap for the OomDoom handler
    pub alloc_heap: Option<BinaryHeap<SwapAlloc>>,
    /// alloc heap for the Hard OOM handler. Must be separate because it's possible for both
    /// to run at the same time.
    pub hard_oom_alloc_heap: Option<BinaryHeap<SwapAlloc>>,
    /// Reserve some memory to be freed by the hard OOM manager. These pages are needed to do things
    /// like create L1 page table entries for the swapper to track evicted pages.
    pub hard_oom_reserved_page: Option<MemoryRange>,
    pub report_full_rpt: bool,
}
impl SwapperSharedState {
    pub fn pt_walk(&self, pid: u8, va: usize, mark_free: bool) -> Option<usize> {
        let l1_pt = &self.pts.roots[pid as usize - 1];
        // mask out bottom 10 bits of flags, shift left by 2 to create the address of L0 table
        let l1_entry = l1_pt.entries[va >> 22];
        let l0_address = (l1_entry & 0xFFFF_FC00) << 2;

        // 0xF means this also checks that RWX is 0, as well as FLG_VALID is true
        if (l1_entry & 0xF) == loader::FLG_VALID as u32 {
            // this is safe because all possible values can be represented as `u32`, the pointer
            // is valid, aligned, and the bounds are known.
            let l0_pt = unsafe { core::slice::from_raw_parts_mut(l0_address as *mut u32, 1024) };
            let l0_entry = l0_pt[(va & 0x003F_F000) >> 12];
            if (l0_entry & loader::FLG_VALID as u32) != 0 {
                if mark_free {
                    l0_pt[(va & 0x003F_F000) >> 12] = 0;
                }
                Some(((l0_entry as usize & 0xFFFF_FC00) << 2) | va & 0xFFF)
            } else {
                // writeln!(DebugUart {}, "pt_walk L0 entry invalid: {:x}", l0_entry).ok();
                None
            }
        } else {
            assert!((l1_entry & 0xE) == 0, "RWX was not zero on L1 PTE, unsupported mode of operation!");
            // writeln!(DebugUart {}, "pt_walk L1 entry invalid: {:x}", l1_entry).ok();
            None
        }
    }
}
struct SharedStateStorage {
    pub inner: Option<SwapperSharedState>,
    pub conn: CID,
}
impl SharedStateStorage {
    pub fn init(&mut self, sid: SID) {
        // Register the swapper with the kernel. Written as a raw syscall, since this is
        // the only instance of its use (no point in use-once code to wrap it).
        // This is an "early registration" which allows us to see debug output quickly,
        // even before we can constitute all of our shared state
        let (s0, s1, s2, s3) = sid.to_u32();
        xous::rsyscall(xous::SysCall::RegisterSwapper(
            s0,
            s1,
            s2,
            s3,
            swap_handler as *mut usize as usize,
            self as *mut SharedStateStorage as usize,
        ))
        .unwrap();
    }
}

fn map_swap(ss: &mut SwapperSharedState, swap_phys: usize, virt: usize, owner: u8) {
    assert!(swap_phys & 0xFFF == 0, "PA is not page aligned");
    assert!(virt & 0xFFF == 0, "VA is not page aligned");
    #[cfg(feature = "debug-verbose")]
    writeln!(DebugUart {}, "    swap pa {:x} -> va {:x}", swap_phys, virt).ok();
    let ppn1 = (swap_phys >> 22) & ((1 << 12) - 1);
    let ppn0 = (swap_phys >> 12) & ((1 << 10) - 1);

    let vpn1 = (virt >> 22) & ((1 << 10) - 1);
    let vpn0 = (virt >> 12) & ((1 << 10) - 1);
    assert!(owner != 0);
    let l1_pt = &mut ss.pts.roots[owner as usize - 1].entries;

    // Allocate a new level 1 pagetable entry if one doesn't exist.
    if l1_pt[vpn1] as usize & loader::FLG_VALID == 0 {
        let na = xous::map_memory(None, None, PAGE_SIZE, MemoryFlags::R | MemoryFlags::W)
            .expect("couldn't allocate a swap page table page")
            .as_ptr() as usize;
        writeln!(
            DebugUart {},
            "Swap Level 1 page table is invalid ({:08x}) @ {:08x} -- allocating a new one @ {:08x}",
            unsafe { l1_pt.as_ptr().add(vpn1) } as usize,
            l1_pt[vpn1],
            na
        )
        .ok();
        // Mark this entry as a leaf node (WRX as 0), and indicate
        // it is a valid page by setting "V".
        l1_pt[vpn1] = (((na & 0xFFFF_F000) >> 2) | loader::FLG_VALID) as u32;
    }

    let l0_pt_idx = unsafe { &mut (*(((l1_pt[vpn1] << 2) & !((1 << 12) - 1)) as *mut PtPage)) };
    let l0_pt = &mut l0_pt_idx.entries;

    // Ensure the entry hasn't already been mapped to a different address.
    if ((l0_pt[vpn0] as usize) & loader::FLG_VALID) != 0
        && (l0_pt[vpn0] as usize & 0xffff_fc00) != ((ppn1 << 20) | (ppn0 << 10))
    {
        // Panics don't print from the swapper in this context - must use the DebugUart to have the message
        // appear. Note that panics just appear as a store fault as the kernel unwind tries to lend messages
        // to the logger.
        writeln!(
            DebugUart {},
            "Swap page {:08x} was already allocated to {:08x}, so cannot map to {:08x}!",
            swap_phys,
            (l0_pt[vpn0] >> 10) << 12,
            virt
        )
        .ok();
        panic!("Swap page already allocated");
    }
    l0_pt[vpn0] = ((ppn1 << 20) | (ppn0 << 10) | loader::FLG_VALID) as u32;
}

/// blocking swap call handler
/// 8 argument values are always pushed on the stack; the meaning is bound differently based upon the specific
/// opcode. Not all arguments are used in all cases, unused argument values have no valid meaning (but in
/// practice typically contain the previous call's value, or 0).
fn swap_handler(
    shared_state: usize,
    opcode: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    _a5: usize,
    _a6: usize,
    _a7: usize,
) {
    // safety: lots of footguns actually, but this is the only way to get this pointer into
    // our context. SharedStateStorage is a Rust structure that is aligned and initialized,
    // so the cast is safe enough, but we have to be careful because this is executed in an
    // interrupt context: we can't wait on locks (they'll hang forever if they are locked).
    let sss = unsafe { &mut *(shared_state as *mut SharedStateStorage) };
    if sss.inner.is_none() {
        // Unearth all of our data trackers in the spots on the map where the loader should have buried them.

        // safety: this is only safe because the loader initializes and aligns the SwapSpec structure:
        //   - The SwapSpec structure is Repr(C), page-aligned, and fully initialized.
        //   - Furthermore, SWAP_CFG_VADDR is already mapped into our address space by the loader; we don't
        //     have to do mapping requests because it's already done for us!
        let swap_spec = unsafe { &*(SWAP_CFG_VADDR as *mut SwapSpec) };

        // reserve memory for hard OOM
        let mut reserved = xous::map_memory(
            None,
            None,
            PAGE_SIZE * HARD_OOM_RESERVED_PAGES,
            MemoryFlags::R | MemoryFlags::W | MemoryFlags::RESERVE,
        )
        .expect("could't reserve space for hard OOM handler");
        // *touch* the memory -- otherwise it might not actually be demand-paged
        let reserved_slice: &mut [u32] = unsafe { reserved.as_slice_mut() }; // this is safe because `u32` is fully representable
        reserved_slice.fill(0);

        // swapper is not allowed to use `log` for debugging under most circumstances, because
        // the swapper can't send messages when handling a swap call. Instead, we use a local
        // debug UART to handle this. This needs to be enabled with the "debug-print" feature
        // and is mutually exclusive with the "gdb-stub" feature in the kernel since it uses
        // the same physical hardware.
        sss.inner = Some(SwapperSharedState {
            // safety: this is only safe because:
            //   - the loader puts the swap root page table pages starting at SWAP_PT_VADDR
            //   - all the page table entries are fully initialized and contains only representable data
            //   - the length of the region is guaranteed by the loader
            pts: SwapPageTables {
                roots: unsafe {
                    core::slice::from_raw_parts_mut(
                        SWAP_PT_VADDR as *mut PtPage,
                        swap_spec.pid_count as usize,
                    )
                },
            },
            hal: SwapHal::new(swap_spec),
            // safety: this is safe because the loader has allocated this region and zeroed the contents,
            // and the length is correctly set up by the loader. Note that the length is slightly
            // longer than it needs to be -- the region that has to be tracked does not include the
            // area of swap dedicated to the MAC table, which swap_len includes.
            sct: SwapCountTracker {
                counts: unsafe {
                    core::slice::from_raw_parts_mut(
                        SWAP_COUNT_VADDR as *mut u32,
                        loader::swap::derive_usable_swap(swap_spec.swap_len as usize) / PAGE_SIZE,
                    )
                },
            },
            sram_start: swap_spec.sram_start as usize,
            sram_size: swap_spec.sram_size as usize,
            free_swap_search_origin: 0,
            alloc_heap: None,
            hard_oom_alloc_heap: None,
            report_full_rpt: true,
            hard_oom_reserved_page: Some(reserved),
        });
    }
    let ss = sss.inner.as_mut().expect("Shared state should be initialized");

    let op: Option<KernelOp> = FromPrimitive::from_usize(opcode);
    #[cfg(feature = "debug-print-verbose")]
    writeln!(DebugUart {}, "got Opcode: {:?}", op).ok();
    match op {
        Some(KernelOp::WriteToSwap) => {
            let pid = a2 as u8;
            let vaddr_in_pid = a3;
            let vaddr_in_swap = a4;
            write_to_swap_inner(ss, pid, vaddr_in_pid, vaddr_in_swap);
            #[cfg(feature = "debug-verbose")]
            {
                writeln!(DebugUart {}, "Swap count & usage table:").ok();
                for (i, &entry) in ss.sct.counts.iter().enumerate() {
                    if entry != 0 {
                        writeln!(DebugUart {}, "  {:04}:{:x}", i, entry).ok();
                    }
                }
            }
            // writeln!(DebugUart {}, "WTS exit").ok();
        }
        Some(KernelOp::ReadFromSwap) => {
            let pid = a2 as u8;
            let vaddr_in_pid = a3;
            let vaddr_in_swap = a4;
            // walk the PT to find the swap data, and remove it from the swap PT
            let paddr_in_swap = match ss.pt_walk(pid as u8, vaddr_in_pid, true) {
                Some(paddr) => paddr,
                None => {
                    writeln!(DebugUart {}, "Couldn't resolve swapped data. Was the page actually swapped?")
                        .ok();
                    panic!("Couldn't resolve swapped data. Was the page actually swapped?")
                }
            };
            // for some reason, `paddr_in_swap` must be printed for the routine to not crash. Or the delay
            // after `pt_walk` is necessary. Either way, it's spooky. I wonder if there isn't some minimum
            // time between reads from the SPIM that we're violating??
            #[cfg(feature = "debug-print")]
            writeln!(
                DebugUart {},
                "RFS PID{}, vaddr_pid {:x}, vaddr_swap {:x}, paddr {:x}",
                pid,
                vaddr_in_pid,
                vaddr_in_swap,
                paddr_in_swap
            )
            .ok();
            // clear the used bit in swap
            ss.sct.counts[paddr_in_swap / PAGE_SIZE] &= !loader::FLG_SWAP_USED;

            // safety: this is only safe because the pointer we're passed from the kernel is guaranteed to be
            // a valid u8-page in memory
            let buf = unsafe { core::slice::from_raw_parts_mut(vaddr_in_swap as *mut u8, PAGE_SIZE) };
            match ss.hal.decrypt_swap_from(
                buf,
                ss.sct.counts[paddr_in_swap / PAGE_SIZE],
                paddr_in_swap,
                vaddr_in_pid,
                pid,
            ) {
                Ok(_) => {}
                Err(e) => {
                    writeln!(
                        DebugUart {},
                        "Decryption error: swap image corrupted, the tag does not match the data! {:?}",
                        e
                    )
                    .ok();
                    panic!("Decryption error: swap image corrupted, the tag does not match the data!");
                }
            }
            // at this point, the `buf` has our desired data, we're done, modulo updating the count.
        }
        Some(KernelOp::ExecFetchAllocs) => {
            if let Some(alloc_heap) = &mut ss.alloc_heap {
                alloc_heap.clear();
                assert!(alloc_heap.len() == 0);
                let rpt = unsafe {
                    core::slice::from_raw_parts(SWAP_RPT_VADDR as *const SwapAlloc, ss.sram_size / PAGE_SIZE)
                };
                for (_i, &entry) in rpt.iter().enumerate() {
                    #[cfg(feature = "debug-verbose")]
                    if entry.raw_vpn() != 0 {
                        writeln!(DebugUart {}, "{:x}: {:x} [{}]", _i, entry.raw_vpn(), entry.timestamp())
                            .ok();
                    }
                    // filter out invalid, wired, or kernel/swapper candidates.
                    // report everything if requested (this is used to wire our heap memory prior to an OOM)
                    if (!entry.is_wired() && entry.is_valid() && entry.raw_pid() != 1 && entry.raw_pid() != 2)
                        || ss.report_full_rpt
                    {
                        //  writeln!(DebugUart {}, "Pushing {:x?}", entry).ok();
                        alloc_heap.push(entry);
                    }
                }
                #[cfg(feature = "debug-verbose")]
                writeln!(DebugUart {}, "Created heap with {} entries", alloc_heap.len()).ok();
                xous::try_send_message(
                    sss.conn,
                    Message::new_scalar(Opcode::FetchAllocsDone.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .ok();
            }
        }
        // HardOom handling will evict any and all pages that it can -- it does no filtering.
        Some(KernelOp::HardOom) => {
            // parse the arguments (none, currently)

            // be sure to allocate some extra space for the handler itself to run the next time!
            let mut pages_to_free = HARD_OOM_PAGE_TARGET + HARD_OOM_RESERVED_PAGES;

            // set the flag that hard oom ran during an Oom Doom handler pass
            MAYBE_HARD_OOM_DURING_OOM_DOOM.store(true, Ordering::SeqCst);

            // free memory for the hard-OOM handler to run
            if let Some(reserved_mem) = ss.hard_oom_reserved_page.take() {
                writeln!(DebugUart {}, "Entering HARD OOM - freeing scratch memory: {:x?}", reserved_mem)
                    .ok();
                xous::unmap_memory(reserved_mem).expect("couldn't free memory for hard OOM handler");
            } else {
                panic!("No space was reserved for the hard OOM manager to run!");
            }
            // recover the RPT from kernel
            let mut alloc_heap =
                ss.hard_oom_alloc_heap.take().expect("Hard OOM, but no pre-allocated storage for handler!");
            alloc_heap.clear();
            assert!(alloc_heap.len() == 0);
            let rpt = unsafe {
                core::slice::from_raw_parts(SWAP_RPT_VADDR as *const SwapAlloc, ss.sram_size / PAGE_SIZE)
            };
            for (_i, &entry) in rpt.iter().enumerate() {
                // filter out invalid, wired, or kernel/swapper candidates.
                if !entry.is_wired() && entry.is_valid() && entry.raw_pid() != 1 && entry.raw_pid() != 2 {
                    //  writeln!(DebugUart {}, "Pushing {:x?}", entry).ok();
                    alloc_heap.push(entry);
                }
            }
            // Inside the interrupt context, evict pages. No progress on any other process is made until this
            // loop is done. The loop is "inside-out" compared to the EvictPage call -- we can't make calls to
            // the kernel that would cause us to re-enter the swap context, because that would overwrite the
            // stored thread `sepc`. The syscalls used here are all "simple calls" that don't require re-entry
            // into the swapper context to handle.
            let target_pages = pages_to_free;
            let mut errs = 0;
            let mut wired = 0;
            loop {
                if pages_to_free == 0 {
                    break;
                }
                if let Some(candidate) = alloc_heap.pop() {
                    if candidate.is_wired()
                        || !candidate.is_valid()
                        || candidate.raw_pid() == 1
                        || candidate.raw_pid() == 2
                    {
                        wired += 1;
                    } else {
                        // step 1: steal the page from the other process. Its data gets mapped into the
                        // swapper as `local_ptr`. This will also unmap the page from memory.
                        let local_ptr = match xous::rsyscall(xous::SysCall::SwapOp(
                            SwapAbi::StealPage as usize,
                            candidate.raw_pid() as usize,
                            candidate.vaddr(),
                            0,
                            0,
                            0,
                            0,
                        )) {
                            Ok(Result::Scalar5(page_ptr, _, _, _, _)) => page_ptr,
                            Ok(_) => panic!("Malformed return value"),
                            Err(_e) => {
                                errs += 1;
                                continue; // try another page
                            }
                        };

                        // step 2: write the page to swap.
                        write_to_swap_inner(ss, candidate.raw_pid(), candidate.vaddr(), local_ptr);

                        // step 3: release the page (currently mapped into the swapper's memory space). Need
                        // to demonstrate to the memory system that we know what we are
                        // doing by also presenting the original PID that owned the page.
                        xous::rsyscall(xous::SysCall::SwapOp(
                            SwapAbi::ReleaseMemory as usize,
                            local_ptr,
                            candidate.raw_pid() as usize,
                            0,
                            0,
                            0,
                            0,
                        )).expect("Unexpected error: couldn't release a page that was mapped into the swapper's space");
                        pages_to_free -= 1;
                    }
                } else {
                    writeln!(
                        DebugUart {},
                        "Ran out of swappable candidates before we could free the requested number of pages!"
                    )
                    .ok();
                    break;
                }
            }
            // put the alloc heap back into the shared state
            ss.hard_oom_alloc_heap = Some(alloc_heap);
            writeln!(
                DebugUart {},
                "Exiting HARD OOM swap free loop: freed {} pages; {} requests rejected, {} wired",
                target_pages - pages_to_free - HARD_OOM_RESERVED_PAGES,
                errs,
                wired
            )
            .ok();
            //  Restore some reserved memory for the next hard OOM invocation.
            let mut reserved = xous::map_memory(
                None,
                None,
                PAGE_SIZE * HARD_OOM_RESERVED_PAGES,
                MemoryFlags::R | MemoryFlags::W | MemoryFlags::RESERVE,
            )
            .expect("could't reserve space for hard OOM handler");
            // *touch* the memory -- otherwise it might not actually be demand-paged
            let reserved_slice: &mut [u32] = unsafe { reserved.as_slice_mut() }; // this is safe because `u32` is fully representable
            reserved_slice.fill(0);
            ss.hard_oom_reserved_page = Some(reserved);
        }
        _ => {
            writeln!(DebugUart {}, "Unimplemented or unknown opcode: {}", opcode).ok();
        }
    }
}

/// Invokes OOM Doom, but only if it hasn't already been invoked.
fn try_invoke_oom_doom(oom_doom_running: &Arc<AtomicBool>, conn: CID, pages_to_free: usize) -> bool {
    if !oom_doom_running.swap(true, Ordering::SeqCst) {
        xous::try_send_message(
            conn,
            Message::new_scalar(Opcode::HandleOomDoom.to_usize().unwrap(), pages_to_free, 0, 0, 0),
        )
        .ok();
        true
    } else {
        false
    }
}

fn main() {
    let sid = xous::create_server().unwrap();
    let conn = xous::connect(sid).unwrap();
    let mut sss = Box::new(SharedStateStorage { conn, inner: None });
    sss.init(sid);

    // init the log, but this is mostly unused.
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    // wait for the share storage to become initialized, happens inside the handler
    // on the first call the kernel makes back. Usually it's done by now (by an alloc
    // advisory), but this check just ensures that happens.
    while sss.inner.is_none() {
        xous::yield_slice();
    }

    let total_ram = sss.inner.as_ref().unwrap().sram_size;
    // Binary heap for storing the view of the memory allocations.
    sss.inner.as_mut().unwrap().alloc_heap = Some(BinaryHeap::with_capacity(total_ram / PAGE_SIZE));
    sss.inner.as_mut().unwrap().hard_oom_alloc_heap = Some(BinaryHeap::with_capacity(total_ram / PAGE_SIZE));

    /// Poll interval for OOMer
    const OOM_POLL_INTERVAL_MS: u64 = 1000;
    // track the current amount of pages to be freed
    let mut pages_to_free = FREE_PAGE_TARGET;
    // track if the oom_doom process is running - so we don't get multiple spawns if the oom runner is taking
    // a long time
    let oom_doom_running = Arc::new(AtomicBool::new(false));

    // Do a single invocation at boot with 0 pages to free, to ensure that the page maps are set up,
    // and sufficient heap has been allocated for the swapper to run in case of a hard OOM. Failure to
    // do this can lead to missing L1 PT entries for the RPT mapping back into user space if the first
    // hard-OOM happens before the OOM-doom routine can run. All of swapper's memory is `wired`, so,
    // once we've done a dry-run, this memory stays ours forever.
    sss.inner.as_mut().unwrap().report_full_rpt = true;
    try_invoke_oom_doom(&oom_doom_running, conn, 0);

    // This thread is the active OOM monitor
    thread::spawn({
        let conn = conn.clone();
        let oom_doom_running = oom_doom_running.clone();
        move || {
            loop {
                let free_mem_pages = get_free_pages();
                if free_mem_pages < OOM_THRESH_PAGES {
                    let pages_to_free = FREE_PAGE_TARGET - free_mem_pages;
                    try_invoke_oom_doom(&oom_doom_running, conn, pages_to_free);
                }
                sleep(Duration::from_millis(OOM_POLL_INTERVAL_MS));
            }
        }
    });

    // This thread is for testing
    #[cfg(feature = "swap-userspace-testing")]
    thread::spawn({
        let conn = conn.clone();
        move || {
            loop {
                sleep(Duration::from_millis(2500));
                xous::send_message(conn, Message::new_scalar(Opcode::Test0.to_usize().unwrap(), 0, 0, 0, 0))
                    .ok();
            }
        }
    });

    let mut msg_opt = None;
    loop {
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        let msg = msg_opt.as_mut().unwrap();
        let op: Option<Opcode> = FromPrimitive::from_usize(msg.body.id());
        log::debug!("Swapper got {:x?}", op);
        match op {
            Some(Opcode::FetchAllocsDone) => {
                let ss = sss.inner.as_mut().unwrap();
                if let Some(mut alloc_heap) = ss.alloc_heap.take() {
                    log::info!("Attempting to free {} pages, {} candidates", pages_to_free, alloc_heap.len());
                    let target_pages = pages_to_free;
                    let mut errs = 0;
                    let mut wired = 0;
                    /* // for debugging ordering of suggested pages
                    use std::collections::VecDeque;
                    let mut preview_vec = VecDeque::new();
                    for _ in 0..16 {
                        preview_vec.push_back(alloc_heap.pop().unwrap());
                    }
                    for (i, entry) in preview_vec.iter().enumerate() {
                        println!("  {:x}: {:x} [{}]", i, entry.raw_vpn(), entry.timestamp());
                    } */

                    // A holding variable for items that we're de-prioritizing from removal. We only try these
                    // if we've exhausted all the other options.
                    let mut deprioritized = VecDeque::new();

                    // avoid log calls inside this loop, as we want to process all of these pages without
                    // context switches
                    loop {
                        // Abort the loop if a hard OOM happened, because our RPT view is now stale
                        if pages_to_free == 0 || MAYBE_HARD_OOM_DURING_OOM_DOOM.load(Ordering::SeqCst) {
                            break;
                        }
                        /*
                        if let Some(candidate) =
                            Some(preview_vec.pop_front().unwrap_or(alloc_heap.pop().unwrap()))
                        {
                        */
                        if let Some(candidate) = alloc_heap.pop() {
                            if candidate.is_wired()
                                || !candidate.is_valid()
                                || candidate.raw_pid() == 1
                                || candidate.raw_pid() == 2
                            {
                                // this should be 0, as it's pre-filtered by the kernel.
                                wired += 1;
                            } else {
                                if KEEP_VADDR_PREFIXES
                                    .iter()
                                    .find(|&&x| x == candidate.vaddr_prefix())
                                    .is_some()
                                {
                                    log::trace!("De-prioritizing {:x?}", candidate);
                                    deprioritized.push_back(candidate);
                                    continue;
                                }

                                // If hard OOM happened somewhere in this loop, this should return a harmless
                                // error if the candidate was already evicted; or it'll evict the candidate,
                                // and either way we'll exit the loop when we wrap around to the top, as our
                                // RPT views are now inconsistent.
                                match xous::rsyscall(xous::SysCall::SwapOp(
                                    SwapAbi::Evict as usize,
                                    candidate.raw_pid() as usize,
                                    candidate.vaddr(),
                                    0,
                                    0,
                                    0,
                                    0,
                                )) {
                                    Ok(_) => pages_to_free -= 1,
                                    Err(_e) => errs += 1,
                                }
                            }
                        } else if let Some(candidate) = deprioritized.pop_front() {
                            log::trace!("Falling back to: {:x?}", candidate);
                            match xous::rsyscall(xous::SysCall::SwapOp(
                                SwapAbi::Evict as usize,
                                candidate.raw_pid() as usize,
                                candidate.vaddr(),
                                0,
                                0,
                                0,
                                0,
                            )) {
                                Ok(_) => pages_to_free -= 1,
                                Err(_e) => errs += 1,
                            }
                            pages_to_free -= 1;
                        } else {
                            log::warn!(
                                "Ran out of swappable candidates before we could free the requested number of pages!"
                            );
                            break;
                        }
                    }
                    log::info!(
                        "Exiting swap free loop: freed {} pages; {} requests rejected, {} wired",
                        target_pages - pages_to_free,
                        errs,
                        wired
                    );
                    assert!(
                        wired == 0 || ss.report_full_rpt,
                        "Wired pages were handed to us, but they should have been filtered by the userspace handler!"
                    );
                    if ss.report_full_rpt {
                        ss.report_full_rpt = false; // reset this flag after the invocation has returned
                        // copy the alloc heap contents to the oom doom alloc heap so as to force its storage
                        // to alloc
                        for alloc in alloc_heap.iter() {
                            ss.hard_oom_alloc_heap.as_mut().unwrap().push(*alloc);
                        }
                    }
                }
                // restore the allocation for the binary heap
                ss.alloc_heap = Some(BinaryHeap::with_capacity(total_ram / PAGE_SIZE));

                // reset the hard OOM during OOM Doom lock
                if MAYBE_HARD_OOM_DURING_OOM_DOOM.swap(false, Ordering::SeqCst) {
                    log::warn!(
                        "Hard OOM detected during OOM Doom running; run was aborted because RPT view is now inconsistent"
                    );
                }

                // clear the running flag on exit
                oom_doom_running.store(false, Ordering::SeqCst);
                log::info!("Free mem after clearing: {}kiB", get_free_pages() * PAGE_SIZE / 1024);
            }
            Some(Opcode::HandleOomDoom) => {
                if let Some(scalar) = msg.body.scalar_message() {
                    // This should be a redundant store -- the caller should have set this to prevent multiple
                    // HandleOomDoom messages from being shoved into the server, blocking
                    // our ability to make progress. But we re-assert it just in case.
                    oom_doom_running.store(true, Ordering::SeqCst);
                    pages_to_free = scalar.arg1;
                    // trigger alloc fetch so we know what to free up
                    xous::rsyscall(xous::SysCall::SwapOp(SwapAbi::FetchAllocs as usize, 0, 0, 0, 0, 0, 0))
                        .ok();
                } else {
                    panic!("Wrong message type for HandleOomDoom");
                }
            }
            #[cfg(feature = "swap-userspace-testing")]
            Some(Opcode::Test0) => {
                log::info!("Free mem before clearing: {}kiB", get_free_pages() * PAGE_SIZE / 1024);
                // Try to free some number of pages
                try_invoke_oom_doom(&oom_doom_running, conn, 64);
            }
            _ => {
                log::info!("Unknown opcode {:?}", op);
            }
        }
    }
}

fn get_free_pages() -> usize {
    match xous::rsyscall(xous::SysCall::SwapOp(SwapAbi::GetFreePages as usize, 0, 0, 0, 0, 0, 0)) {
        Ok(Result::Scalar5(free_pages, _total_memory, _, _, _)) => free_pages,
        _ => panic!("GetFreeMem syscall failed"),
    }
}

/// Core of write_to_swap: this is also used by HardOom.
fn write_to_swap_inner(ss: &mut SwapperSharedState, pid: u8, vaddr_in_pid: usize, vaddr_in_swap: usize) {
    #[cfg(feature = "debug-print")]
    writeln!(DebugUart {}, "WTS PID{}, vaddr_pid {:x}, vaddr_swap {:x}", pid, vaddr_in_pid, vaddr_in_swap)
        .ok();
    // this is safe because the page is aligned and initialized as it comes from the kernel
    // remember that this page is overwritten with encrypted data
    let buf: &mut [u8] = unsafe { core::slice::from_raw_parts_mut(vaddr_in_swap as *mut u8, PAGE_SIZE) };

    // search the swap page tables for the next free page
    let mut next_free_page: Option<usize> = None;
    for slot in 0..ss.sct.counts.len() {
        let candidate = (ss.free_swap_search_origin + slot) % ss.sct.counts.len();
        if (ss.sct.counts[candidate] & loader::FLG_SWAP_USED) == 0 {
            next_free_page = Some(candidate);
            break;
        }
    }
    if let Some(free_page_number) = next_free_page {
        ss.free_swap_search_origin = free_page_number + 1; // start search at next page beyond the one about to be used
        // increment the swap counter by one, rolling over if full. Note that we only have 31
        // bits; the MSB is the "swap used" status bit
        let mut count = ss.sct.counts[free_page_number] & !loader::FLG_SWAP_USED;
        count = (count + 1) & !loader::FLG_SWAP_USED;
        ss.sct.counts[free_page_number] = count | loader::FLG_SWAP_USED;

        // add a PT mapping for the swap entry
        map_swap(ss, free_page_number * PAGE_SIZE, vaddr_in_pid, pid);

        ss.hal.encrypt_swap_to(buf, count, free_page_number * PAGE_SIZE, vaddr_in_pid, pid);
    } else {
        writeln!(DebugUart {}, "OOM detected, dumping all swap allocs:").ok();
        for (i, &entry) in ss.sct.counts.iter().enumerate() {
            writeln!(DebugUart {}, "  {:04}:{:x}", i, entry).ok();
        }
        // OOS path
        panic!("Ran out of swap space, hard OOM!");
    }
}
