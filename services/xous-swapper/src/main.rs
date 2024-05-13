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
//  - add kernel OOM callback
//  - add test command to force eviction of some variable number of pages
//  - create test program that over provisions heap versus available memory and prove that swap really works.
//  - Implement Evictor routine with a thing that checks free memory level, and then more intelligently swaps
//    stuff out to create free space for the kernel

mod debug;
mod platform;
use core::fmt::Write;
use std::collections::BinaryHeap;
use std::fmt::Debug;
use std::thread;
use std::thread::sleep;
use std::time::Duration;

use debug::*;
use loader::swap::{SwapAlloc, SwapSpec, SWAP_CFG_VADDR, SWAP_COUNT_VADDR, SWAP_PT_VADDR, SWAP_RPT_VADDR};
use num_traits::*;
use platform::{SwapHal, PAGE_SIZE};
use xous::{MemoryFlags, Message, Result, CID, PID, SID};

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
    GetFreeMem = 2,
    FetchAllocs = 3,
    SetOomThresh = 4,
}
/// SYNC WITH `kernel/src/swap.rs`
impl SwapAbi {
    pub fn from(val: usize) -> SwapAbi {
        use SwapAbi::*;
        match val {
            1 => Evict,
            2 => GetFreeMem,
            3 => FetchAllocs,
            4 => SetOomThresh,
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
    /// OOM warning from the kernel
    OomDoom = 3,
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
    pub alloc_heap: Option<BinaryHeap<SwapAlloc>>,
}
impl SwapperSharedState {
    pub fn pt_walk(&self, pid: u8, va: usize) -> Option<usize> {
        let l1_pt = &self.pts.roots[pid as usize - 1];
        let l1_entry = (l1_pt.entries[(va & 0xFFC0_0000) >> 22] >> 10) << 12;

        if l1_entry != 0 {
            // this is safe because all possible values can be represented as `u32`, the pointer
            // is valid, aligned, and the bounds are known.
            let l0_pt = unsafe { core::slice::from_raw_parts(l1_entry as *const u32, 1024) };
            let l0_entry = l0_pt[(va & 0x003F_F000) >> 12];
            if l0_entry & 1 != 0 {
                // bit 1 is the "valid" bit
                Some(((l0_entry as usize >> 10) << 12) | va & 0xFFF)
            } else {
                None
            }
        } else {
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
        l1_pt[vpn1] = (((na >> 12) << 10) | loader::FLG_VALID) as u32;
    }

    let l0_pt_idx = unsafe { &mut (*(((l1_pt[vpn1] << 2) & !((1 << 12) - 1)) as *mut PtPage)) };
    let l0_pt = &mut l0_pt_idx.entries;

    // Ensure the entry hasn't already been mapped to a different address.
    if (l0_pt[vpn0] as usize) & 1 != 0
        && (l0_pt[vpn0] as usize & 0xffff_fc00) != ((ppn1 << 20) | (ppn0 << 10))
    {
        panic!(
            "Swap page {:08x} was already allocated to {:08x}, so cannot map to {:08x}!",
            swap_phys,
            (l0_pt[vpn0] >> 10) << 12,
            virt
        );
    }
    let previous_flags = l0_pt[vpn0] as usize & 0x3f;
    l0_pt[vpn0] = ((ppn1 << 20) | (ppn0 << 10) | previous_flags | loader::FLG_VALID) as u32;
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
        });
    }
    let ss = sss.inner.as_mut().expect("Shared state should be initialized");

    let op: Option<KernelOp> = FromPrimitive::from_usize(opcode);
    writeln!(DebugUart {}, "got Opcode: {:?}", op).ok();
    match op {
        Some(KernelOp::WriteToSwap) => {
            let pid = a2 as u8;
            let vaddr_in_pid = a3;
            let vaddr_in_swap = a4;
            // this is safe because the page is aligned and initialized as it comes from the kernel
            // renember that this page is overwritten with encrypted data
            let buf: &mut [u8] =
                unsafe { core::slice::from_raw_parts_mut(vaddr_in_swap as *mut u8, PAGE_SIZE) };

            // search the swap page tables for the next free page
            let mut next_free_page: Option<usize> = None;
            for slot in 0..ss.sct.counts.len() {
                let candidate = (ss.free_swap_search_origin + slot) % ss.sct.counts.len();
                if ss.sct.counts[candidate] & loader::FLG_SWAP_USED != 0 {
                    next_free_page = Some(candidate);
                    break;
                }
            }
            if let Some(free_page_number) = next_free_page {
                // increment the swap counter by one, rolling over if full. Note that we only have 31
                // bits; the MSB is the "swap used" status bit
                let mut count = ss.sct.counts[free_page_number] & !loader::FLG_SWAP_USED;
                count = (count + 1) & !loader::FLG_SWAP_USED;
                ss.sct.counts[free_page_number] = count;

                // add a PT mapping for the swap entry
                map_swap(ss, free_page_number, vaddr_in_pid, pid);

                ss.hal.encrypt_swap_to(buf, count, free_page_number, vaddr_in_pid, pid);
            } else {
                // OOS path
                panic!("Ran out of swap space, hard OOM!");
            }
        }
        Some(KernelOp::ReadFromSwap) => {
            let pid = a2 as u8;
            let vaddr_in_pid = a3;
            let vaddr_in_swap = a4;
            writeln!(
                DebugUart {},
                "rfs ptw PID{}, vaddr_pid {:x}, vaddr_swap {:x}",
                pid,
                vaddr_in_pid,
                vaddr_in_swap
            )
            .ok();
            // walk the PT to find the swap data
            let paddr_in_swap = ss
                .pt_walk(pid as u8, vaddr_in_pid)
                .expect("Couldn't resolve swapped data. Was the page actually swapped?");
            // safety: this is only safe because the pointer we're passed from the kernel is guaranteed to be
            // a valid u8-page in memory
            let buf = unsafe { core::slice::from_raw_parts_mut(vaddr_in_swap as *mut u8, PAGE_SIZE) };
            ss.hal
                .decrypt_swap_from(
                    buf,
                    ss.sct.counts[paddr_in_swap / PAGE_SIZE],
                    paddr_in_swap,
                    vaddr_in_pid,
                    pid,
                )
                .expect("Decryption error: swap image corrupted, the tag does not match the data!");
            // at this point, the `buf` has our desired data, we're done, modulo updating the count.
        }
        Some(KernelOp::ExecFetchAllocs) => {
            if let Some(alloc_heap) = &mut ss.alloc_heap {
                alloc_heap.clear();
                let rpt = unsafe {
                    core::slice::from_raw_parts(SWAP_RPT_VADDR as *const SwapAlloc, ss.sram_size / PAGE_SIZE)
                };
                for &entry in rpt.iter() {
                    if !entry.is_wired() && entry.is_valid() {
                        //  writeln!(DebugUart {}, "Pushing {:x?}", entry).ok();
                        alloc_heap.push(entry);
                    }
                }
                writeln!(DebugUart {}, "Allocs have been copied").ok();
                xous::try_send_message(
                    sss.conn,
                    Message::new_scalar(Opcode::FetchAllocsDone.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .ok();
                writeln!(DebugUart {}, "FetchAllocsDone sent").ok();
            }
            writeln!(DebugUart {}, "Dummy ExecFetchAllocs").ok();
        }
        _ => {
            writeln!(DebugUart {}, "Unimplemented or unknown opcode: {}", opcode).ok();
        }
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
    // handle for the current sorted vector of swap-out candidates
    let mut swap_candidates: Option<Vec<SwapAlloc>> = None;

    /// Threshold for OomDoom callback
    const OOM_THRESH_PAGES: usize = 16;
    /// Target of free pages we want to get to after OomDoom call
    const FREE_PAGE_TARGET: usize = 32;
    // set OOM threshold - first argument is the threshold, in pages, below which we ping the swapper to start
    // clearing memory
    xous::rsyscall(xous::SysCall::SwapOp(SwapAbi::SetOomThresh as usize, OOM_THRESH_PAGES, 0, 0, 0, 0, 0))
        .ok();

    thread::spawn({
        let conn = conn.clone();
        move || {
            loop {
                log::info!("Kernel reports free mem: {}kiB", get_free_mem() / 1024);
                xous::send_message(conn, Message::new_scalar(Opcode::Test0.to_usize().unwrap(), 0, 0, 0, 0))
                    .ok();
                sleep(Duration::from_millis(5000));
            }
        }
    });

    let mut msg_opt = None;
    loop {
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        let msg = msg_opt.as_mut().unwrap();
        let op: Option<Opcode> = FromPrimitive::from_usize(msg.body.id());
        log::info!("Swapper got {:x?}", op);
        match op {
            Some(Opcode::FetchAllocsDone) => {
                let ss = sss.inner.as_mut().unwrap();
                if let Some(alloc_heap) = ss.alloc_heap.take() {
                    let candidates = alloc_heap.into_sorted_vec();
                    for (i, candidate) in candidates[..8.min(candidates.len())].iter().enumerate() {
                        log::info!("Candidate for swap-out #{}: {:x?}", i, candidate);
                    }
                    swap_candidates = Some(candidates);
                }
                // restore the allocation for the binary heap
                ss.alloc_heap = Some(BinaryHeap::with_capacity(total_ram / PAGE_SIZE));
            }
            Some(Opcode::Test0) => {
                if let Some(candidates) = &swap_candidates {
                    for (i, candidate) in candidates[..3.min(candidates.len())].iter().enumerate() {
                        log::info!("Test0 replay swap-out #{}: {:x?}", i, candidate);
                    }
                }
                log::info!(
                    "FetchAllocs result: {:?}",
                    xous::rsyscall(xous::SysCall::SwapOp(SwapAbi::FetchAllocs as usize, 0, 0, 0, 0, 0, 0))
                );
            }
            _ => {
                log::info!("Unknown opcode {:?}", op);
            }
        }
    }
}

fn get_free_mem() -> usize {
    match xous::rsyscall(xous::SysCall::SwapOp(SwapAbi::GetFreeMem as usize, 0, 0, 0, 0, 0, 0)) {
        Ok(Result::Scalar5(mem, _, _, _, _)) => mem,
        _ => panic!("GetFreeMem sycall failed"),
    }
}
