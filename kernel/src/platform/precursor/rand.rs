// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use crate::mem::MemoryManager;
use utralib::generated::*;
use xous_kernel::{MemoryFlags, MemoryType, PID};

/// The manually chosen virtual address has to be in the top 4MiB as it is the
/// only page shared among all processes.
///
/// See https://github.com/betrusted-io/xous-core/blob/master/docs/memory.md
pub const TRNG_KERNEL_ADDR: usize = 0xffce_0000;
pub static mut TRNG_KERNEL: Option<TrngKernel> = None;

pub struct TrngKernel {
    pub trng_kernel_csr: CSR<u32>,
}

impl TrngKernel {
    pub fn new(addr: usize) -> TrngKernel {
        TrngKernel {
            trng_kernel_csr: CSR::new(addr as *mut u32),
        }
    }

    pub fn init(&mut self) {
        if false {
            // raw random path - left in for debugging urandom, can strip out later
            while self.trng_kernel_csr.rf(utra::trng_kernel::STATUS_AVAIL) == 0 {}
            // discard the first entry, as it is always 0x0000_0000
            // this is because the read register is a pipeline stage behind the FIFO
            // once the pipeline has been filled, there is no need to prime it again.
            self.trng_kernel_csr.rf(utra::trng_kernel::DATA_DATA);
        } else {
            // urandom path (recommended)
            // simulations show this isn't strictly necessary, but I prefer to have it
            // just in case a subtle bug in the reset logic leaves something deterministic
            // in the connecting logic: the simulation coverage stops at the edge of the TRNG block.
            for _ in 0..4 {
                // wait until the urandom port is initialized
                while self
                    .trng_kernel_csr
                    .rf(utra::trng_kernel::URANDOM_VALID_URANDOM_VALID)
                    == 0
                {}
                // pull a dummy piece of data
                self.trng_kernel_csr.rf(utra::trng_kernel::URANDOM_URANDOM);
            }
        }
    }

    pub fn get_u32(&mut self) -> u32 {
        if false {
            // raw random path
            while self.trng_kernel_csr.rf(utra::trng_kernel::STATUS_AVAIL) == 0 {}
            self.trng_kernel_csr.rf(utra::trng_kernel::DATA_DATA)
        } else {
            // urandom path (recommended)
            while self
                .trng_kernel_csr
                .rf(utra::trng_kernel::URANDOM_VALID_URANDOM_VALID)
                == 0
            {}
            self.trng_kernel_csr.rf(utra::trng_kernel::URANDOM_URANDOM)
        }
    }
}

/// Initialize TRNG driver.
///
/// Needed so that the kernel can allocate names.
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
                (TRNG_KERNEL_ADDR & !4095) as *mut u8,
                4096,
                PID::new(1).unwrap(),
                MemoryFlags::R | MemoryFlags::W,
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
    unsafe {
        TRNG_KERNEL
            .as_mut()
            .expect("TRNG_KERNEL driver not initialized")
            .get_u32()
    }
}
