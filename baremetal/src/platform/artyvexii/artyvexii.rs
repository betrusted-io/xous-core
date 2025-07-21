use utralib::*;

#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

// RAM size has 2 pages taken off the top to make space for exception handlers
pub const RAM_SIZE: usize = utralib::generated::HW_MAIN_RAM_MEM_LEN - 8192;
pub const RAM_BASE: usize = utralib::generated::HW_MAIN_RAM_MEM;

// scratch page for exceptions located at top of RAM
// NOTE: there is an additional page above this for exception stack
// pub const SCRATCH_PAGE: usize = RAM_BASE + RAM_SIZE;

// Arbitrarily placing heap at 16k into RAM, with a 32k length
// Total memory is 400k in size
// RAM starts at 256k, giving 144k RAM total
// Bottom 4k is for static data (.bss section)
// next 64k is heap; rest is stack.
pub const ROM_LEN: usize = 64 * 1024;
pub const BSS_LEN: usize = 4 * 1024;
pub const HEAP_START: usize = RAM_BASE + ROM_LEN + BSS_LEN;
pub const HEAP_LEN: usize = 1024 * 32;

pub const SYSTEM_CLOCK_FREQUENCY: u32 = 40_000_000;
pub const SYSTEM_TICK_INTERVAL_MS: u32 = 1;

pub const CACHE_LINE_STRIDE_BYTES: usize = 512 / 8;

pub fn early_init() {
    // crate::ramtests::ramtests();

    // setup interrupts & enable IRQ handler for characters
    crate::irq::irq_setup();
    crate::debug::Uart::enable_rx(true);
    crate::irq::enable_irq(utra::uart::UART_IRQ);

    // Initialize the timer, which is needed by the delay() function.
    let mut timer = CSR::new(utra::timer0::HW_TIMER0_BASE as *mut u32);
    // not using interrupts, this will be polled by delay()
    timer.wfo(utra::timer0::EV_ENABLE_ZERO, 0);
    timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);

    let ms = SYSTEM_TICK_INTERVAL_MS;
    timer.wfo(utra::timer0::EN_EN, 0b0); // disable the timer
    // load its values
    timer.wfo(utra::timer0::LOAD_LOAD, 0);
    timer.wfo(utra::timer0::RELOAD_RELOAD, (SYSTEM_CLOCK_FREQUENCY / 1_000) * ms);
    // enable the timer
    timer.wfo(utra::timer0::EN_EN, 0b1);

    setup_alloc();
}

pub fn setup_alloc() {
    // Initialize the allocator with heap memory range
    crate::println!("Setting up heap @ {:x}-{:x}", HEAP_START, HEAP_START + HEAP_LEN);
    crate::println!("Stack @ {:x}-{:x}", HEAP_START + HEAP_LEN, RAM_BASE + RAM_SIZE);
    unsafe {
        ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_LEN);
    }
}

// Install a panic handler when not running tests.
#[cfg(all(not(test)))]
mod panic_handler {
    use core::panic::PanicInfo;
    #[panic_handler]
    fn handle_panic(_arg: &PanicInfo) -> ! {
        crate::println!("{}", _arg);
        loop {}
    }
}

/// Delay function that delays a given number of milliseconds.
pub fn delay(ms: usize) {
    let mut timer = CSR::new(utra::timer0::HW_TIMER0_BASE as *mut u32);
    timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);
    for _ in 0..ms {
        while timer.rf(utra::timer0::EV_PENDING_ZERO) == 0 {}
        timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);
    }
}

pub const PAGE_SIZE: usize = 4096;
const WORD_SIZE: usize = core::mem::size_of::<u32>();

const FLG_VALID: usize = 0x1;
const FLG_X: usize = 0x8;
const FLG_W: usize = 0x4;
const FLG_R: usize = 0x2;
#[allow(dead_code)]
const FLG_U: usize = 0x10;
#[allow(dead_code)]
const FLG_G: usize = 0x20;
#[allow(dead_code)]
const FLG_A: usize = 0x40;
#[allow(dead_code)]
const FLG_D: usize = 0x80;

#[repr(C)]
pub struct PageTable {
    entries: [usize; PAGE_SIZE / WORD_SIZE],
}

// locate the page table entries
pub const ROOT_PT_PA: usize = 0x4000_0000; // 1st level at base of sram
// 2nd level PTs
const SRAM_PT_PA: usize = 0x4000_1000;
const CODE_PT_PA: usize = 0x4000_2000;
const CSR_PT_PA: usize = 0x4000_3000;
const LCD_PT_PA: usize = 0x4000_4000;
const FLASH_PT_PA: usize = 0x4000_5000;
const TEST_PT_PA: usize = 0x4000_6000;

pub const ROOT_S_PT_PA: usize = 0x4000_7000;
const SRAM_S_PT_PA: usize = 0x4000_8000;
const CODE_S_PT_PA: usize = 0x4000_9000;
const CSR_S_PT_PA: usize = 0x4000_A000;
const TEST_S_PT_PA: usize = 0x4000_B000;

// exception handler pages. Mapped 1:1 PA:VA, so no explicit remapping needed as RAM area is already mapped.
pub const SCRATCH_PAGE: usize = 0x4000_C000; // update this in irq.rs _start_trap() asm! as the scratch are
pub const _EXCEPTION_STACK_LIMIT: usize = 0x4000_D000; // update this in irq.rs as default stack pointer. The start of stack is this + 0x1000 & grows down
const BSS_PAGE: usize = 0x4000_E000; // this is manually read out of the link file. Equal to "base of RAM"
pub const PT_LIMIT: usize = 0x4000_F000; // this is carved out in link.x by setting RAM base at BSS_PAGE start

// VAs
const CODE_VA: usize = 0x8000_0000;
const CSR_VA: usize = 0xe000_0000;
const SRAM_VA: usize = 0x4000_0000;
const LCD_VA: usize = 0xB000_0000;
const FLASH_VA: usize = 0x2000_0000;
pub const TEST_VA: usize = 0xF000_0000;

// PAs (when different from VAs)
const CODE_PA: usize = 0x8000_0000;
const TEST_PA: usize = 0x5000_0000;

// ASID's for various 'processes'
pub const SUP_ASID: u32 = 1;
pub const USR_ASID: u32 = 2;

fn set_l1_pte(from_va: usize, to_pa: usize, root_pt: &mut PageTable) {
    let index = from_va >> 22;
    root_pt.entries[index] = ((to_pa & 0xFFFF_FC00) >> 2) // top 2 bits of PA are not used, we don't do 34-bit PA featured by Sv32
        | FLG_VALID;
}

fn set_l2_pte(from_va: usize, to_pa: usize, l2_pt: &mut PageTable, flags: usize) {
    let index = (from_va >> 12) & 0x3_FF;
    l2_pt.entries[index] = ((to_pa & 0xFFFF_FC00) >> 2) // top 2 bits of PA are not used, we don't do 34-bit PA featured by Sv32
        | flags
        | FLG_VALID;
}

#[inline(never)] // correct behavior depends on RA being set.
pub fn test_pivot() {
    // crate::println!("test_pivot");
    /*
    unsafe {
        let pt_clr = ROOT_PT_PA as *mut u32;
        for i in 0..(PT_LIMIT - ROOT_PT_PA) / core::mem::size_of::<u32>() {
            pt_clr.add(i).write_volatile(0x0000_0000);
        }
    }
    */
    // root page table is at p0x6100_0000 == v0x6100_0000
    let mut root_pt = unsafe { &mut *(ROOT_PT_PA as *mut PageTable) };
    let mut sram_pt = unsafe { &mut *(SRAM_PT_PA as *mut PageTable) };
    let mut code_pt = unsafe { &mut *(CODE_PT_PA as *mut PageTable) };
    let mut csr_pt = unsafe { &mut *(CSR_PT_PA as *mut PageTable) };
    let mut lcd_pt = unsafe { &mut *(LCD_PT_PA as *mut PageTable) };
    let mut flash_pt = unsafe { &mut *(FLASH_PT_PA as *mut PageTable) };
    let mut test_pt = unsafe { &mut *(TEST_PT_PA as *mut PageTable) };

    let mut root_s_pt = unsafe { &mut *(ROOT_S_PT_PA as *mut PageTable) };
    let mut sram_s_pt = unsafe { &mut *(SRAM_S_PT_PA as *mut PageTable) };
    let mut code_s_pt = unsafe { &mut *(CODE_S_PT_PA as *mut PageTable) };
    let mut csr_s_pt = unsafe { &mut *(CSR_S_PT_PA as *mut PageTable) };
    let mut test_s_pt = unsafe { &mut *(TEST_S_PT_PA as *mut PageTable) };

    set_l1_pte(CODE_VA, CODE_PT_PA, &mut root_pt);
    set_l1_pte(CSR_VA, CSR_PT_PA, &mut root_pt);
    set_l1_pte(SRAM_VA, SRAM_PT_PA, &mut root_pt); // L1 covers 16MiB, so SP_VA will cover all of SRAM
    set_l1_pte(LCD_VA, LCD_PT_PA, &mut root_pt);
    set_l1_pte(FLASH_VA, FLASH_PT_PA, &mut root_pt);
    set_l1_pte(TEST_VA, TEST_PT_PA, &mut root_pt);

    set_l1_pte(CODE_VA, CODE_S_PT_PA, &mut root_s_pt);
    set_l1_pte(CSR_VA, CSR_S_PT_PA, &mut root_s_pt);
    set_l1_pte(SRAM_VA, SRAM_S_PT_PA, &mut root_s_pt);
    set_l1_pte(TEST_VA, TEST_S_PT_PA, &mut root_s_pt);

    // map code space. This is the only one that has a difference on VA->PA
    const CODE_LEN: usize = 0x2_0000; // 128k
    for offset in (0..CODE_LEN).step_by(PAGE_SIZE) {
        set_l2_pte(CODE_VA + offset, CODE_PA + offset, &mut code_pt, FLG_X | FLG_R | FLG_A | FLG_D | FLG_U);
    }
    for offset in (0..CODE_LEN).step_by(PAGE_SIZE) {
        set_l2_pte(CODE_VA + offset, CODE_PA + offset, &mut code_s_pt, FLG_X | FLG_R | FLG_A | FLG_D);
    }

    // crate::println!("mem pte");
    // map sram. Mapping is 1:1, so we use _VA and _PA targets for both args
    const SRAM_LEN: usize = 0x2_0000; // 128k
    // make the page tables not writeable
    for offset in (0..SCRATCH_PAGE - RAM_BASE).step_by(PAGE_SIZE) {
        set_l2_pte(SRAM_VA + offset, SRAM_VA + offset, &mut sram_pt, FLG_R | FLG_U);
    }
    for offset in (0..SCRATCH_PAGE - RAM_BASE).step_by(PAGE_SIZE) {
        set_l2_pte(SRAM_VA + offset, SRAM_VA + offset, &mut sram_s_pt, FLG_R);
    }
    // rest of RAM is r/w/x
    for offset in ((SCRATCH_PAGE - RAM_BASE)..SRAM_LEN).step_by(PAGE_SIZE) {
        set_l2_pte(
            SRAM_VA + offset,
            SRAM_VA + offset,
            &mut sram_pt,
            FLG_W | FLG_R | FLG_X | FLG_A | FLG_D | FLG_U,
        );
    }
    for offset in ((SCRATCH_PAGE - RAM_BASE)..SRAM_LEN).step_by(PAGE_SIZE) {
        set_l2_pte(SRAM_VA + offset, SRAM_VA + offset, &mut sram_s_pt, FLG_W | FLG_R | FLG_X | FLG_A | FLG_D);
    }
    if true {
        // map SoC-local peripherals (ticktimer, etc.)
        const CSR_LEN: usize = 0x2_0000; // 128k
        for offset in (0..CSR_LEN).step_by(PAGE_SIZE) {
            set_l2_pte(CSR_VA + offset, CSR_VA + offset, &mut csr_pt, FLG_W | FLG_R | FLG_A | FLG_D | FLG_U);
        }
        for offset in (0..CSR_LEN).step_by(PAGE_SIZE) {
            set_l2_pte(CSR_VA + offset, CSR_VA + offset, &mut csr_s_pt, FLG_W | FLG_R | FLG_A | FLG_D);
        }
        const LCD_LEN: usize = 0x0_8000; // 32k
        for offset in (0..LCD_LEN).step_by(PAGE_SIZE) {
            set_l2_pte(LCD_VA + offset, LCD_VA + offset, &mut lcd_pt, FLG_W | FLG_R | FLG_U | FLG_A | FLG_U);
        }
        const FLASH_LEN: usize = 0x10_0000; // 1M
        for offset in (0..FLASH_LEN).step_by(PAGE_SIZE) {
            set_l2_pte(
                FLASH_VA + offset,
                FLASH_VA + offset,
                &mut flash_pt,
                FLG_R | FLG_U | FLG_X | FLG_A | FLG_U,
            );
        }
        const TEST_LEN: usize = 0x1_0000; // 64k
        for offset in (0..TEST_LEN).step_by(PAGE_SIZE) {
            set_l2_pte(
                TEST_VA + offset,
                TEST_PA + offset,
                &mut test_pt,
                FLG_R | FLG_W | FLG_X | FLG_A | FLG_D | FLG_U,
            );
        }
        for offset in (0..TEST_LEN).step_by(PAGE_SIZE) {
            set_l2_pte(
                TEST_VA + offset,
                TEST_PA + offset,
                &mut test_s_pt,
                FLG_R | FLG_W | FLG_X | FLG_A | FLG_D,
            );
        }
    }
    if false {
        // clear BSS
        unsafe {
            let bss_region = core::slice::from_raw_parts_mut(BSS_PAGE as *mut u32, PAGE_SIZE / WORD_SIZE);
            for d in bss_region.iter_mut() {
                *d = 0;
                core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
            }
        }
    }

    let asid: u32 = SUP_ASID;
    let satp: u32 = 0x8000_0000 | asid << 22 | (ROOT_S_PT_PA as u32 >> 12);

    /*
    // flush pts
    for i in (ROOT_PT_PA..SCRATCH_PAGE).step_by(512 / 8) {
        unsafe { flush_block(i) };
    }
    for i in (0x8000_0000..0x8000_2000).step_by(512 / 8) {
        unsafe { flush_block(i) };
    }
    */
    // flush stack
    for i in (0x4001_d000..0x4001_f000).step_by(512 / 8) {
        // unsafe { flush_block(i) };
        unsafe {
            core::arch::asm!(
                ".word 0x0025200f",
                in("a0") i,
                options(nostack),
            );
        }
    }

    // crate::println!("vmem pivot");
    unsafe {
        core::arch::asm!(
            "fence",
            "fence.i",

            // Install a machine mode trap handler
            "la          t0, abort_vii",
            "csrw        mtvec, t0",
            // "csrw        utvec, t0",
            "csrw        stvec, t0",

            // Delegate as much as we can supervisor mode
            "li          t0, 0xffffffff",
            "csrw        mideleg, t0",
            "csrw        medeleg, t0",

            // Return to Supervisor mode (1 << 11) when we call `reti`.
            // Disable interrupts (0 << 5), allow supervisor mode to run user mode code (1 << 18)
            "li		    t0, (0 << 5) | (1 << 11)", //  | (1 << 18)
            "csrw	    mstatus, t0",

            // Enable the MMU (once we issue `mret`) and flush the cache
            "csrw        satp, {satp_val}",
            "sfence.vma",
            /*
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
            "sfence.vma",
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
            */
            satp_val = in(reg) satp,
        );
        core::arch::asm!(
            // When loading with GDB we don't use a VM offset so GDB is less confused
            // "li          t0, 0x20000000",
            "li          t0, 0x00000000",
        );
        core::arch::asm!(
            "add         a4, ra, t0",
            "csrw        mepc, a4",
            // sp "shouldn't move" because the mapping will take RAM mapping as 1:1 for VA:PA

            // Issue the return, which will jump to $mepc in the specified mode in mstatus
            "mret",
        );
    }
}

#[inline(never)] // correct behavior depends on RA being set.
pub fn pivot_user() {
    unsafe {
        core::arch::asm!(
            // Disable interrupts (0 << 5)
            "li		    t0, (0 << 5)",
            "csrw	    sstatus, t0",
            // Enable the MMU (once we issue `mret`) and flush the cache
            "csrw        sepc, ra",
            // Issue the return, which will jump to $mepc in the specified mode in mstatus
            "sret",
        );
    }
}
/*
#[target_feature(enable = "zicbom")]
pub unsafe fn flush_block(addr: usize) {
    core::arch::asm!(
        "cbo.flush  0({addr})",
        addr = in(reg) addr,
    )
}
*/
pub unsafe fn flush_block(addr: usize) {
    core::arch::asm!(
        "mv          a0, {addr}",
        ".word 0x0025200f", /* cbo.flush 0(a0) */
        addr = in(reg) addr,
        out("a0") _, // clobber a0
    );
}

/*
#define XENVCFG_CBIE_OK 0x30
#define XENVCFG_CBCFE_OK 0x40
  li x1, XENVCFG_CBCFE_OK | XENVCFG_CBIE_OK
  csrw 0x30a, x1 // Allow supervisor
  csrw 0x10a, x1 // Allow user
*/

pub const XENVCFG_CBIE_OK: usize = 0x30;
pub const XENVCFG_CBCFE_OK: usize = 0x40;

pub unsafe fn config_flush() {
    core::arch::asm!(
        "li   t0, 0x30 | 0x40",
        "csrw 0x30a, t0", // allow supervisor
        "csrw 0x10a, t0", // allow user
        out("t0") _, // clobber t0
    )
}

#[link_section = ".text.init"]
#[export_name = "abort_vii"]
/// This is only used in debug mode
pub extern "C" fn abort_vii() -> ! {
    unsafe {
        #[rustfmt::skip]
        core::arch::asm!(
            "li t0, 0xe0000800",
            "li t1, 0x00f",
            "li t2, 0xf00",
        "300:", // abort
            "sw t1, 0(t0)",
            "sw t2, 0(t0)",
            "j 300b",
            options(noreturn)
        );
    }
}
