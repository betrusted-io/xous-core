use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::sleep;
use std::time::Duration;

use log::info;

// put here to force a .sbss/.bss section for loader testing
static mut LOOP_COUNT: AtomicU32 = AtomicU32::new(0);

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    info!("my PID is {}", xous::process::id());

    let new_limit = 10 * 1024 * 1024;
    let result =
        xous::rsyscall(xous::SysCall::AdjustProcessLimit(xous::Limits::HeapMaximum as usize, 0, new_limit));

    if let Ok(xous::Result::Scalar2(1, current_limit)) = result {
        xous::rsyscall(xous::SysCall::AdjustProcessLimit(
            xous::Limits::HeapMaximum as usize,
            current_limit,
            new_limit,
        ))
        .unwrap();
        log::info!("Heap limit increased to: {}", new_limit);
    } else {
        panic!("Unsupported syscall!");
    }

    log::info!("start spi flash map test");
    let mut spimap = xous::map_memory(
        None,
        xous::MemoryAddress::new(xous::arch::MMAP_VIRT_BASE),
        // should normally be cramium_hal::board::SPINOR_LEN
        16 * 1024 * 1024,
        xous::MemoryFlags::R | xous::MemoryFlags::W | xous::MemoryFlags::VIRT,
    )
    .expect("couldn't map spi range");
    log::info!("spimap: {:x?}", spimap);
    // cache_flush();
    // we have the mapping, now try to dereference it and read something to test it
    let spislice: &mut [u32] = unsafe { spimap.as_slice_mut() };
    log::info!("spislice ptr: {:x}", spislice.as_ptr() as usize);
    // this should trigger the page fault and thus the handler
    log::info!("spislice @ 0: {:x?}", &spislice[..32]);
    log::info!("spislice @ 16384: {:x?}", &spislice[16384..16384 + 32]);
    log::info!("spislice @ 8190: {:x?}", &spislice[8190..8190 + 32]);

    // poke some data into the slice for flushing
    log::info!("writing data");
    for (i, d) in spislice[4096 / size_of::<u32>()..(4096 / size_of::<u32>()) + 4].iter_mut().enumerate() {
        *d = i as u32 | 0x1111_0000;
    }
    log::info!("wrote: {:x?}", &spislice[4096 / size_of::<u32>()..(4096 / size_of::<u32>()) + 4]);
    // marks modified pages as dirty.
    xous_swapper::mark_dirty(&spislice[4096 / size_of::<u32>()..(4096 / size_of::<u32>()) + 4]);
    log::info!("pages marked as dirty; now doing sync");

    // Calls sync to explicitly flush the dirty pages now
    xous_swapper::sync(Some(&spislice[4096 / size_of::<u32>()..(4096 / size_of::<u32>()) + 4]));
    // xous_swapper::sync::<u8>(None);
    log::info!("sync call done");

    log::info!("end spi flash map test");

    const DELAY_MS: u64 = 1000;

    for i in 0.. {
        unsafe { LOOP_COUNT.store(i, Ordering::SeqCst) };
        info!("Loop #{}, waiting {} ms", unsafe { LOOP_COUNT.load(Ordering::SeqCst) }, DELAY_MS);
        sleep(Duration::from_millis(DELAY_MS));

        const TEST_SIZE: usize = 800 * 1024; // 950 really stresses things, as nothing fits
        if i == 8 || i == 17 {
            log::info!("allocating big_vec");
            let mut big_vec = Vec::with_capacity(TEST_SIZE);
            log::info!("big_vec len: {}, capacity: {}", big_vec.len(), big_vec.capacity());
            for j in 0..TEST_SIZE {
                big_vec.push(j);
            }
            log::info!("after init big_vec len: {}, capacity: {}", big_vec.len(), big_vec.capacity());
            log::info!("allocating copy_vec");
            let mut copy_vec = vec![0usize; TEST_SIZE];
            log::info!("copy_vec len: {}, capacity: {}", copy_vec.len(), copy_vec.capacity());
            copy_vec.copy_from_slice(&big_vec);
            log::info!("copy_vec copied: {}, capacity: {}", copy_vec.len(), copy_vec.capacity());
            let mut checksum1 = 0;
            let mut checksum2 = 0;
            log::info!("computing checksums");
            for &v in big_vec.iter() {
                checksum1 += v;
            }
            for &v in copy_vec.iter() {
                checksum2 += v;
            }
            log::info!("check 1 {}, check 2 {}", checksum1, checksum2);
            assert!(checksum1 == checksum2);
            log::info!("adding vecs");
            let mut final_check = 0;
            for (&v, &w) in big_vec.iter().zip(copy_vec.iter()) {
                final_check = final_check + v + w;
            }
            log::info!("final check {}, checksum1+checksum2 {}", final_check, checksum1 + checksum2);
            assert!(final_check == checksum1 + checksum2);
        }
    }

    panic!("Finished endless loop");
}

#[allow(dead_code)]
fn cache_flush() {
    unsafe {
        // let the write go through before continuing
        #[rustfmt::skip]
        core::arch::asm!(
            ".word 0x500F",
            "nop",
            "nop",
            "nop",
            "nop",
            "fence",
            "nop",
            "nop",
            "nop",
            "nop",
        );
    }
}
