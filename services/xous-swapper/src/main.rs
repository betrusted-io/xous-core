mod platform;

use core::fmt::{Error, Write};
use core::sync::atomic::{AtomicUsize, Ordering};

use loader::swap::{SwapSpec, SWAP_CFG_VADDR, SWAP_PT_VADDR, SWAP_RPT_VADDR};
use num_traits::FromPrimitive;
use platform::SwapMac;
use utralib::*;

static DUART_CSR_PA: AtomicUsize = AtomicUsize::new(0);

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

pub struct DebugUart {}
impl DebugUart {
    #[cfg(feature = "debug-print")]
    pub fn putc(&mut self, c: u8) {
        let mut csr_ptr = DUART_CSR_PA.load(Ordering::SeqCst);
        // map it if it doesn't exist
        if csr_ptr == 0 {
            let debug_uart_mem = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::app_uart::HW_APP_UART_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't claim the debug UART");
            DUART_CSR_PA.store(csr_ptr, Ordering::SeqCst);
            csr_ptr = debug_uart_mem.as_ptr() as usize;
        }
        let mut csr = CSR::new(csr_ptr as *mut u32);

        // Wait until TXFULL is `0`
        while csr.r(utra::app_uart::TXFULL) != 0 {}
        csr.wfo(utra::app_uart::RXTX_RXTX, c as u32);
    }

    #[cfg(not(feature = "debug-print"))]
    pub fn putc(&self, _c: u8) {}
}

impl Write for DebugUart {
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        for c in s.bytes() {
            self.putc(c);
        }
        Ok(())
    }
}

/// This structure contains shared state accessible between the userspace code and the blocking swap call
/// handler.
struct SwapperSharedState {
    pub key: [u8; 32],
    pub pts: SwapPageTables,
    pub macs: SwapMac,
    pub rpt: RuntimePageTracker,
}
struct SharedStateStorage {
    pub inner: Option<SwapperSharedState>,
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

        // extract our key
        let mut key = [0u8; 32];
        key.copy_from_slice(&swap_spec.key);
        // swapper is not allowed to use `log` for debugging under most circumstances, because
        // the swapper can't send messages when handling a swap call. Instead, we use a local
        // debug UART to handle this. This needs to be enabled with the "debug-print" feature
        // and is mutually exclusive with the "gdb-stub" feature in the kernel since it uses
        // the same physical hardware.
        sss.inner = Some(SwapperSharedState {
            key,
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
            macs: SwapMac::new(swap_spec.mac_base as usize, swap_spec.mac_len as usize),
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
                "rfs PID{}, vaddr_pid {:x}, vaddr_swap {:x}",
                pid,
                vaddr_in_pid,
                vaddr_in_swap
            )
            .ok();
            // walk the PT to find the swap data
            // ss.pts[pid as usize]
        }
        Some(Opcode::AllocateAdvisory) => {
            let advisories = [
                xous::AllocAdvice::deserialize(a2, a3),
                xous::AllocAdvice::deserialize(a4, a5),
                xous::AllocAdvice::deserialize(a6, a7),
            ];
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

    // check that we got called by the kernel -- this should happen really early in the process
    // due to the advisory allocs that start piling up very quickly
    if let Some(ss) = sss.inner {
        log::info!(
            "Swap params: key: {:x?}, root: {:x}, rpt: {:?}",
            ss.key,
            ss.pts.roots[0].entries[0],
            &ss.rpt.allocs[..8]
        );
    } else {
        log::info!("Swapper did not get called to initialize its shared state.")
    }

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
