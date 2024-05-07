mod debug;
mod platform;
use core::fmt::Write;
use std::num::NonZeroUsize;
use std::thread;
use std::thread::sleep;
use std::time::Duration;
use std::{fmt::Debug, num::NonZeroU8};

use debug::*;
use heapless::LinearMap;
use loader::swap::{SwapSpec, SWAP_CFG_VADDR, SWAP_PT_VADDR, SWAP_RPT_VADDR};
use lru::LruCache;
use num_traits::*;
use platform::{SwapHal, PAGE_SIZE};
use xous::{AllocAdvice, Message, Result, CID, PID};

/// This controls how deep we aggregate advisories before passing them into true
/// userspace. The trade-off is performance (bigger depth takes longer to search),
/// precision (a larger aggregation can make our tracking less accurate, as updates
/// happen less frequently), and performance (aggregating advisories reduces expensive
/// updates of the allocation LRU cache). This should also be a power of 2 for
/// best performance, because the underlying primitive is `heapless`.
const ADVISORY_AGGREGATE_DEPTH: usize = 8;

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
}
impl SwapAbi {
    pub fn from(val: usize) -> SwapAbi {
        use SwapAbi::*;
        match val {
            1 => Evict,
            2 => GetFreeMem,
            _ => Invalid,
        }
    }
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

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
#[repr(usize)]
pub enum Opcode {
    /// Take the page lent to us, encrypt it and write it to swap
    WriteToSwap = 0,
    /// Find the requested page, decrypt it, and return it
    ReadFromSwap = 1,
    /// Kernel message advising us that a page of RAM was allocated
    AllocateAdvisory = 2,
    /// Test messages
    Test0 = 128,
    /// A message from the kernel handler context to evaluate if a trim is needed
    EvalTrim = 256,
}

/// This structure contains shared state accessible between the userspace code and the blocking swap call
/// handler.
pub struct SwapperSharedState {
    pub pts: SwapPageTables,
    pub hal: SwapHal,
    pub rpt: RuntimePageTracker,
    pub sram_start: usize,
    pub sram_size: usize,
    pub alloc_aggregator: LinearMap<usize, AllocAdvice, ADVISORY_AGGREGATE_DEPTH>,
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

pub fn change_owner(ss: &mut SwapperSharedState, pid: u8, paddr: usize) {
    // First, check to see if the region is in RAM,
    if paddr >= ss.sram_start && paddr < ss.sram_start + ss.sram_size {
        // Mark this page as in-use by the kernel
        ss.rpt.allocs[(paddr - ss.sram_start as usize) / PAGE_SIZE] = PID::new(pid);
        return;
    }
    // The region isn't in RAM. We're in the swapper, we can't handle errors - drop straight to panic.
    panic!("Tried to swap region {:08x} that isn't in RAM!", paddr);
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
    a5: usize,
    a6: usize,
    a7: usize,
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
            // safety: this is only safe because the loader guarantees this raw pointer is initialized and
            // aligned correctly
            rpt: RuntimePageTracker {
                allocs: unsafe {
                    core::slice::from_raw_parts_mut(
                        SWAP_RPT_VADDR as *mut Option<NonZeroU8>,
                        swap_spec.rpt_len_bytes as usize,
                    )
                },
            },
            sram_start: swap_spec.sram_start as usize,
            sram_size: swap_spec.sram_size as usize,
            alloc_aggregator: LinearMap::new(),
        });
    }
    let ss = sss.inner.as_mut().expect("Shared state should be initialized");

    let op: Option<Opcode> = FromPrimitive::from_usize(opcode);
    writeln!(DebugUart {}, "got Opcode: {:?}", op).ok();
    match op {
        Some(Opcode::WriteToSwap) => {
            let pid = a2 as u8;
            let vaddr_in_pid = a3;
            let vaddr_in_swap = a4;
            // next steps on the swap journey:
            //  - create a "dummy" Evictor routine that just tell the kernel to evict one of our resident
            //    processes
            //  - this should trigger WriteToSwap here
            //  - implement the routine here
            //  - also implement some routines to track free memory to check that things are doing what we
            //    expect them to do
            //
            //  - Do something with allocate advisories (e.g. LRU cache with pre-defined capacity)
            //  - Replace the Evictor routine with a thing that checks free memory level, and then more
            //    intelligently swaps stuff out to create free space for the kernel
            //
            //  - Somehow come up with some test cases for stress-testing the swapper...probably some function
            //    that allocates a bunch of heap, and occasionally touches the contents, while other system
            //    processes trundle on.
        }
        Some(Opcode::ReadFromSwap) => {
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
            // TODO: manage swap_count
            ss.hal
                .decrypt_swap_from(buf, 0, paddr_in_swap, vaddr_in_pid, pid)
                .expect("Decryption error: swap image corrupted, the tag does not match the data!");
            // at this point, the `buf` has our desired data, we're done, modulo updating the count.
        }
        Some(Opcode::AllocateAdvisory) => {
            let advisories = [
                xous::AllocAdvice::deserialize(a2, a3),
                xous::AllocAdvice::deserialize(a4, a5),
                xous::AllocAdvice::deserialize(a6, a7),
            ];
            // Record all the advice into a short LinearMap. We desires the keys (physical addresses) to
            // be overwritten if multiple advisories are issued to a single physical address -- no need to
            // track all that activity down to the userspace. A hot page is a hot page: LRU cache will get
            // touched, touching it twice in a row doesn't make it significantly less recently used.
            for advice in advisories {
                match advice {
                    xous::AllocAdvice::Allocate(pid, vaddr, paddr) => {
                        change_owner(ss, pid.get(), paddr);
                        match ss.alloc_aggregator.insert(paddr, AllocAdvice::Allocate(pid, vaddr, paddr)) {
                            Ok(_) => {}
                            Err((a, _advice)) => {
                                writeln!(
                                    DebugUart {},
                                    "alloc_aggregator capacity exceeded, dropping request @ PA {:x}",
                                    a,
                                )
                                .ok();
                            }
                        }
                    }
                    xous::AllocAdvice::Free(pid, vaddr, paddr) => {
                        change_owner(ss, 0, paddr);
                        match ss.alloc_aggregator.insert(paddr, AllocAdvice::Free(pid, vaddr, paddr)) {
                            Ok(_) => {}
                            Err((a, _advice)) => {
                                writeln!(
                                    DebugUart {},
                                    "alloc_aggregator capacity exceeded, dropping request @ PA {:x}",
                                    a,
                                )
                                .ok();
                            }
                        }
                    }
                    xous::AllocAdvice::Uninit => {} // not all the records have to be populated
                }
            }
            // flush what entries we can, filling an even multiple of messages so we are efficiently
            // utilizing the expensive messaging channel to userspace
            let mut removed_pa = [0usize; 2]; // track removals to avoid interior mutability problems
            while ss.alloc_aggregator.len() >= 2 {
                {
                    let mut removed_entries: [Option<AllocAdvice>; 2] = [None; 2];
                    for (index, (&pa, &entry)) in ss.alloc_aggregator.iter().enumerate() {
                        if index == removed_pa.len() {
                            break;
                        }
                        removed_pa[index] = pa;
                        removed_entries[index] = Some(entry)
                    }
                    let (a1, a2) = removed_entries[0].take().unwrap().serialize();
                    let (a3, a4) = removed_entries[1].take().unwrap().serialize();
                    match xous::try_send_message(
                        sss.conn,
                        Message::new_scalar(Opcode::AllocateAdvisory as usize, a1, a2, a3, a4),
                    ) {
                        Ok(_) => {}
                        Err(e) => {
                            // report the error, and just...lose the message, don't retry or anything like
                            // that.
                            writeln!(DebugUart {}, "Couldn't send advisory to userspace: {:?}", e).ok();
                        }
                    }
                }
                // do the removals, outside of the previous for loop
                for entry in removed_pa.iter() {
                    ss.alloc_aggregator.remove(entry);
                }
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
    let mut sss = SharedStateStorage { conn, inner: None };
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
        &mut sss as *mut SharedStateStorage as usize,
    ))
    .unwrap();

    // init the log, but this is mostly unused.
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    // LRU cache
    // We want to be able to ask it, "give me the next page to evict". The answer should be the
    // least recently used page, and the format of the answer is a physical page number.
    //
    // The cache is keyed by physical page number; the values are an Option<(PID, vaddr)> tuple that
    // corresponds to the current owner of that page.
    let (_free, total) = get_free_mem();
    let mut lru = LruCache::new(NonZeroUsize::new(total / 4096).unwrap());

    thread::spawn({
        let conn = conn.clone();
        move || {
            loop {
                match xous::rsyscall(xous::SysCall::SwapOp(SwapAbi::GetFreeMem as usize, 0, 0, 0, 0, 0, 0)) {
                    Ok(Result::Scalar5(mem, total, _, _, _)) => {
                        log::info!("Free mem: {}kiB / {}kiB", mem / 1024, total / 1024)
                    }
                    Ok(e) => log::warn!("Unexpected response: {:?}", e),
                    Err(e) => log::warn!("Error: {:?}", e),
                }
                xous::send_message(conn, Message::new_scalar(Opcode::Test0.to_usize().unwrap(), 0, 0, 0, 0))
                    .ok();
                sleep(Duration::from_millis(1000));
            }
        }
    });

    let mut msg_opt = None;
    loop {
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        let msg = msg_opt.as_mut().unwrap();
        let op: Option<Opcode> = FromPrimitive::from_usize(msg.body.id());
        log::info!("Swapper got {:?}", msg);
        match op {
            Some(Opcode::WriteToSwap) => {
                unimplemented!();
            }
            Some(Opcode::AllocateAdvisory) => {
                if let Some(scalar) = msg.body.scalar_message() {
                    let advisories = [
                        AllocAdvice::deserialize(scalar.arg1, scalar.arg2),
                        AllocAdvice::deserialize(scalar.arg3, scalar.arg4),
                    ];
                    for advice in advisories {
                        match advice {
                            AllocAdvice::Allocate(pid, vaddr, paddr) => {
                                lru.push(paddr, Some((pid, vaddr)));
                            }
                            AllocAdvice::Free(_pid, _vaddr, paddr) => {
                                lru.pop(&paddr);
                            }
                            _ => { // it's okay to have an unutilized entry
                            }
                        }
                    }
                }
            }
            Some(Opcode::Test0) => {
                // this test routine computes our view of free space from the RPT.
                if let Some(ss) = &sss.inner {
                    let mut total_mem = 0;
                    for &page in ss.rpt.allocs.iter() {
                        if let Some(_pid) = page {
                            total_mem += 4096;
                        }
                    }
                    log::info!("Computed mem: {}kiB", total_mem / 1024);
                }
            }
            // ... todo, other opcodes.
            _ => {
                log::info!("Unknown opcode {:?}", op);
            }
        }
    }
}

fn get_free_mem() -> (usize, usize) {
    match xous::rsyscall(xous::SysCall::SwapOp(SwapAbi::GetFreeMem as usize, 0, 0, 0, 0, 0, 0)) {
        Ok(Result::Scalar5(mem, total, _, _, _)) => (mem, total),
        _ => panic!("GetFreeMem sycall failed"),
    }
}
