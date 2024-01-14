// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use atsama5d27::trng::{Enabled, StatefulTrng, Trng};
use utralib::HW_TRNG_BASE;
use xous_kernel::{MemoryFlags, MemoryType, PID};

use crate::mem::MemoryManager;

/// The manually chosen virtual address has to be in the top 4MiB as it is the
/// only page shared among all processes.
///
/// See https://github.com/betrusted-io/xous-core/blob/master/docs/memory.md
pub const TRNG_KERNEL_ADDR: usize = 0xffce_0000;
pub static mut TRNG_KERNEL: Option<TrngKernel> = None;

pub struct TrngKernel {
    base_addr: usize,
    pub inner: Option<StatefulTrng<Enabled>>,
}

impl TrngKernel {
    pub fn new(addr: usize) -> TrngKernel { TrngKernel { base_addr: addr, inner: None } }

    pub fn init(&mut self) { self.inner = Some(Trng::with_alt_base_addr(self.base_addr as u32).enable()); }

    pub fn get_u32(&mut self) -> u32 {
        if let Some(trng) = &self.inner {
            return trng.read_u32();
        }

        unreachable!()
    }
}

/// Initialize TRNG driver.
///
/// Needed so that the kernel can allocate names.
///
/// # Panics
///
/// If `pmc::init()` hasn't called prior to calling this function.
pub fn init() {
    // PMC driver should be initialized before.
    super::pmc::enable_trng();

    // Map the TRNG so that we can allocate names
    // hardware guarantees that:
    //   - TRNG will automatically power on
    //   - Both TRNGs are enabled, with conservative defaults
    //   - Kernel FIFO will fill with TRNGs such that at least the next 512 calls to get_u32() will succeed
    //     without delay
    //   - The kernel will start a TRNG server
    //   - All further security decisions and policies are 100% delegated to this new server.
    MemoryManager::with_mut(|memory_manager| {
        memory_manager
            .map_range(
                HW_TRNG_BASE as *mut u8,
                (TRNG_KERNEL_ADDR & !4095) as *mut u8,
                0x4000, // 16K
                PID::new(1).unwrap(),
                MemoryFlags::R | MemoryFlags::W | MemoryFlags::DEV,
                MemoryType::Default,
            )
            .expect("unable to map TRNG_KERNEL")
    });

    let mut trng_kernel = TrngKernel::new(TRNG_KERNEL_ADDR);
    trng_kernel.init();

    unsafe {
        TRNG_KERNEL = Some(trng_kernel);
    }
}

/// Retrieve random `u32`.
pub fn get_u32() -> u32 {
    unsafe { TRNG_KERNEL.as_mut().expect("TRNG_KERNEL driver not initialized").get_u32() }
}
