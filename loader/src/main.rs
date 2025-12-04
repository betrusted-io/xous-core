#![cfg_attr(not(test), no_main)]
#![cfg_attr(not(test), no_std)]

// Vex-II porting notes:
//
// Vex-II is currently supported as a "testing" stub. Compatibility to Vex-I is maintained
// through the "legacy-int" shim, which implements Vex-I style litex interrupt handling and
// masking through a CSR.
//
// ** Interrupts **
// The final version of Vex-II will introduce a PLIC. This will sit in between the litex
// interrupt controller and be largely a pass-through in Xous, but is provided so that other
// OSes which assume PLIC can port more easily to the SoC. This means that the stubs in
// kernel/src/arch/riscv/irq.rs (sim_read(), sim_write, sip_read()) will need to be re-written
// to either setup the PLIC and forget it or work the PLIC directly into the handler loop.
// Tasks:
//   -[ ] Add PLIC into Vex-ii implementation
//   -[ ] Refactor kernel/src/arch/riscv/irq.rs to handle this
//
// ** MMU **
// Vex-II includes support for -A and -D flags in the MMU. The current work-around in Xous is
// to simply set A for all R pages, and D for all W pages. Instead, A/D should be cleared
// in the loader, and inside the OS when a page is "read" but "A" is missing, A should be
// set and the read is retried. Likewise, when a page is "written" but "D" is missing,
// "D" should be set and the write retried. This implementation will allow the MMU to skip
// writing pages out to swap that are clean, and to also pick only pages that have not been
// accessed to consider as candidates for swapping out.
// Tasks:
//   -[ ] Remove A/D "on by default" in loader/src/phase2.rs/ProgramDescription/Load() - multiple locations
//   -[ ] Remove A/D "on by default" in kernel/src/arch/riscv/mem.rs/translate_flags()
//   -[ ] Add handler for A/D trap inside
//        kernel/src/arch/riscv/irq.rs/trap_handler/RiscvException::StoragePageFault | LoadPage Fault
//   -[ ] Update swap handler to use the A/D features - not sure exact files, need to research this
//
// ** No curve25519 engine **
// To make space for the Vex-II core the curve25519 engine was gutted. This means there is no signature
// verification in the boot process. For the final Denarius implementation, this won't be a big impact
// because there is crypto hardware there on the SoC, but for Precursor validation model this means a lot
// of the L1 bootloader is stubbed out, and some stub code is put into the loader to "fake" the items
// now missing due to this omission. These are clearly delineated with a "vexii-test" fetaure flag.
//
// ** Clocking differences & TRNG **
// Vex-II on Precursor runs at 50MHz. This means that all of the other dependent clocks also have
// to down-clock. This breaks XADC and causes the TrngManager to fail test. Since this is a temporary
// config, the SoC bypasses the self-test and just causes the system to run in a more or less deterministic
// mode of operation. The TRNG will come from a different source on Denarius.
//
// ** Cache flush **
// Vex-II implements cmo.flush instructions. This means that the flush routine inside SPINOR needs
// to be handled differently: flushes need to address a specific address to flush.
//   -[ ] services/spinor/src/lib.rs/Spinor/patch() has multiple locations where the API is modified
//        to allow a specific virtual address to be pushed back to the flush routine.
//
// ** AES **
// Vex-II implements compliant AES instructions to the -Zkn standards (unlike Vex-I).
// The AES library has flags that recognize the "vexii-testing" feature, and implements acceleration
// for AES-256 (but falls back to software for AES-128). This will need to be revisited regardless
// for the bao1x/bao1x bring-up.
//
// ** Testing features **
//   -[ ] libs/precursor/hal/src/board/precursors.rs - PDDB_LEN is shortened for vexii-test target

extern crate alloc;

#[macro_use]
mod args;
use args::{KernelArgument, KernelArguments};
#[cfg(feature = "bao1x")]
use bao1x_api::UUID;

#[cfg_attr(feature = "atsama5d27", path = "platform/atsama5d27/debug.rs")]
mod debug;
mod fonts;
#[cfg(all(feature = "secboot", not(feature = "vexii-test")))]
mod secboot;

#[cfg_attr(feature = "atsama5d27", path = "platform/atsama5d27/asm.rs")]
mod asm;
mod bootconfig;
#[cfg_attr(feature = "atsama5d27", path = "platform/atsama5d27/consts.rs")]
mod consts;
mod env;
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
#[cfg(feature = "bao1x")]
use bao1x_hal::board::{BOOKEND_END, BOOKEND_START};
use bootconfig::BootConfig;
use consts::*;
pub use loader::*;
use minielf::*;
use phase1::{InitialProcess, phase_1};
use phase2::{ProgramDescription, phase_2};
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

// Bao1x needs a heap because the signature checking code assumes its availability.
// However the heap promises not to allocate any objects that are needed by the kernel,
// so we stick it in a region in low RAM, just about the statics, and restrict it to
// just 4k in size. Note that maybe we should strive to avoid the loader from allocating
// into these regions - but actually, later in the loader, we should avoid using the heaps
// and most data is done with, and it's nice to be able to reclaim these pages, so we
// somewhat dangerously tell the loader to go ahead and allocate over this region by
// telling it has the whole rest of SRAM to stick initial process pages in.
#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();
// heap start is selected by looking at the total reserved .data + .bss region in the compiled loader.
// it hovers at 0x5000 unless I start adding lots of statics to the loader (which there are not).
pub const HEAP_OFFSET: usize = 0x5000;
// just a small heap, big enough for us to use alloc to simplify argument processing
pub const HEAP_LEN: usize = 0x1000;

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
    let perclk_freq = crate::platform::early_init(); // sets up PLLs so we're not running at 16MHz...
    // need to make this "official" for bao1x, the feature flag combo below works around some simulation
    // config conflicts.
    #[cfg(all(feature = "verilator-only", not(feature = "bao1x-mpw")))]
    platform::coreuser_config();

    // initially validate the whole image on disk (including kernel args)
    // kernel args must be validated because tampering with them can change critical assumptions about
    // how data is loaded into memory
    #[cfg(all(feature = "secboot", not(feature = "vexii-test")))]
    let mut fs_prehash = [0u8; 64];
    #[cfg(not(all(feature = "secboot", not(feature = "vexii-test"))))]
    let fs_prehash = [0u8; 64];
    #[cfg(all(feature = "secboot", not(feature = "vexii-test")))]
    if !secboot::validate_xous_img(signed_buffer as *const u32, &mut fs_prehash) {
        loop {}
    };

    #[cfg(all(feature = "vexii-test", not(feature = "verilator-only")))]
    {
        // fake the prehash
        use ed25519_dalek_loader::Digest;
        #[repr(C)]
        struct SignatureInFlash {
            pub version: u32,
            pub signed_len: u32,
            pub signature: [u8; 64],
        }
        let sig_ptr = signed_buffer as *const SignatureInFlash;
        let sig: &SignatureInFlash = unsafe { sig_ptr.as_ref().unwrap() };
        let signed_len = sig.signed_len;
        let image: &[u8] = unsafe {
            core::slice::from_raw_parts(
                (signed_buffer as usize + SIGBLOCK_SIZE) as *const u8,
                signed_len as usize,
            )
        };
        let mut h: sha2_loader::Sha512 = sha2_loader::Sha512::new();
        h.update(&image);
        let prehash = h.finalize();
        fs_prehash.copy_from_slice(prehash.as_slice());

        // ensure the crypto unit is on - necessary for TRNG to operate
        let mut power = utralib::CSR::new(utralib::utra::power::HW_POWER_BASE as *mut u32);
        power.rmwf(utralib::utra::power::POWER_CRYPTO_ON, 1);
        let mut trng = utralib::CSR::new(utralib::utra::trng_server::HW_TRNG_SERVER_BASE as *mut u32);
        trng.wo(utralib::utra::trng_server::CONTROL, 0x5); // disable avalanche generator

        // debug output on TRNG unit.
        println!(
            "trng: cha/{:x} dat/{:x} sta/{:x}",
            trng.r(utralib::utra::trng_server::CHACHA),
            trng.r(utralib::utra::trng_server::DATA),
            trng.r(utralib::utra::trng_server::STATUS)
        );
        let trngk = utralib::utra::trng_kernel::HW_TRNG_KERNEL_BASE as *const u32;
        for i in 0..utralib::utra::trng_kernel::TRNG_KERNEL_NUMREGS {
            println!("trngk{}: {:x}", i, trngk.add(i).read_volatile())
        }
        let trngs = utralib::utra::trng_server::HW_TRNG_SERVER_BASE as *const u32;
        for i in 0..utralib::utra::trng_server::TRNG_SERVER_NUMREGS {
            println!("trngs{}: {:x}", i, trngs.add(i).read_volatile())
        }

        // allow cache flushes to be initiated from supervisor and userspace
        // I guess this creates a potential sidechannel, but we're not a multitenant
        // threat model - if someone is running code on the SoC that can trigger
        // cache flushes to exfil data, we've got bigger problems...?
        core::arch::asm!(
            "li   t0, 0x30 | 0x40", // XENVCFG_CBIE_OK | XENVCFG_CBCFE_OK
            "csrw 0x30a, t0", // allow supervisor
            "csrw 0x10a, t0", // allow user
            out("t0") _, // clobber t0
        );
    }

    // Initialize the allocator with heap memory range. The heap memory is "throw-away"
    // so we stick it near the bottom of RAM, with the assumption that the loader process
    // won't smash over it.
    #[cfg(feature = "bao1x")]
    {
        // heap is needed for bao1x-boot because signature depends on it
        let heap_start = utralib::HW_SRAM_MEM + HEAP_OFFSET;
        println!("Setting up heap @ {:x}-{:x}", heap_start, heap_start + HEAP_LEN);
        unsafe {
            ALLOCATOR.lock().init(heap_start as *mut u8, HEAP_LEN);
        }
    }

    #[cfg(feature = "bao1x")]
    let mut csprng = bao1x_hal::hardening::Csprng::new();
    #[cfg(feature = "bao1x")]
    csprng.random_delay();
    #[cfg(feature = "bao1x")]
    // it's security-important to ensure we're running off the PLL
    bao1x_hal::hardening::check_pll();

    // Run kernel image validation now that the heap is set up.
    #[cfg(feature = "bao1x")]
    let detached_app = {
        use bao1x_api::signatures::FunctionCode;
        // validate using the bao1x signature scheme
        match bao1x_hal::sigcheck::validate_image(
            bao1x_api::KERNEL_START as *const u32,
            bao1x_api::LOADER_START as *const u32,
            bao1x_api::LOADER_REVOCATION_OFFSET,
            &[
                FunctionCode::Kernel as u32,
                FunctionCode::UpdatedKernel as u32,
                FunctionCode::Developer as u32,
            ],
            false,
            None,
            Some(&mut csprng),
        ) {
            Ok((k, k2, tag)) => {
                println!(
                    "*** Kernel signature check by key @ {}/{}({}) OK ***",
                    k,
                    k2,
                    core::str::from_utf8(&tag).unwrap_or("invalid tag")
                );
                if k != k2 {
                    bao1x_hal::sigcheck::die_no_std();
                }
                // k is just a nominal slot number. If either match, assume we are dealing with a
                // developer image.
                if tag == *bao1x_api::pubkeys::KEYSLOT_INITIAL_TAGS[bao1x_api::pubkeys::DEVELOPER_KEY_SLOT]
                    || k == bao1x_api::pubkeys::DEVELOPER_KEY_SLOT
                {
                    // we can't erase keys in the loader, because the keys have already been locked
                    // out at this point. Thus, ensure that the system is already in developer mode.
                    let owc = bao1x_hal::acram::OneWayCounter::new();
                    if owc.get(bao1x_api::DEVELOPER_MODE).unwrap() == 0 {
                        println!("{}LOADER.KERNDIE,{}", BOOKEND_START, BOOKEND_END);
                        println!("Kernel is devkey signed, but system is not in developer mode. Dying!");
                        bao1x_hal::sigcheck::die_no_std();
                    } else {
                        println!("{}LOADER.KERNDEV,{}", BOOKEND_START, BOOKEND_END);
                        println!("Developer key detected on kernel. Proceeding in developer mode!");
                    }
                }
            }
            Err(e) => {
                println!("Kernel failed signature check. Dying: {:?}", e);
                println!("{}LOADER.KERNFAIL,{}", BOOKEND_START, BOOKEND_END);
                bao1x_hal::sigcheck::die_no_std();
            }
        }

        let one_way = bao1x_hal::acram::OneWayCounter::new();
        if one_way.get_decoded::<bao1x_api::BoardTypeCoding>().expect("Board type coding error")
            == bao1x_api::BoardTypeCoding::Dabao
        {
            match bao1x_hal::sigcheck::validate_image(
                bao1x_api::offsets::dabao::APP_RRAM_START as *const u32,
                bao1x_api::LOADER_START as *const u32,
                bao1x_api::LOADER_REVOCATION_OFFSET,
                &[FunctionCode::App as u32, FunctionCode::UpdatedApp as u32],
                false,
                None,
                Some(&mut csprng),
            ) {
                Ok((k, k2, tag)) => {
                    println!(
                        "*** Detached app signature check by key @ {}/{}({}) OK ***",
                        k,
                        k2,
                        core::str::from_utf8(&tag).unwrap_or("invalid tag")
                    );
                    // k is just a nominal slot number. If either match, assume we are dealing with a
                    // developer image.
                    if k != k2 {
                        bao1x_hal::sigcheck::die_no_std();
                    }
                    if tag
                        == *bao1x_api::pubkeys::KEYSLOT_INITIAL_TAGS[bao1x_api::pubkeys::DEVELOPER_KEY_SLOT]
                        || k == bao1x_api::pubkeys::DEVELOPER_KEY_SLOT
                    {
                        // we can't erase keys in the loader, because the keys have already been locked
                        // out at this point. Thus, ensure that the system is already in developer mode.
                        let owc = bao1x_hal::acram::OneWayCounter::new();
                        if owc.get(bao1x_api::DEVELOPER_MODE).unwrap() == 0 {
                            println!("{}LOADER.APPDIE,{}", BOOKEND_START, BOOKEND_END);
                            println!(
                                "Detached app is devkey signed, but system is not in developer mode. Dying!"
                            );
                            bao1x_hal::sigcheck::die_no_std();
                        } else {
                            println!("{}LOADER.APPDEV,{}", BOOKEND_START, BOOKEND_END);
                            println!("Developer key detected on detached app. Proceeding in developer mode!");
                        }
                    }
                    true
                }
                Err(_e) => {
                    println!("{}LOADER.APPFAIL,{}", BOOKEND_START, BOOKEND_END);
                    println!("No valid detached app found");
                    false
                }
            }
        } else {
            // no detached apps on other boards
            false
        }
    };

    #[cfg(feature = "bao1x")]
    {
        // Follow up the mesh check in loader - it takes 100ms or so for the mesh to settle, we can't afford
        // to wait that long in boot1.
        let one_way = bao1x_hal::acram::OneWayCounter::new();
        bao1x_hal::hardening::mesh_check_and_react(&mut csprng, &one_way);
    }

    #[cfg(not(feature = "bao1x"))]
    let detached_app = false;

    // the kernel arg buffer is SIG_BLOCK_SIZE into the signed region
    #[cfg(not(feature = "bao1x"))]
    let signature_size = SIGBLOCK_SIZE;
    #[cfg(feature = "bao1x")]
    let signature_size = bao1x_api::signatures::SIGBLOCK_LEN;
    let arg_buffer = (signed_buffer as u32 + signature_size as u32) as *const usize;
    println!("arg_buffer: {:x}", arg_buffer as usize);

    // perhaps later on in these sequences, individual sub-images may be validated
    // against sub-signatures; or the images may need to be re-validated after loading
    // into RAM, if we have concerns about RAM glitching as an attack surface (I don't think we do...).
    // But for now, the basic "validate everything as a blob" is perhaps good enough to
    // armor code-at-rest against front-line patching attacks.
    let kab = KernelArguments::new(arg_buffer);
    boot_sequence(kab, signature, fs_prehash, perclk_freq, detached_app);
}

fn boot_sequence(
    args: KernelArguments,
    _signature: u32,
    fs_prehash: [u8; 64],
    _perclk_freq: u32,
    detached_app: bool,
) -> ! {
    // Store the initial boot config on the stack.  We don't know
    // where in heap this memory will go.
    #[allow(clippy::cast_ptr_alignment)] // This test only works on 32-bit systems
    let mut cfg = BootConfig { base_addr: args.base as *const usize, args, ..Default::default() };
    #[cfg(feature = "swap")]
    println!("Size of BootConfig: {:x}", core::mem::size_of::<BootConfig>());
    read_initial_config(&mut cfg);

    #[cfg(feature = "swap")]
    {
        cfg.swap_hal = SwapHal::new(&cfg, _perclk_freq);
        read_swap_config(&mut cfg);
    }
    if detached_app {
        read_detached_app_config(&mut cfg);
    }

    // check to see if we are recovering from a clean suspend or not
    #[cfg(feature = "resume")]
    let (clean, was_forced_suspend, susres_pid) = check_resume(&mut cfg);
    #[cfg(not(feature = "resume"))]
    let clean = false;
    if !clean {
        #[cfg(not(feature = "bao1x"))]
        {
            // setup heap so we can make env. It's not set up earlier in precursor environment
            // because it's not safe to smash memory on a resume
            let heap_start = utralib::HW_SRAM_EXT_MEM + HEAP_OFFSET;
            // for precursor, clear this region, as it is only cleared later in the boot process
            let ram_init = utralib::HW_SRAM_EXT_MEM as *mut u32;
            for i in 0..(HEAP_LEN + HEAP_OFFSET) / size_of::<u32>() {
                unsafe { ram_init.add(i).write_volatile(0) };
            }
            println!("Setting up heap @ {:x}-{:x}", heap_start, heap_start + HEAP_LEN);
            unsafe {
                ALLOCATOR.lock().init(heap_start as *mut u8, HEAP_LEN);
            }
        }
        // build the environment variables - requires heap
        let mut env_variables = crate::env::EnvVariables::new();
        env_variables.add_var("ROOT_FILESYSTEM_HASH", &crate::env::to_hex_ascii(&fs_prehash));

        #[cfg(feature = "bao1x")]
        {
            let owc = bao1x_hal::acram::OneWayCounter::new();
            let slot_mgr = bao1x_hal::acram::SlotManager::new();
            let sn = bao1x_hal::usb::derive_usb_serial_number(&owc, &slot_mgr);
            env_variables.add_var("PUBLIC_SERIAL", &sn);
            let hex_uuid = hex::encode(slot_mgr.read(&UUID).unwrap());
            env_variables.add_var("UUID", &hex_uuid);
        }

        // cold boot path
        println!("No suspend marker found, doing a cold boot!");
        clear_ram(&mut cfg);
        phase_1(&mut cfg, detached_app);
        phase_2(&mut cfg, env_variables);
        #[cfg(any(feature = "debug-print", feature = "swap"))]
        if VDBG || SDBG {
            check_load(&mut cfg);
        }
        println!("done initializing for cold boot.");
    }
    #[cfg(feature = "resume")]
    if clean {
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
            #[cfg(not(any(feature = "bao1x")))]
            {
                // uart mux only exists on the FPGA variant
                use utralib::generated::*;
                let mut gpio_csr = CSR::new(utra::gpio::HW_GPIO_BASE as *mut u32);
                #[cfg(not(feature = "early-printk"))]
                gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, 1); // patch us over to a different UART for debug (1=LOG 2=APP, 0=KERNEL(hw reset default))
                #[cfg(feature = "early-printk")]
                gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, 0); // on early printk, leave us on the kernel owning the UART
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
    let page0 = cfg.swap_hal.as_mut().unwrap().decrypt_src_page_at(0x0).unwrap();
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

pub fn read_detached_app_config(cfg: &mut BootConfig) {
    let app_args = KernelArguments::new(
        (bao1x_api::offsets::dabao::APP_RRAM_START + bao1x_api::signatures::SIGBLOCK_LEN) as *const usize,
    );
    for tag in app_args.iter() {
        if tag.name == u32::from_le_bytes(*b"IniF") {
            assert!(tag.size >= 4, "invalid Init size");
            cfg.init_process_count += 1;
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
#[cfg(all(not(feature = "simulation-only")))]
fn clear_ram(cfg: &mut BootConfig) {
    // clear RAM on a cold boot.
    // RAM is persistent and battery-backed. This means secret material could potentially
    // stay there forever, if not explicitly cleared. This clear adds a couple seconds
    // to a cold boot, but it's probably worth it. Note that it doesn't happen on a suspend/resume.
    let ram: *mut u32 = cfg.sram_start as *mut u32;
    #[cfg(feature = "swap")]
    let clear_limit = GUARD_MEMORY_BYTES;
    #[cfg(not(feature = "swap"))]
    let clear_limit = ((4096 + core::mem::size_of::<BootConfig>()) + 4095) & !4095;
    if VDBG {
        println!("Stack clearing limit: {:x}", clear_limit);
    }
    let clear_start = HEAP_OFFSET + HEAP_LEN;
    unsafe {
        for addr in clear_start..(cfg.sram_size - clear_limit) / 4 {
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
