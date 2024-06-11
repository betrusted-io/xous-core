#![cfg_attr(not(test), no_main)]
#![cfg_attr(not(test), no_std)]

#[macro_use]
mod args;
use args::{KernelArgument, KernelArguments};

#[cfg_attr(feature = "atsama5d27", path = "platform/atsama5d27/debug.rs")]
mod debug;
mod fonts;
#[cfg(feature = "secboot")]
mod secboot;

#[cfg_attr(feature = "atsama5d27", path = "platform/atsama5d27/asm.rs")]
mod asm;
mod bootconfig;
#[cfg_attr(feature = "atsama5d27", path = "platform/atsama5d27/consts.rs")]
mod consts;
mod minielf;
#[cfg(feature = "resume")]
mod murmur3;
mod phase1;
mod phase2;
mod platform;
#[cfg(feature = "swap")]
pub mod swap;

use core::{mem, ptr, slice};

use asm::*;
use bootconfig::BootConfig;
use consts::*;
pub use loader::*;
use minielf::*;
use phase1::{phase_1, InitialProcess};
use phase2::{phase_2, ProgramDescription};
#[cfg(feature = "swap")]
use platform::SwapHal;

const WORD_SIZE: usize = mem::size_of::<usize>();
pub const SIGBLOCK_SIZE: usize = 0x1000;
const STACK_PAGE_COUNT: usize = 8;

const VDBG: bool = false; // verbose debug
const VVDBG: bool = false; // very verbose debug
const SDBG: bool = false; // swap debug

#[cfg(test)]
mod test;

#[repr(C)]
pub struct MemoryRegionExtra {
    start: u32,
    length: u32,
    name: u32,
    padding: u32,
}

/// A single RISC-V page table entry.  In order to resolve an address,
/// we need two entries: the top level, followed by the lower level.
#[repr(C)]
pub struct PageTable {
    entries: [usize; PAGE_SIZE / WORD_SIZE],
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum IniType {
    // RAM
    IniE,
    // XIP
    IniF,
    // Swap
    IniS,
}

/// Entrypoint
/// This makes the program self-sufficient by setting up memory page assignment
/// and copying the arguments to RAM.
/// Assume the bootloader has already set up the stack to point to the end of RAM.
///
/// # Safety
///
/// This function is safe to call exactly once.
#[export_name = "rust_entry"]
pub unsafe extern "C" fn rust_entry(signed_buffer: *const usize, signature: u32) -> ! {
    //
    // YOU WILL FORGET THIS -- but I haven't figured out a good way to describe this scenario
    // when cross-building a SoC image (for s32 sim) to run on the FPGA variant,
    // you need to comment this out. or else, the simulation will hang. But really, the SoC
    // should be the SoC, the FPGA should be the FPGA...the cross-sim is not really guaranteed to work
    // but it is just a handy tool  to use when it *does* work.
    //
    #[cfg(feature = "cramium-soc")]
    crate::platform::early_init(); // sets up PLLs so we're not running at 16MHz...

    // initially validate the whole image on disk (including kernel args)
    // kernel args must be validated because tampering with them can change critical assumptions about
    // how data is loaded into memory
    #[cfg(feature = "secboot")]
    let mut fs_prehash = [0u8; 64];
    #[cfg(not(feature = "secboot"))]
    let fs_prehash = [0u8; 64];
    #[cfg(feature = "secboot")]
    if !secboot::validate_xous_img(signed_buffer as *const u32, &mut fs_prehash) {
        loop {}
    };
    // the kernel arg buffer is SIG_BLOCK_SIZE into the signed region
    let arg_buffer = (signed_buffer as u32 + SIGBLOCK_SIZE as u32) as *const usize;

    // perhaps later on in these sequences, individual sub-images may be validated
    // against sub-signatures; or the images may need to be re-validated after loading
    // into RAM, if we have concerns about RAM glitching as an attack surface (I don't think we do...).
    // But for now, the basic "validate everything as a blob" is perhaps good enough to
    // armor code-at-rest against front-line patching attacks.
    let kab = KernelArguments::new(arg_buffer);
    boot_sequence(kab, signature, fs_prehash);
}

fn boot_sequence(args: KernelArguments, _signature: u32, fs_prehash: [u8; 64]) -> ! {
    // Store the initial boot config on the stack.  We don't know
    // where in heap this memory will go.
    #[allow(clippy::cast_ptr_alignment)] // This test only works on 32-bit systems
    #[cfg(feature = "platform-tests")]
    platform::platform_tests();

    let mut cfg = BootConfig { base_addr: args.base as *const usize, args, ..Default::default() };
    #[cfg(feature = "swap")]
    println!("Size of BootConfig: {:x}", core::mem::size_of::<BootConfig>());
    read_initial_config(&mut cfg);

    #[cfg(feature = "swap")]
    {
        cfg.swap_hal = SwapHal::new(&cfg);
        read_swap_config(&mut cfg);
    }

    // check to see if we are recovering from a clean suspend or not
    #[cfg(feature = "resume")]
    let (clean, was_forced_suspend, susres_pid) = check_resume(&mut cfg);
    #[cfg(not(feature = "resume"))]
    let clean = {
        // cold boot path
        println!("No suspend marker found, doing a cold boot!");
        #[cfg(feature = "simulation-only")]
        println!("Configured for simulation. Skipping RAM clear!");
        #[cfg(not(feature = "simulation-only"))]
        clear_ram(&mut cfg);
        phase_1(&mut cfg);
        phase_2(&mut cfg, &fs_prehash);
        #[cfg(any(feature = "debug-print", feature = "swap"))]
        if VDBG || SDBG {
            check_load(&mut cfg);
        }
        println!("done initializing for cold boot.");
        false
    };
    #[cfg(feature = "resume")]
    if !clean {
        // cold boot path
        println!("No suspend marker found, doing a cold boot!");
        clear_ram(&mut cfg);
        phase_1(&mut cfg);
        phase_2(&mut cfg, &fs_prehash);
        #[cfg(any(feature = "debug-print", feature = "swap"))]
        if VDBG || SDBG {
            check_load(&mut cfg);
        }
        println!("done initializing for cold boot.");
    } else {
        // resume path
        use utralib::generated::*;
        // flip my self-power-on switch: otherwise, I might turn off before the whole sequence is finished.
        let mut power_csr = CSR::new(utra::power::HW_POWER_BASE as *mut u32);
        power_csr.rmwf(utra::power::POWER_STATE, 1);
        power_csr.rmwf(utra::power::POWER_SELF, 1);

        // TRNG virtual memory mapping already set up, but we pump values out just to make sure
        // the pipeline is fresh. Simulations show this isn't necessary, but I feel paranoid;
        // I worry a subtle bug in the reset logic could leave deterministic values in the pipeline.
        let trng_csr = CSR::new(utra::trng_kernel::HW_TRNG_KERNEL_BASE as *mut u32);
        for _ in 0..4 {
            while trng_csr.rf(utra::trng_kernel::URANDOM_VALID_URANDOM_VALID) == 0 {}
            trng_csr.rf(utra::trng_kernel::URANDOM_URANDOM);
        }
        // turn on the kernel UART.
        let mut uart_csr = CSR::new(utra::uart::HW_UART_BASE as *mut u32);
        uart_csr.rmwf(utra::uart::EV_ENABLE_RX, 1);

        // setup the `susres` register for a resume
        let mut resume_csr = CSR::new(utra::susres::HW_SUSRES_BASE as *mut u32);
        // set the resume marker for the SUSRES server, noting the forced suspend status
        if was_forced_suspend {
            resume_csr.wo(
                utra::susres::STATE,
                resume_csr.ms(utra::susres::STATE_RESUME, 1)
                    | resume_csr.ms(utra::susres::STATE_WAS_FORCED, 1),
            );
        } else {
            resume_csr.wfo(utra::susres::STATE_RESUME, 1);
        }
        resume_csr.wfo(utra::susres::CONTROL_PAUSE, 1); // ensure that the ticktimer is paused before resuming
        resume_csr.wfo(utra::susres::EV_ENABLE_SOFT_INT, 1); // ensure that the soft interrupt is enabled for the kernel to kick
        println!("clean suspend marker found, doing a resume!");

        // trigger the interrupt; it's not immediately handled, but rather checked later on by the kernel on
        // clean resume
        resume_csr.wfo(utra::susres::INTERRUPT_INTERRUPT, 1);
    }

    // condense debug and resume arguments into a single register, so we have space for XPT
    let debug_resume = if cfg.debug { 0x1 } else { 0x0 } | if clean { 0x2 } else { 0x0 };
    if !clean {
        // The MMU should be set up now, and memory pages assigned to their
        // respective processes.
        let krn_struct_start = cfg.sram_start as usize + cfg.sram_size - cfg.init_size + cfg.swap_offset;
        #[cfg(feature = "swap")]
        if SDBG && VDBG {
            const CHUNK_SIZE: usize = 2;
            // activate to debug stack smashes. RPT should be 0's here (or at least valid PIDs) if stack did
            // not overflow.
            for (_i, _r) in cfg.runtime_page_tracker[cfg.runtime_page_tracker.len() - 512..]
                .chunks(CHUNK_SIZE)
                .enumerate()
            {
                println!("  rpt {:08x}: {:02x?}", cfg.runtime_page_tracker.len() - 512 + _i * CHUNK_SIZE, _r);
            }
        }
        // Add a static check for stack overflow, using a heuristic that the last 64 entries of the RPT
        // ought to be a valid PID. A stack smash is likely to write something that does not obey
        // this heuristic within that range (any stack-stored pointer, for example, will break this).
        for &check in cfg.runtime_page_tracker[cfg.runtime_page_tracker.len() - 64..].iter() {
            assert!(
                // use .to_le() to access the structure because SwapAlloc can either be a u8 or a composite
                // type, and .to_le() can do the right thing for both cases.
                check.to_le() <= cfg.processes.len() as u8,
                "RPT looks corrupted, suspect stack overflow in loader. Increase GUARD_MEMORY_BYTES!"
            );
        }
        // compute the virtual addresses of all of these "manually"
        let arg_offset = cfg.args.base as usize - krn_struct_start + KERNEL_ARGUMENT_OFFSET;
        let ip_offset = cfg.processes.as_ptr() as usize - krn_struct_start + KERNEL_ARGUMENT_OFFSET;
        let rpt_offset =
            cfg.runtime_page_tracker.as_ptr() as usize - krn_struct_start + KERNEL_ARGUMENT_OFFSET;
        let xpt_offset = cfg.extra_page_tracker.as_ptr() as usize - krn_struct_start + KERNEL_ARGUMENT_OFFSET;
        #[cfg(not(feature = "atsama5d27"))]
        let _tt_addr = { cfg.processes[0].satp };
        #[cfg(feature = "atsama5d27")]
        let _tt_addr = { cfg.processes[0].ttbr0 };
        println!(
            "Jumping to kernel @ {:08x} with map @ {:08x} and stack @ {:08x} (kargs: {:08x}, ip: {:08x}, rpt: {:08x}, xpt: {:08x})",
            cfg.processes[0].entrypoint,
            _tt_addr,
            cfg.processes[0].sp,
            arg_offset,
            ip_offset,
            rpt_offset,
            xpt_offset,
        );

        // save a copy of the computed kernel registers at the bottom of the page reserved
        // for the bootloader stack. Note there is no stack-smash protection for these arguments,
        // so we're definitely vulnerable to e.g. buffer overrun attacks in the early bootloader.
        //
        // Probably the right long-term solution to this is to turn all the bootloader "loading"
        // actions into "verify" actions during a clean resume (right now we just don't run them),
        // so that attempts to mess with the args during a resume can't lead to overwriting
        // critical parameters like these kernel arguments.
        #[cfg(not(feature = "atsama5d27"))]
        unsafe {
            let backup_args: *mut [u32; 8] = BACKUP_ARGS_ADDR as *mut [u32; 8];
            (*backup_args)[0] = arg_offset as u32;
            (*backup_args)[1] = ip_offset as u32;
            (*backup_args)[2] = rpt_offset as u32;
            (*backup_args)[3] = cfg.processes[0].satp as u32;
            (*backup_args)[4] = cfg.processes[0].entrypoint as u32;
            (*backup_args)[5] = cfg.processes[0].sp as u32;
            (*backup_args)[6] = if cfg.debug { 1 } else { 0 };
            (*backup_args)[7] = xpt_offset as u32;
            #[cfg(feature = "debug-print")]
            {
                if VDBG {
                    println!("Backup kernel args:");
                    for &arg in (*backup_args).iter() {
                        println!("0x{:08x}", arg);
                    }
                }
            }
        }

        #[cfg(not(feature = "atsama5d27"))]
        {
            #[cfg(not(feature = "cramium-soc"))]
            {
                // uart mux only exists on the FPGA variant
                use utralib::generated::*;
                let mut gpio_csr = CSR::new(utra::gpio::HW_GPIO_BASE as *mut u32);
                gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, 1); // patch us over to a different UART for debug (1=LOG 2=APP, 0=KERNEL(hw reset default))
            }

            start_kernel(
                arg_offset,
                ip_offset,
                rpt_offset,
                xpt_offset,
                cfg.processes[0].satp,
                cfg.processes[0].entrypoint,
                cfg.processes[0].sp,
                debug_resume,
            );
        }

        #[cfg(feature = "atsama5d27")]
        unsafe {
            start_kernel(
                cfg.processes[0].sp,
                cfg.processes[0].ttbr0,
                cfg.processes[0].entrypoint,
                arg_offset,
                ip_offset,
                rpt_offset,
                xpt_offset,
                debug_resume,
            )
        }
    } else {
        #[cfg(feature = "resume")]
        unsafe {
            let backup_args: *mut [u32; 8] = BACKUP_ARGS_ADDR as *mut [u32; 8];
            #[cfg(feature = "debug-print")]
            {
                println!("Using backed up kernel args:");
                for &arg in (*backup_args).iter() {
                    println!("0x{:08x}", arg);
                }
            }
            let satp = ((*backup_args)[3] as usize) & 0x803F_FFFF | (((susres_pid as usize) & 0x1FF) << 22);
            //let satp = (*backup_args)[3];
            println!(
                "Adjusting SATP to the sures process. Was: 0x{:08x} now: 0x{:08x}",
                (*backup_args)[3],
                satp
            );

            #[cfg(not(feature = "renode-bypass"))]
            {
                use utralib::generated::*;
                let mut gpio_csr = CSR::new(utra::gpio::HW_GPIO_BASE as *mut u32);
                gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, 0); // patch us over to a different UART for debug (1=LOG 2=APP, 0=KERNEL(default))
            }

            start_kernel(
                (*backup_args)[0] as usize,
                (*backup_args)[1] as usize,
                (*backup_args)[2] as usize,
                (*backup_args)[7] as usize,
                satp as usize,
                (*backup_args)[4] as usize,
                (*backup_args)[5] as usize,
                debug_resume,
            );
        }
        #[cfg(not(feature = "resume"))]
        panic!("Unreachable code executed");
    }
}

pub fn read_initial_config(cfg: &mut BootConfig) {
    let args = cfg.args;
    let mut i = args.iter();
    let xarg = i.next().expect("couldn't read initial tag");
    if xarg.name != u32::from_le_bytes(*b"XArg") || xarg.size != 20 {
        panic!("XArg wasn't first tag, or was invalid size");
    }
    cfg.sram_start = xarg.data[2] as *mut usize;
    cfg.sram_size = xarg.data[3] as usize;
    #[cfg(feature = "swap")]
    if SDBG {
        println!("XAarg // sram_start: {:x}, sram_size: {:x}", cfg.sram_start as usize, cfg.sram_size);
    }

    let mut kernel_seen = false;
    let mut init_seen = false;

    for tag in i {
        if tag.name == u32::from_le_bytes(*b"MREx") {
            cfg.regions = unsafe {
                slice::from_raw_parts(
                    tag.data.as_ptr() as *const MemoryRegionExtra,
                    tag.size as usize / mem::size_of::<MemoryRegionExtra>(),
                )
            };
        } else if tag.name == u32::from_le_bytes(*b"Bflg") {
            let boot_flags = tag.data[0];
            if boot_flags & 1 != 0 {
                cfg.no_copy = true;
            }
            if boot_flags & (1 << 1) != 0 {
                cfg.base_addr = core::ptr::null::<usize>();
            }
            if boot_flags & (1 << 2) != 0 {
                cfg.debug = true;
            }
        } else if tag.name == u32::from_le_bytes(*b"XKrn") {
            assert!(!kernel_seen, "kernel appears twice");
            assert!(tag.size as usize == mem::size_of::<ProgramDescription>(), "invalid XKrn size");
            kernel_seen = true;
        } else if tag.name == u32::from_le_bytes(*b"IniE") || tag.name == u32::from_le_bytes(*b"IniF") {
            assert!(tag.size >= 4, "invalid Init size");
            init_seen = true;
            cfg.init_process_count += 1;
        }
        #[cfg(feature = "swap")]
        if tag.name == u32::from_le_bytes(*b"Swap") {
            // safety: the image creator guarantees this data is aligned and initialized properly
            cfg.swap = Some(unsafe { &*(tag.data.as_ptr() as *const crate::swap::SwapDescriptor) });
        }
    }

    assert!(kernel_seen, "no kernel definition");
    assert!(init_seen, "no initial programs found");
}

#[cfg(feature = "swap")]
pub fn read_swap_config(cfg: &mut BootConfig) {
    // Read in the swap arguments: should be located at beginning of the encrypted image in swap.
    let page0 = cfg.swap_hal.as_mut().unwrap().decrypt_src_page_at(0x0);
    let swap_args = KernelArguments::new(page0.as_ptr() as *const usize);
    for tag in swap_args.iter() {
        if tag.name == u32::from_le_bytes(*b"IniS") {
            assert!(tag.size >= 4, "invalid Init size");
            cfg.init_process_count += 1;
        } else if tag.name == u32::from_le_bytes(*b"XArg") {
            // these are actually specified inside the `Swap` arg, this is just
            // a mirror because this argument is added by default by the image creator
            if SDBG {
                println!("Swap start: {:x}", tag.data[2]);
                println!("Swap size:  {:x}", tag.data[3]);
            }
        } else {
            println!("Unhandled argument in swap: {:x}", tag.name);
        }
    }
}

/// Checks a reserved area of RAM for a pattern with a pre-defined mathematical
/// relationship. The purpose is to detect if we have a "clean suspend", or if
/// we've rebooted from a corrupt/cold RAM state.
#[cfg(feature = "resume")]
fn check_resume(cfg: &mut BootConfig) -> (bool, bool, u32) {
    use utralib::generated::*;
    const WORDS_PER_SECTOR: usize = 128;
    const NUM_SECTORS: usize = 8;
    const WORDS_PER_PAGE: usize = PAGE_SIZE / 4;

    let suspend_marker = cfg.sram_start as usize + cfg.sram_size - GUARD_MEMORY_BYTES;
    let marker: *mut [u32; WORDS_PER_PAGE] = suspend_marker as *mut [u32; WORDS_PER_PAGE];

    let boot_seed = CSR::new(utra::seed::HW_SEED_BASE as *mut u32);
    let seed0 = boot_seed.r(utra::seed::SEED0);
    let seed1 = boot_seed.r(utra::seed::SEED1);
    let was_forced_suspend: bool = if unsafe { (*marker)[0] } != 0 { true } else { false };

    let mut clean = true;
    let mut hashbuf: [u32; WORDS_PER_SECTOR - 1] = [0; WORDS_PER_SECTOR - 1];
    let mut index: usize = 0;
    let mut pid: u32 = 0;
    for sector in 0..NUM_SECTORS {
        for i in 0..hashbuf.len() {
            hashbuf[i] = unsafe { (*marker)[index * WORDS_PER_SECTOR + i] };
        }
        // sector 0 contains the boot seeds, which we replace with our own as read out from our FPGA before
        // computing the hash it also contains the PID of the suspend/resume process manager, which we
        // need to inject into the SATP
        if sector == 0 {
            hashbuf[1] = seed0;
            hashbuf[2] = seed1;
            pid = hashbuf[3];
        }
        let hash = crate::murmur3::murmur3_32(&hashbuf, 0);
        if hash != unsafe { (*marker)[(index + 1) * WORDS_PER_SECTOR - 1] } {
            println!("* computed 0x{:08x} - stored 0x{:08x}", hash, unsafe {
                (*marker)[(index + 1) * (WORDS_PER_SECTOR) - 1]
            });
            clean = false;
        } else {
            println!("  computed 0x{:08x} - match", hash);
        }
        index += 1;
    }
    // zero out the clean suspend marker, so if something goes wrong during resume we don't try to resume
    // again
    for i in 0..WORDS_PER_PAGE {
        unsafe {
            (*marker)[i] = 0;
        }
    }

    (clean, was_forced_suspend, pid)
}

/// Clears all of RAM. This is a must for systems that have suspend-to-RAM for security.
/// It is configured to be skipped in simulation only, to accelerate the simulation times
/// since we can initialize the RAM to zero in simulation.
#[cfg(not(feature = "simulation-only"))]
fn clear_ram(cfg: &mut BootConfig) {
    // clear RAM on a cold boot.
    // RAM is persistent and battery-backed. This means secret material could potentially
    // stay there forever, if not explicitly cleared. This clear adds a couple seconds
    // to a cold boot, but it's probably worth it. Note that it doesn't happen on a suspend/resume.
    let ram: *mut u32 = cfg.sram_start as *mut u32;
    let clear_limit = ((4096 + core::mem::size_of::<BootConfig>()) + 4095) & !4095;
    if VDBG {
        println!("Stack clearing limit: {:x}", clear_limit);
    }
    unsafe {
        for addr in 0..(cfg.sram_size - clear_limit) / 4 {
            // 8k is reserved for our own stack
            ram.add(addr).write_volatile(0);
        }
    }
}

pub unsafe fn bzero<T>(mut sbss: *mut T, ebss: *mut T) {
    if VDBG {
        println!("ZERO: {:08x} - {:08x}", sbss as usize, ebss as usize);
    }
    while sbss < ebss {
        // NOTE(volatile) to prevent this from being transformed into `memclr`
        // which can create an accidental dependency on libc.
        ptr::write_volatile(sbss, mem::zeroed());
        sbss = sbss.offset(1);
    }
}

/// This function allows us to check the final loader results
/// It will print to the console the first 32 bytes of each loaded
/// region top/bottom, based upon extractions from the page table.
#[cfg(any(feature = "debug-print", feature = "swap"))]
fn check_load(cfg: &mut BootConfig) {
    let args = cfg.args;

    // This is the offset in RAM where programs are loaded from.
    println!("\n\nCHECKING LOAD");

    // Go through all Init processes and the kernel, setting up their
    // page tables and mapping memory to them.
    let mut pid = 2;
    for tag in args.iter() {
        if tag.name == u32::from_le_bytes(*b"IniE") {
            let inie = MiniElf::new(&tag);
            println!("\n\nChecking IniE region");
            inie.check(cfg, inie.load_offset as usize, pid, IniType::IniE);
            pid += 1;
        } else if tag.name == u32::from_le_bytes(*b"IniF") {
            let inif = MiniElf::new(&tag);
            println!("\n\nChecking IniF region");
            inif.check(cfg, inif.load_offset as usize, pid, IniType::IniF);
            pid += 1;
        } else if tag.name == u32::from_le_bytes(*b"IniS") {
            let inis = MiniElf::new(&tag);
            println!("\n\nChecking IniS region");
            inis.check(cfg, inis.load_offset as usize, pid, IniType::IniS);
            pid += 1;
        }
    }
}

// Install a panic handler when not running tests.
#[cfg(all(not(test), not(feature = "atsama5d27")))]
mod panic_handler {
    use core::panic::PanicInfo;
    #[panic_handler]
    fn handle_panic(_arg: &PanicInfo) -> ! {
        crate::println!("{}", _arg);
        loop {}
    }
}
