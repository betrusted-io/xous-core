use core::fmt::{Error, Write};

use num_traits::FromPrimitive;
use utralib::*;

/// This trait defines a set of functions to get and receive MACs (message
/// authentication codes, also referred to as the tag in AES-GCM-SIV.
///
/// The implementation of the MAC table depends on the details of the hardware;
/// we cannot always guarantee that the MACs are memory mapped, as they could
/// be stored in an off-chip SPI RAM that is accessible only through a register
/// interface.
pub trait SmtAccessor {
    /// Lookup the MAC corresponding to a given page in swap. Offsets are
    /// relative to the base of the swap region, and are given in units of pages, not bytes.
    fn lookup_mac(swap_page_offset: usize) -> [u8; 16];
    /// Store a MAC for a given page in swap.
    fn store_mac(swap_page_offset: usize, mac: &[u8; 16]);
}

/// An array of pointers to the SATPs (root page table) of all the processes.
pub struct SwapPageTables {
    satps: &'static mut [usize],
}
/// This is an implementation for SMTs that are memory mapped. Directly mapped
/// tables are just as lice of MACs
pub struct SwapMacTableMemMap {
    macs: &'static mut [[u8; 16]],
}
impl SmtAccessor for SwapMacTableMemMap {
    fn lookup_mac(swap_page_offset: usize) -> [u8; 16] { todo!() }

    fn store_mac(swap_page_offset: usize, mac: &[u8; 16]) { todo!() }
}
/// This is an implementation for SMTs that are accessible only through a SPI
/// register interface. The base and bounds must be translated to SPI accesses
/// in a hardware-specific manner.
pub struct SwapMacTableSpi {
    base: usize,
    bounds: usize,
}
impl SmtAccessor for SwapMacTableSpi {
    fn lookup_mac(swap_page_offset: usize) -> [u8; 16] { todo!() }

    fn store_mac(swap_page_offset: usize, mac: &[u8; 16]) { todo!() }
}
pub struct RuntimePageTracker {
    allocs: &'static mut [u8],
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
    /// Kernel message requesting N pages to be swapped out.
    Trim = 3,
    /// Kernel message informing us that we have pages to free.
    Free = 4,
}

pub struct DebugUart {
    #[cfg(feature = "debug-print")]
    csr: CSR<u32>,
}
impl DebugUart {
    #[cfg(feature = "debug-print")]
    pub fn new() -> Self {
        let debug_uart_mem = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::app_uart::HW_APP_UART_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't claim the debug UART");
        let debug_uart = CSR::new(debug_uart_mem.as_mut_ptr() as *mut u32);

        Self { csr: debug_uart }
    }

    #[cfg(feature = "debug-print")]
    pub fn putc(&mut self, c: u8) {
        // Wait until TXFULL is `0`
        while self.csr.r(utra::app_uart::TXFULL) != 0 {}
        self.csr.wfo(utra::app_uart::RXTX_RXTX, c as u32);
    }

    #[cfg(not(feature = "debug-print"))]
    pub fn new() -> Self { Self {} }

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
struct SwapperSharedState {}
impl SwapperSharedState {
    pub fn new() -> Self { Self {} }
}

/// blocking swap call handler
/// 8 argument values are always pushed on the stack; the meaning is bound differently based upon the specific
/// opcode. Not all arguments are used in all cases, unused argument values have no valid meaning (but in
/// practice typically contain the previous call's value, or 0).
fn swap_handler(
    _a0: usize,
    _a1: usize,
    _a2: usize,
    _a3: usize,
    _a4: usize,
    _a5: usize,
    _a6: usize,
    _a7: usize,
) {
    todo!()
}

fn main() {
    // init the log, but this is mostly unused.
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    // swapper is not allowed to use `log` for debugging under most circumstances, because
    // the swapper can't send messages when handling a swap call. Instead, we use a local
    // debug UART to handle this. This needs to be enabled with the "debug-print" feature
    // and is mutually exclusive with the "gdb-stub" feature in the kernel since it uses
    // the same physical hardware.
    let mut duart = DebugUart::new();
    write!(duart, "Swapper started.\n\r").ok();

    let sid = xous::create_server().unwrap();
    let mut swapper_state = SwapperSharedState::new();

    // Register the swapper with the kernel. Written as a raw syscall, since this is
    // the only instance of its use (no point in use-once code to wrap it).
    let (s0, s1, s2, s3) = sid.to_u32();
    let (spt_init, smt_base_init, smt_bounds_init, rpt_init) =
        xous::rsyscall(xous::SysCall::RegisterSwapper(
            s0,
            s1,
            s2,
            s3,
            swap_handler as *mut usize as usize,
            &mut swapper_state as *mut SwapperSharedState as usize,
        ))
        .and_then(|result| {
            if let xous::Result::Scalar5(spt, smt_base, smt_bounds, rpt, _) = result {
                Ok((spt, smt_base, smt_bounds, rpt))
            } else {
                panic!("Failed to register swapper");
            }
        })
        .unwrap();
    // safety: this is only safe because the loader guarantees this raw pointer is initialized and aligned
    // correctly
    let spt = unsafe { &mut *(spt_init as *mut SwapPageTables) };
    #[cfg(feature = "precursor")]
    let smt = SwapMacTableMemMap {
        // safety: this is only safe because the loader guarantees memory-mapped SMT is initialized and
        // aligned and properly mapped into the swapper's memory space.
        macs: unsafe { core::slice::from_raw_parts_mut(smt_base_init as *mut [u8; 16], smt_bounds_init) },
    };
    #[cfg(feature = "cramium-soc")]
    let smt = SwapMacTableSpi { base: smt_base_init, bounds: smt_bounds_init };
    // safety: this is only safe because the loader guarantees this raw pointer is initialized and aligned
    // correctly
    let rpt = unsafe { &mut *(rpt_init as *mut RuntimePageTracker) };

    let mut msg_opt = None;
    loop {
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        let msg = msg_opt.as_mut().unwrap();
        let op: Option<Opcode> = FromPrimitive::from_usize(msg.body.id());
        write!(duart, "Swapper got {:?}", msg).ok();
        match op {
            Some(Opcode::WriteToSwap) => {
                unimplemented!();
            }
            // ... todo, other opcodes.
            _ => {
                write!(duart, "Unknown opcode {:?}", op).ok();
            }
        }
    }
}
