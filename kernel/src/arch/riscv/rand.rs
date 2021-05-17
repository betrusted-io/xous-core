// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use crate::mem::MemoryManager;
use utralib::generated::*;
use xous_kernel::{MemoryFlags, MemoryType, PID};

pub const TRNG_KERNEL: Trng = Trng {
    // the HW device mapping is done in xous-rs/src/lib.rs/init()
    // the manually chosen virtual address has to be in the top 4MiB as it is the only page shared among all processes
    base: 0xffce_0000 as *mut usize, // see https://github.com/betrusted-io/xous-core/blob/master/docs/memory.md
};

pub struct Trng {
    pub base: *mut usize,
}

pub fn init() {
    // Map the TRNG so that we can allocate names
    // hardware guarantees that:
    //   - TRNG will automatically power on
    //   - Both TRNGs are enabled, with conservative defaults
    //   - Kernel FIFO will fill with TRNGs such that at least the next 512 calls to get_u32() will succeed without delay
    //   - The kernel will start a TRNG server
    //   - All further security decisions and policies are 100% delegated to this new server.
    MemoryManager::with_mut(|memory_manager| {
        memory_manager
            .map_range(
                utra::trng_kernel::HW_TRNG_KERNEL_BASE as *mut u8,
                ((TRNG_KERNEL.base as u32) & !4095) as *mut u8,
                4096,
                PID::new(1).unwrap(),
                MemoryFlags::R | MemoryFlags::W,
                MemoryType::Default,
            )
            .expect("unable to map TRNG")
    });

    let trng_kernel_csr = CSR::new(TRNG_KERNEL.base as *mut u32);
    while trng_kernel_csr.rf(utra::trng_kernel::STATUS_AVAIL) == 0 {}
    // discard the first entry, as it is always 0x0000_0000
    // this is because the read register is a pipeline stage behind the FIFO
    // once the pipeline has been filled, there is no need to prime it again.
    trng_kernel_csr.rf(utra::trng_kernel::DATA_DATA);
}

pub fn get_u32() -> u32 {
    let trng_kernel_csr = CSR::new(TRNG_KERNEL.base as *mut u32);

    while trng_kernel_csr.rf(utra::trng_kernel::STATUS_AVAIL) == 0 {}

    trng_kernel_csr.rf(utra::trng_kernel::DATA_DATA)
}
