// TODO: redo docs to match streamlined architecture

//! ==Architecture==
//!
//! The Xous philosophy is to leave the kernel lightweight and free of dependencies. The swap implementation
//! adheres to this by trying to move as much of the difficult algorithmic processing and performance tuning
//! outside of the kernel.
//!
//! The one thing swap does introduce to the kernel that is algorithm-y is a renormalization routine for
//! counting page accesses. We track page access frequency with a 32-bit "epoch" counter, which is simply
//! incremented whenever a page table interaction happens. We don't use a 64-bit counter because greatly
//! increases the memory used to track things due to the single 64-bit record forcing the next item to also
//! have 64-bit alignment, thus effectively wasting several bytes per page. Anyways, when the epoch is about
//! to roll-over, a mostly in-place sweep with no allocations beyond a few dozen bytes in stack is done to the
//! memory usage tracker to "compact" the epoch numbers down. There is a #[test] in the kernel crate for this
//! routine.
//!
//! In order to perform all the other processing outside of the kernel, the swapper introduces a special new
//! "blocking userspace handler". It's "IRQ-like", in that it borrows the same mechanism used for blocking
//! IRQ handlers, but with different entry and exit magic numbers so we can differentiate the two. The
//! blocking userspace handler happens with interrupts disabled, giving it an atomic view of all of memory for
//! the duration of the handler.
//!
//! == Measuring Memory Usage ==
//!
//! The swapper needs to come up with an answer for which page to swap out, and it
//! also needs to know when to do it (OOM pressure).
//!
//! OOM pressure is handled with a syscall to the kernel to query the current `MEMORY_ALLOCATIONS`
//! table and return the available RAM. This is queried periodically with a timer, and if we
//! fall below a certain threshold, the swapper will force a pre-emptive OOM.
//!
//! When the `swap` feature is selected, `MEMORY_ALLOCATIONS` is upgraded from a `u8` to a table of
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
//! an interrupt context. To initiate OOM handling, PID 2 is invoked by the kernel with a call the swapper
//! interrupt context with `MEMORY_ALLOCATIONS` mapped into its memory space. At this point, PID 2 will copy
//! the current `MEMORY_ALLOCATIONS` table into a pre-allocated BinaryHeap in the shared state structure,
//! indexed by the timestamp. At this point, the blocking userspace handler can work through a sorted vector
//! of allocations to pick the pages it wants to remove.

mod debug;
mod platform;
use core::fmt::Write;
use std::collections::{BinaryHeap, VecDeque};
use std::fmt::Debug;
use std::thread;
use std::thread::sleep;
use std::time::Duration;

use debug::*;
use loader::swap::{SwapAlloc, SwapSpec, SWAP_CFG_VADDR, SWAP_COUNT_VADDR, SWAP_PT_VADDR, SWAP_RPT_VADDR};
use num_traits::*;
use platform::{SwapHal, PAGE_SIZE};
use xous::{MemoryFlags, MemoryRange, Message, Result, CID, PID, SID};

/// Target of pages to free in case of a Hard OOM. Note that the PAGE_TARGET numbers
/// are imprecise, in that there is a chance that one target is active during another
/// invocation of a routine. This is because the hard OOM handler is entirely asynchronous
/// and could be invoked at any time, including while we are trying to handle a soft OOM.
const HARD_OOM_PAGE_TARGET: usize = 32;
/// Target of pages to free in case of OOM Doom
#[cfg(feature = "oom-doom")]
const OOM_DOOM_PAGE_TARGET: usize = 48;
/// Polling interval for OOM Doom. Slightly off from an even second so we don't have constant
/// competition with other processes that probably use even-second multiples for polling.
#[cfg(feature = "oom-doom")]
const OOM_DOOM_POLL_INTERVAL_MS: u64 = 1057;

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
    ClearMemoryNow = 1,
    GetFreePages = 2,
    // FetchAllocs = 3,
    // HardOom = 4, // meant to be initiated within the kernel to itself
    StealPage = 5,
    ReleaseMemory = 6,
}
/// SYNC WITH `kernel/src/swap.rs`
impl SwapAbi {
    pub fn from(val: usize) -> SwapAbi {
        use SwapAbi::*;
        match val {
            1 => ClearMemoryNow,
            2 => GetFreePages,
            // 3 => FetchAllocs,
            // 4 => HardOom,
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
    /// Find the requested page, decrypt it, and return it
    ReadFromSwap = 1,
    /// Hard OOM invocation - stop everything and free memory!
    HardOom = 3,
}

/// public userspace & swapper handler -> swapper userspace ABI
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
#[repr(usize)]
pub enum Opcode {
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

/// Number of pages to reserve for hard OOM handling. In case of a hard OOM, there are 0 pages
/// available, which makes it impossible for the hard OOM handler to do things like allocate an L1
/// page table entry to track memory being swapped out. This places a hold on some memory that's
/// de-allocated on entry to the hard OOM handler, and re-allocated on exit.
///
/// Known things the swapper has to allocate memory for in hard-OOM:
///   - L1 page table entries for tracking swap
///   - An extra page for stack (needed for cramium targets, but not on precursor due to HAL differences)
///   - An additional page will be requested in case of panic in hard OOM for TLS, but these shouldn't happen
///     so we don't reserve it.
const HARD_OOM_RESERVED_PAGES: usize = 2;

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
    pub hard_oom_alloc_heap: Option<BinaryHeap<SwapAlloc>>,
    /// Reserve some memory to be freed by the hard OOM manager. These pages are needed to do things
    /// like create L1 page table entries for the swapper to track evicted pages.
    pub hard_oom_reserved_page: Option<MemoryRange>,
    pub report_full_rpt: bool,
    /// number of pages to free in the OOM routine. Note that this value is imprecise: it can
    /// be mutated by the userspace soft-OOM handler at any time.
    pub pages_to_free: usize,
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
    pub _conn: CID,
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

/// Convenience wrapper for GetFreePages syscall
fn get_free_pages() -> usize {
    match xous::rsyscall(xous::SysCall::SwapOp(SwapAbi::GetFreePages as usize, 0, 0, 0, 0, 0, 0)) {
        Ok(Result::Scalar5(free_pages, _total_memory, _, _, _)) => free_pages,
        _ => panic!("GetFreeMem syscall failed"),
    }
}

/// Core of write_to_swap.
fn write_to_swap_inner(
    ss: &mut SwapperSharedState,
    candidate: SwapAlloc,
    errs: &mut usize,
    pages_to_free: &mut usize,
) -> core::result::Result<(), xous::Error> {
    // step 1: steal the page from the other process. Its data gets mapped into the
    // swapper as `local_ptr`. This will also unmap the page from memory.
    let vaddr_in_swap = match xous::rsyscall(xous::SysCall::SwapOp(
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
            *errs += 1;
            return Err(xous::Error::ShareViolation); // try another page
        }
    };

    // step 2: write the page to swap
    #[cfg(feature = "debug-print")]
    writeln!(DebugUart {}, "WTS PID{} VA {:x}", candidate.raw_pid(), vaddr_in_swap).ok();
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
        map_swap(ss, free_page_number * PAGE_SIZE, candidate.vaddr(), candidate.raw_pid());

        ss.hal.encrypt_swap_to(
            buf,
            count,
            free_page_number * PAGE_SIZE,
            candidate.vaddr(),
            candidate.raw_pid(),
        );
    } else {
        writeln!(DebugUart {}, "OOM detected, dumping all swap allocs:").ok();
        for (i, &entry) in ss.sct.counts.iter().enumerate() {
            writeln!(DebugUart {}, "  {:04}:{:x}", i, entry).ok();
        }
        // OOS path
        panic!("Ran out of swap space, hard OOM!");
    }

    // step 3: release the page (currently mapped into the swapper's memory space). Need
    // to demonstrate to the memory system that we know what we are
    // doing by also presenting the original PID that owned the page.
    xous::rsyscall(xous::SysCall::SwapOp(
        SwapAbi::ReleaseMemory as usize,
        vaddr_in_swap,
        candidate.raw_pid() as usize,
        0,
        0,
        0,
        0,
    ))
    .expect("Unexpected error: couldn't release a page that was mapped into the swapper's space");
    *pages_to_free -= 1;

    Ok(())
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
            hard_oom_alloc_heap: None,
            report_full_rpt: true,
            hard_oom_reserved_page: Some(reserved),
            pages_to_free: HARD_OOM_PAGE_TARGET + HARD_OOM_RESERVED_PAGES,
        });
    }
    let ss = sss.inner.as_mut().expect("Shared state should be initialized");

    let op: Option<KernelOp> = FromPrimitive::from_usize(opcode);
    #[cfg(feature = "debug-print-verbose")]
    writeln!(DebugUart {}, "got Opcode: {:?}", op).ok();
    match op {
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
            writeln!(DebugUart {}, "RFS PID{} VA {:x} PA {:x}", pid, vaddr_in_pid, paddr_in_swap).ok();
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
        // HardOom handling will evict any and all pages that it can -- it does no filtering.
        Some(KernelOp::HardOom) => {
            // parse the arguments (none, currently)

            // be sure to allocate some extra space for the handler itself to run the next time!
            let mut pages_to_free = ss.pages_to_free;

            // free memory for the hard-OOM handler to run
            if let Some(reserved_mem) = ss.hard_oom_reserved_page.take() {
                writeln!(
                    DebugUart {},
                    "Entering HARD OOM attempt to free {} pages - scratch memory: {:x?}",
                    pages_to_free,
                    reserved_mem,
                )
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
                // filter out invalid, wired, or kernel/swapper candidates
                if (!entry.is_wired() && entry.is_valid() && entry.raw_pid() != 1 && entry.raw_pid() != 2)
                // report_full_rpt is used to force the heap to reserve all the data we might need in a future oom
                    || ss.report_full_rpt
                {
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
            let mut errs: usize = 0;
            let mut wired: usize = 0;

            // A holding variable for items that we're de-prioritizing from removal. We only try these
            // if we've exhausted all the other options.
            let mut deprioritized = VecDeque::new();

            while pages_to_free > 0 {
                if let Some(candidate) = alloc_heap.pop() {
                    if candidate.is_wired()
                        || !candidate.is_valid()
                        || candidate.raw_pid() == 1
                        || candidate.raw_pid() == 2
                    {
                        wired += 1;
                    } else {
                        if KEEP_VADDR_PREFIXES.iter().find(|&&x| x == candidate.vaddr_prefix()).is_some() {
                            log::trace!("De-prioritizing {:x?}", candidate);
                            deprioritized.push_back(candidate);
                            continue;
                        }
                        // error is ignored because the correct behavior on error is to try another page
                        write_to_swap_inner(ss, candidate, &mut errs, &mut pages_to_free).ok();
                    }
                } else if let Some(candidate) = deprioritized.pop_front() {
                    write_to_swap_inner(ss, candidate, &mut errs, &mut pages_to_free).ok();
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
            if ss.report_full_rpt {
                ss.report_full_rpt = false;
            }
        }
        _ => {
            writeln!(DebugUart {}, "Unimplemented or unknown opcode: {}", opcode).ok();
        }
    }
}

fn main() {
    let sid = xous::create_server().unwrap();
    let conn = xous::connect(sid).unwrap();
    let mut sss = Box::new(SharedStateStorage { _conn: conn, inner: None });
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
    sss.inner.as_mut().unwrap().hard_oom_alloc_heap = Some(BinaryHeap::with_capacity(total_ram / PAGE_SIZE));

    // Do a single invocation at boot with 0 pages to free, to ensure that the page maps are set up,
    // and sufficient heap has been allocated for the swapper to run in case of a hard OOM. Failure to
    // do this can lead to missing L1 PT entries for the RPT mapping back into user space if the first
    // hard-OOM happens before the OOM-doom routine can run. All of swapper's memory is `wired`, so,
    // once we've done a dry-run, this memory stays ours forever.
    sss.inner.as_mut().unwrap().report_full_rpt = true;
    sss.inner.as_mut().unwrap().pages_to_free = 2;
    xous::rsyscall(xous::SysCall::SwapOp(SwapAbi::ClearMemoryNow as usize, 0, 0, 0, 0, 0, 0))
        .expect("ClearMemoryNow syscall failed");
    // restore the normal parameters
    sss.inner.as_mut().unwrap().report_full_rpt = false;
    sss.inner.as_mut().unwrap().pages_to_free = HARD_OOM_PAGE_TARGET + HARD_OOM_RESERVED_PAGES;

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

    // This thread pings the free memory level and will try to clear memory to avoid OOM
    #[cfg(feature = "oom-doom")]
    thread::spawn({
        move || {
            loop {
                sleep(Duration::from_millis(OOM_DOOM_POLL_INTERVAL_MS));
                if get_free_pages() < OOM_DOOM_PAGE_TARGET {
                    sss.inner.as_mut().unwrap().pages_to_free = OOM_DOOM_PAGE_TARGET;
                    xous::rsyscall(xous::SysCall::SwapOp(SwapAbi::ClearMemoryNow as usize, 0, 0, 0, 0, 0, 0))
                        .expect("ClearMemoryNow syscall failed");
                    sss.inner.as_mut().unwrap().pages_to_free = HARD_OOM_PAGE_TARGET;
                }
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
            #[cfg(feature = "swap-userspace-testing")]
            Some(Opcode::Test0) => {
                log::info!("Free mem: {}kiB", get_free_pages() * PAGE_SIZE / 1024);
            }
            _ => {
                log::info!("Unknown opcode {:?}", op);
            }
        }
    }
}
