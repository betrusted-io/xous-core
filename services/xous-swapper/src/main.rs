mod debug;
mod platform;
use core::fmt::Write;
use std::fmt::Debug;

use debug::*;
use loader::swap::{SwapSpec, SWAP_CFG_VADDR, SWAP_PT_VADDR, SWAP_RPT_VADDR};
use num_traits::FromPrimitive;
use platform::{SwapHal, PAGE_SIZE};

pub struct PtPage {
    pub entries: [u32; 1024],
}

/// An array of pointers to the root page tables of all the processes.
pub struct SwapPageTables {
    pub roots: &'static mut [PtPage],
}

pub struct RuntimePageTracker {
    pub allocs: &'static mut [u8],
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
}

pub fn change_owner(ss: &mut SwapperSharedState, pid: u8, paddr: usize) {
    // First, check to see if the region is in RAM,
    if paddr >= ss.sram_start && paddr < ss.sram_start + ss.sram_size {
        // Mark this page as in-use by the kernel
        ss.rpt.allocs[(paddr - ss.sram_start as usize) / PAGE_SIZE] = pid;
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
                        SWAP_RPT_VADDR as *mut u8,
                        swap_spec.rpt_len_bytes as usize,
                    )
                },
            },
            sram_start: swap_spec.sram_start as usize,
            sram_size: swap_spec.sram_size as usize,
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
            writeln!(
                DebugUart {},
                "resolved address in swap for 0x{:x}:{} -> 0x{:x?}",
                vaddr_in_pid,
                pid,
                paddr_in_swap
            )
            .ok();
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
            for advice in advisories {
                match advice {
                    xous::AllocAdvice::Allocate(pid, _vaddr, paddr) => {
                        change_owner(ss, pid.get(), paddr);
                    }
                    xous::AllocAdvice::Free(_pid, _vaddr, paddr) => {
                        change_owner(ss, 0, paddr);
                    }
                    xous::AllocAdvice::Uninit => {} // not all the records have to be populated
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
    let mut sss = SharedStateStorage { inner: None };
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
            // ... todo, other opcodes.
            _ => {
                log::info!("Unknown opcode {:?}", op);
            }
        }
    }
}
