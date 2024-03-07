use xous_kernel::SID;

#[cfg(baremetal)]
#[no_mangle]
static mut SWAP: Swap =
    Swap { spt_ptr: 0, smt_base: 0, smt_bounds: 0, rpt_ptr: 0, sid: SID::from_u32(0, 0, 0, 0) };

pub struct Swap {
    /// Pointer to the swap page table base
    spt_ptr: usize,
    /// SMT base and bounds: address meanings can vary depending on the target system,
    /// if swap is memory-mapped, or if behind a SPI register interface.
    smt_base: usize,
    smt_bounds: usize,
    /// Pointer to runtime page tracker
    rpt_ptr: usize,
    /// SID for the swapper
    sid: SID,
}
impl Swap {
    /// Calls the provided function with the current inner process state.
    pub fn with<F, R>(f: F) -> R
    where
        F: FnOnce(&Swap) -> R,
    {
        #[cfg(baremetal)]
        unsafe {
            f(&SWAP)
        }
        #[cfg(not(baremetal))]
        SWAP.with(|ss| f(&ss.borrow()))
    }

    pub fn with_mut<F, R>(f: F) -> R
    where
        F: FnOnce(&mut Swap) -> R,
    {
        #[cfg(baremetal)]
        unsafe {
            f(&mut SWAP)
        }

        #[cfg(not(baremetal))]
        SWAP.with(|ss| f(&mut ss.borrow_mut()))
    }

    pub fn init_from_args(&mut self, args: &crate::args::KernelArguments) -> Result<(), xous_kernel::Error> {
        for tag in args.iter() {
            if tag.name == u32::from_le_bytes(*b"Swap") {
                self.spt_ptr = tag.data[0] as usize;
                self.smt_base = tag.data[1] as usize;
                self.smt_bounds = tag.data[2] as usize;
                self.rpt_ptr = tag.data[3] as usize;
                return Ok(());
            }
        }
        Err(xous_kernel::Error::UseBeforeInit)
    }

    pub fn register_handler(&mut self, s0: u32, s1: u32, s2: u32, s3: u32) {
        assert!(self.sid == SID::from_u32(0, 0, 0, 0), "Swap handler already registered, fatal error!");
        self.sid = SID::from_u32(s0, s1, s2, s3);
    }
}
