#[cfg(feature = "swap")]
pub mod swap;
#[cfg(feature = "swap")]
pub use swap::*;

#[cfg(not(feature = "swap"))]
pub const RAM_SIZE: usize = utralib::generated::HW_SRAM_EXT_MEM_LEN;
#[cfg(feature = "swap")]
// also update in services/xous-susres/src/main.rs @ 157 to adjust where the clean suspend page goes...
// probably should fix that to be linked to this more seamlessly somehow.
pub const RAM_SIZE: usize = 2 * 1024 * 1024;
pub const RAM_BASE: usize = utralib::generated::HW_SRAM_EXT_MEM;

// placeholder for compatibility with other targets
pub fn early_init() -> u32 { 0 }

/// Note that this memory test is "destructive" -- supend/resume will fail if it is enabled
#[cfg(feature = "platform-tests")]
pub fn platform_tests() {
    use crate::*;
    let ram_ptr: *mut u32 = crate::platform::RAM_BASE as *mut u32;
    let sram_ptr: *mut u32 = 0x1000_0000 as *mut u32;
    use utralib::generated::*;

    /* // Status readout block removed to reduce critical path.
    let mut sram_csr = CSR::new(utra::sram_ext::HW_SRAM_EXT_BASE as *mut u32);
    sram_csr.wfo(utra::sram_ext::READ_CONFIG_TRIGGER, 1);

    // give some time for the status to read
    for i in 0..8 {
        unsafe { sram_ptr.add(i).write_volatile(i as u32) };
    }

    println!("status: 0x{:08x}", sram_csr.rf(utra::sram_ext::CONFIG_STATUS_MODE));
    */

    for i in 0..(256 * 1024 / 4) {
        unsafe {
            ram_ptr.add(i).write_volatile((0xACE0_0000 + i) as u32);
        }
    }
    println!("Simple write...");
    let mut errcnt = 0;
    for i in 0..(256 * 1024 / 4) {
        unsafe {
            let rd = ram_ptr.add(i).read_volatile();
            if rd != (0xACE0_0000 + i) as u32 {
                if errcnt < 16 || ((errcnt % 256) == 0) {
                    println!("* 0x{:08x}: e:0x{:08x} o:0x{:08x}", i * 4, 0xACE0_0000 + i, rd);
                }
                errcnt += 1;
            } else if (i & 0x1FFF) == 8 {
                println!("  0x{:08x}: e:0x{:08x} o:0x{:08x}", i * 4, 0xACE0_0000 + i, rd);
            };
        }
    }
    println!("Test random blocks...");
    // let mut seed = 100;
    const START_ADDR: usize = 0x0000_0000; // 3D_0000
    const TESTLEN: usize = 0xC_0000; // 2_0000
    for k in 0..256 {
        println!("Loop {}", k);
        let trng_csr = CSR::new(utra::trng_kernel::HW_TRNG_KERNEL_BASE as *mut u32);
        // fill the top half with random data
        for i in START_ADDR + TESTLEN / 2..START_ADDR + TESTLEN {
            while trng_csr.rf(utra::trng_kernel::URANDOM_VALID_URANDOM_VALID) == 0 {}
            unsafe {
                ram_ptr.add(i).write_volatile(trng_csr.rf(utra::trng_kernel::URANDOM_URANDOM));
            }
            /*
            seed = crate::murmur3::murmur3_32(&[0], seed);
            unsafe {
                ram_ptr.add(i).write_volatile(seed);
            }
            */
        }
        // copy one half to another
        for i in START_ADDR..START_ADDR + TESTLEN / 2 {
            unsafe {
                ram_ptr.add(i).write_volatile(ram_ptr.add(i + TESTLEN / 2).read_volatile());
            }
        }
        // check for copy (write) errors
        let basecnt = errcnt;
        println!("** take one **");
        for i in START_ADDR..START_ADDR + TESTLEN / 2 {
            let rd1 = unsafe { ram_ptr.add(i).read_volatile() };
            let rd2 = unsafe { ram_ptr.add(i + TESTLEN / 2).read_volatile() };
            if rd1 != rd2 {
                if errcnt < 16 + basecnt || ((errcnt % 256) == 0) {
                    println!("* 0x{:08x}: rd1:0x{:08x} rd2:0x{:08x}", i * 4, rd1, rd2);
                }
                errcnt += 1;
            } else if (i & 0x1FFF) == 12 {
                // println!("  0x{:08x}: rd1:0x{:08x} rd2:0x{:08x}", i*4, rd1, rd2);
            }
        }
        // check for again for read errors
        println!("** take two (check for read errors)**");
        let basecnt = errcnt;
        for i in START_ADDR..START_ADDR + TESTLEN / 2 {
            let rd1 = unsafe { ram_ptr.add(i).read_volatile() };
            let rd2 = unsafe { ram_ptr.add(i + TESTLEN / 2).read_volatile() };
            if rd1 != rd2 {
                if errcnt < 16 + basecnt || ((errcnt % 256) == 0) {
                    println!("* 0x{:08x}: rd1:0x{:08x} rd2:0x{:08x}", i * 4, rd1, rd2);
                }
                errcnt += 1;
            } else if (i & 0x1FFF) == 12 {
                // println!("  0x{:08x}: rd1:0x{:08x} rd2:0x{:08x}", i*4, rd1, rd2);
            }
        }
    }

    if errcnt != 0 {
        println!("error count: {}", errcnt);
        println!("0x01000: {:08x}", unsafe { ram_ptr.add(0x1000 / 4).read_volatile() });
        println!("0x00000: {:08x}", unsafe { ram_ptr.add(0x0).read_volatile() });
        println!("0x0FFFC: {:08x}", unsafe { ram_ptr.add(0xFFFC / 4).read_volatile() });
        println!("0xfdff04: {:08x}", unsafe { ram_ptr.add(0xfdff04 / 4).read_volatile() });
    } else {
        println!("No errors detected by memory test.");
    }
}
