#![cfg_attr(not(test), no_main)]
#![cfg_attr(not(test), no_std)]

#[macro_use]
mod args;
use args::{KernelArgument, KernelArguments};

#[cfg(feature="resume")]
mod murmur3;
mod platform;
pub const BACKUP_ARGS_ADDR: usize = crate::platform::RAM_BASE + crate::platform::RAM_SIZE - 0x2000;

use core::num::NonZeroUsize;
use core::{mem, ptr, slice};
use core::arch::asm;

pub type XousPid = u8;
pub const PAGE_SIZE: usize = 4096;
const WORD_SIZE: usize = mem::size_of::<usize>();

const USER_STACK_TOP: usize = 0x8000_0000;
const PAGE_TABLE_OFFSET: usize = 0xff40_0000;
const PAGE_TABLE_ROOT_OFFSET: usize = 0xff80_0000;
const CONTEXT_OFFSET: usize = 0xff80_1000;
const USER_AREA_END: usize = 0xff00_0000;

// All of the kernel structures must live within Megapage 1023,
// and therefore are limited to 4 MB.
const EXCEPTION_STACK_TOP: usize = 0xffff_0000;
const KERNEL_LOAD_OFFSET: usize = 0xffd0_0000;
const KERNEL_STACK_PAGE_COUNT: usize = 1;
const KERNEL_ARGUMENT_OFFSET: usize = 0xffc0_0000;
const GUARD_MEMORY_BYTES: usize = 2 * PAGE_SIZE;

pub const SIGBLOCK_SIZE: usize = 0x1000;

const FLG_VALID: usize = 0x1;
const FLG_X: usize = 0x8;
const FLG_W: usize = 0x4;
const FLG_R: usize = 0x2;
const FLG_U: usize = 0x10;
const FLG_A: usize = 0x40;
const FLG_D: usize = 0x80;
const STACK_PAGE_COUNT: usize = 8;

const MINIELF_FLG_W: u8 = 1;
#[allow(dead_code)]
const MINIELF_FLG_NC: u8 = 2;
#[allow(dead_code)]
const MINIELF_FLG_X: u8 = 4;
#[allow(dead_code)]
const MINIELF_FLG_EHF: u8 = 8;
#[allow(dead_code)]
const MINIELF_FLG_EHH: u8 = 0x10;

const VDBG: bool = false; // verbose debug
const VVDBG: bool = false; // very verbose debug

mod debug;

mod fonts;
#[cfg(feature="secboot")]
mod secboot;

// Install a panic handler when not running tests.
#[cfg(not(test))]
mod panic_handler {
    use core::panic::PanicInfo;
    #[panic_handler]
    fn handle_panic(_arg: &PanicInfo) -> ! {
        crate::println!("{}", _arg);
        loop {}
    }
}

#[cfg(test)]
mod test;

#[repr(C)]
struct MemoryRegionExtra {
    start: u32,
    length: u32,
    name: u32,
    padding: u32,
}

/// In-memory copy of the configuration page,
/// used by the stage-1 bootloader.
pub struct BootConfig {
    /// `true` if the kernel and Init programs run XIP
    no_copy: bool,

    /// Base load address.  Defaults to the start of the args block
    base_addr: *const usize,

    /// `true` if we should enable the `SUM` bit, allowing the
    /// kernel to access user memory.
    debug: bool,

    /// Where the tagged args list starts in RAM.
    args: KernelArguments,

    /// Additional memory regions in this system
    regions: &'static [MemoryRegionExtra],

    /// The origin of usable memory.  This is where heap lives.
    sram_start: *mut usize,

    /// The size (in bytes) of the heap.
    sram_size: usize,

    /// A running total of the number of bytes consumed during
    /// initialization.  This value, divided by PAGE_SIZE,
    /// indicates the number of pages at the end of RAM that
    /// will need to be owned by the kernel.
    init_size: usize,

    /// Additional pages that are consumed during init.
    /// This includes pages that are allocated to other
    /// processes.
    extra_pages: usize,

    /// This structure keeps track of which pages are owned
    /// and which are free. A PID of `0` indicates it's free.
    runtime_page_tracker: &'static mut [XousPid],

    /// A list of processes that were set up.  The first element
    /// is the kernel, and any subsequent elements are init processes.
    processes: &'static mut [InitialProcess],

    /// The number of 'Init' tags discovered
    init_process_count: usize,
}

impl Default for BootConfig {
    fn default() -> BootConfig {
        BootConfig {
            no_copy: false,
            debug: false,
            base_addr: core::ptr::null::<usize>(),
            regions: Default::default(),
            sram_start: core::ptr::null_mut::<usize>(),
            sram_size: 0,
            args: KernelArguments::new(core::ptr::null::<usize>()),
            init_size: 0,
            extra_pages: 0,
            runtime_page_tracker: Default::default(),
            init_process_count: 0,
            processes: Default::default(),
        }
    }
}

/// A single RISC-V page table entry.  In order to resolve an address,
/// we need two entries: the top level, followed by the lower level.
#[repr(C)]
pub struct PageTable {
    entries: [usize; PAGE_SIZE / WORD_SIZE],
}

#[repr(C)]
pub struct InitialProcess {
    /// The RISC-V SATP value, which includes the offset of the root page
    /// table plus the process ID.
    satp: usize,

    /// Where execution begins
    entrypoint: usize,

    /// Address of the top of the stack
    sp: usize,
}

#[repr(C)]
pub struct MiniElfSection {
    // Virtual address of this section
    pub virt: u32,

    // A combination of the size and flags
    size_and_flags: u32,
}

impl MiniElfSection {
    pub fn len(&self) -> usize {
        // Strip off the top four bits, which contain the flags.
        let len = self.size_and_flags & !0xff00_0000;
        len as usize
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn flags(&self) -> usize {
        let le_bytes = self.size_and_flags >> 24;
        le_bytes as usize
    }

    pub fn no_copy(&self) -> bool {
        self.size_and_flags & (1 << 25) != 0
    }
}

/// Describes a Mini ELF file, suitable for loading into RAM
pub struct MiniElf {
    /// Offset of the source data relative to the start of the image file
    pub load_offset: u32,

    /// Virtual address of the entrypoint
    pub entry_point: u32,

    /// All of the sections inside this file
    pub sections: &'static [MiniElfSection],
}

impl MiniElf {
    pub fn new(tag: &KernelArgument) -> Self {
        let ptr = tag.data.as_ptr();
        unsafe {
            MiniElf {
                load_offset: ptr.add(0).read(),
                entry_point: ptr.add(1).read(),
                sections: slice::from_raw_parts(
                    ptr.add(2) as *mut MiniElfSection,
                    (tag.size as usize - 8) / mem::size_of::<MiniElfSection>(),
                ),
            }
        }
    }

    /// Load the process into its own memory space.
    /// The process will have been already loaded in stage 1.  This simply assigns
    /// memory maps as necessary.
    pub fn load(&self, allocator: &mut BootConfig, load_offset: usize, pid: XousPid, xip: bool) -> usize {
        println!("Mapping PID {} starting at offset {:08x}", pid, load_offset);
        let mut allocated_bytes = 0;

        let mut current_page_addr: usize = 0;
        let mut previous_addr: usize = 0;
        let mut last_mapped_xip = 0;
        let image_phys_base = allocator.base_addr as usize + self.load_offset as usize;
        // It is a requirement that the image generator lay out the artifacts on disk such that
        // the page offsets line up for XIP sections. This assert confirms this necessary pre-condition.
        if xip {
            assert!((image_phys_base & (PAGE_SIZE - 1)) == self.sections[0].virt as usize & (PAGE_SIZE - 1), "Image generator did not align load offsets to page offsets!");
        }
        println!("flash_map_offset: {:x} / base_addr {:x} load_offset {:x}", image_phys_base as usize, allocator.base_addr as usize, self.load_offset as usize);

        // The load offset is the end of this process.  Shift it down by one page
        // so we get the start of the first page.
        let mut top = load_offset - PAGE_SIZE;
        let stack_addr = USER_STACK_TOP - 16;

        // Allocate a page to handle the top-level memory translation
        let satp_address = allocator.alloc() as usize;
        allocator.change_owner(pid as XousPid, satp_address);

        // Turn the satp address into a pointer
        println!("    Pagetable @ {:08x}", satp_address);
        let satp = unsafe { &mut *(satp_address as *mut PageTable) };
        allocator.map_page(satp, satp_address, PAGE_TABLE_ROOT_OFFSET, FLG_R | FLG_W | FLG_VALID);

        // Allocate thread 1 for this process
        let thread_address = allocator.alloc() as usize;
        println!("    Thread 1 @ {:08x}", thread_address);
        allocator.map_page(satp, thread_address, CONTEXT_OFFSET, FLG_R | FLG_W | FLG_VALID);
        allocator.change_owner(pid as XousPid, thread_address as usize);

        // Allocate stack pages.
        println!("    Stack");
        for i in 0..STACK_PAGE_COUNT {
            if i == 0 {
                // For the initial stack frame, allocate a valid page
                let sp_page = allocator.alloc() as usize;
                allocator.map_page(
                    satp,
                    sp_page,
                    (stack_addr - PAGE_SIZE * i) & !(PAGE_SIZE - 1),
                    FLG_U | FLG_R | FLG_W | FLG_VALID,
                );
                allocator.change_owner(pid as XousPid, sp_page);
            } else {
                // Reserve every other stack page other than the 1st page.
                allocator.map_page(
                    satp,
                    0,
                    (stack_addr - PAGE_SIZE * i) & !(PAGE_SIZE - 1),
                    FLG_U | FLG_R | FLG_W,
                );
            }
        }

        // this works to set the initial offset, but from here we have to track it by
        // adding the length of each section as we see it
        let mut section_start_phys_offset = 0;
        // Example: Page starts at 0xf0c0 and is 8192 bytes long.
        // 1. Copy 3094 bytes to page 1
        // 2. Copy 4096 bytes to page 2
        // 3. Copy 192 bytes to page 3
        //
        // Example: Page starts at 0xf000 and is 4096 bytes long
        // 1. Copy 4096 bytes to page 1
        //
        // Example: Page starts at 0xf000 and is 128 bytes long
        // 1. Copy 128 bytes to page 1
        //
        // Example: Page starts at 0xf0c0 and is 128 bytes long
        // 1. Copy 128 bytes to page 1
        for section in self.sections {
            if VDBG {println!("    Section @ {:08x}", section.virt as usize);}
            let flag_defaults = FLG_U
            | FLG_R
            | FLG_X
            | FLG_VALID
            | if section.flags() & 1 == 1 { FLG_W } else { 0 }
            | if section.flags() & 4 == 4 { FLG_X } else { 0 };

            let copy_to_ram = ((section.flags() as u8) & MINIELF_FLG_W) != 0;
            if (section.virt as usize) < previous_addr {
                panic!("init section addresses are not strictly increasing");
            }

            if copy_to_ram || !xip {
                let mut this_page = section.virt as usize & !(PAGE_SIZE - 1);
                let mut bytes_to_copy = section.len();

                // If this is not a new page, ensure the uninitialized values from between
                // this section and the previous one are all zeroed out.
                if this_page != current_page_addr || previous_addr == current_page_addr {
                    if VDBG {println!("1       {:08x} -> {:08x}", top as usize, this_page);}
                    allocator.map_page(satp, top as usize, this_page, flag_defaults);
                    allocator.change_owner(pid as XousPid, top as usize);
                    allocated_bytes += PAGE_SIZE;
                    top -= PAGE_SIZE;
                    this_page += PAGE_SIZE;

                    // Part 1: Copy the first chunk over.
                    let mut first_chunk_size = PAGE_SIZE - (section.virt as usize & (PAGE_SIZE - 1));
                    if first_chunk_size > section.len() {
                        first_chunk_size = section.len();
                    }
                    bytes_to_copy -= first_chunk_size;
                } else {
                    if VDBG {println!(
                        "This page is {:08x}, and last page was {:08x}",
                        this_page, current_page_addr
                    );}
                    // This is a continuation of the previous section, and as a result
                    // the memory will have been copied already. Avoid copying this data
                    // to a new page.
                    let first_chunk_size = PAGE_SIZE - (section.virt as usize & (PAGE_SIZE - 1));
                    if VDBG {println!("First chunk size: {}", first_chunk_size);}
                    if bytes_to_copy < first_chunk_size {
                        bytes_to_copy = 0;
                        if VDBG {println!("Clamping to 0 bytes");}
                    } else {
                        bytes_to_copy -= first_chunk_size;
                        if VDBG {println!(
                            "Clamping to {} bytes by cutting off {} bytes",
                            bytes_to_copy, first_chunk_size
                        );}
                    }
                    this_page += PAGE_SIZE;
                }

                // Part 2: Copy any full pages.
                while bytes_to_copy >= PAGE_SIZE {
                    if VDBG {println!("2       {:08x} -> {:08x}", top as usize, this_page);}
                    allocator.map_page(satp, top as usize, this_page, flag_defaults);
                    allocator.change_owner(pid as XousPid, top as usize);
                    allocated_bytes += PAGE_SIZE;
                    top -= PAGE_SIZE;
                    this_page += PAGE_SIZE;
                    bytes_to_copy -= PAGE_SIZE;
                }

                // Part 3: Copy the final residual partial page
                if bytes_to_copy > 0 {
                    let this_page = (section.virt as usize + section.len()) & !(PAGE_SIZE - 1);
                    if VDBG {println!("3       {:08x} -> {:08x}", top as usize, this_page);}
                    allocator.map_page(satp, top as usize, this_page, flag_defaults);
                    allocator.change_owner(pid as XousPid, top as usize);
                    allocated_bytes += PAGE_SIZE;
                    top -= PAGE_SIZE;
                }
            } else {
                // --- calculate how many pages need mapping ---
                let mut bytes_to_map = section.len();
                assert!(bytes_to_map > 0, "no data to map");
                let mut pages_to_map = 1;
                let start_page = section.virt as usize & !(PAGE_SIZE - 1);

                let unaligned_start_len = (start_page + PAGE_SIZE) - section.virt as usize;
                if unaligned_start_len >= bytes_to_map {
                     // we're done: the page is already mapped and it holds all the data we intend to map
                } else {
                    // remaining data from the first aligned page to end of mapped region
                    bytes_to_map -= unaligned_start_len;
                    // convert this to pages_to_map
                    pages_to_map += bytes_to_map / PAGE_SIZE;
                    if (bytes_to_map % PAGE_SIZE) != 0 {
                        // unaligned end page adds one more mapped page
                        pages_to_map += 1;
                    }
                }

                // --- calculate starting offset of section from image base ---
                let mut section_map_phys_offset = section_start_phys_offset;

                // --- avoid double-mapping the previous section's end ---
                // check if last_mapped_xip has already been mapped so we don't double-map overlapping pages
                // assume: sections are always increasing in size
                let mut virt_page = start_page;
                if last_mapped_xip == start_page {
                    if VDBG {println!("Skipping a page to avoid double-mapping: pa {:x} -> va {:x}",
                        (image_phys_base + section_start_phys_offset) & !(PAGE_SIZE - 1), virt_page);}
                    virt_page += PAGE_SIZE;
                    section_map_phys_offset += PAGE_SIZE;
                    pages_to_map -= 1;
                }
                if VDBG {
                    println!("section is 0x{:x} bytes long; mapping {} pages", section.len(), pages_to_map);
                }

                // --- map FLASH pages to virtual memory ---
                while pages_to_map > 0 {
                    let map_phys_addr = (image_phys_base + section_map_phys_offset) & !(PAGE_SIZE - 1);
                    allocator.map_page(satp, map_phys_addr, virt_page, flag_defaults);
                    last_mapped_xip = virt_page;
                    allocator.change_owner(pid as XousPid, top as usize);

                    section_map_phys_offset += PAGE_SIZE;
                    virt_page += PAGE_SIZE;
                    pages_to_map -= 1;
                }
            }
            section_start_phys_offset += section.len(); // the length of the section on disk

            previous_addr = section.virt as usize + section.len();
            current_page_addr = previous_addr & !(PAGE_SIZE - 1);
        }

        let mut process = &mut allocator.processes[pid as usize - 1];
        process.entrypoint = self.entry_point as usize;
        process.sp = stack_addr;
        process.satp = 0x8000_0000 | ((pid as usize) << 22) | (satp_address >> 12);
        println!("load allocated 0x{:x} bytes", allocated_bytes);
        allocated_bytes
    }

    /// Page through a processes allocated pages and check against the file spec.
    #[cfg(feature="debug-print")]
    pub fn check(&self, allocator: &mut BootConfig, load_offset: usize, pid: XousPid, xip: bool) {
        println!("Checking {} PID {} starting at offset {:08x}", if xip {"xip"} else {"ram"}, pid, load_offset);
        let image_phys_base = allocator.base_addr as usize + self.load_offset as usize;
        // the process offset is always 1 less than the PID, because that's how we built the table.
        let satp = allocator.processes[pid as usize - 1].satp;

        let mut section_offset = 0;
        for (index, section) in self.sections.iter().enumerate() {
            if let Some(dest_offset) = pt_walk(satp, section.virt as usize) {
                println!("  Section {} start 0x{:x}(PA src), 0x{:x}(VA dst), 0x{:x}(PA dst) len {}/0x{:x}",
                    index, section_offset + image_phys_base, section.virt as usize + section_offset,
                    dest_offset, section.len(), section.len()
                );
                // dumping routines
                let dump_pa_src = section_offset + image_phys_base;
                let dump_pa_dst = dest_offset;
                dump_addr(dump_pa_src, "    Src [:20]  ");
                dump_addr(dump_pa_dst, "    Dst [:20]  ");
                dump_addr(dump_pa_src + section.len() - 20, "    Src [-20:] ");
                dump_addr(dump_pa_dst + section.len() - 20, "    Dst [-20:] ");
            } else {
                println!("  Section {} start 0x{:x}(PA src), 0x{:x}(VA dst), ERR UNMAPPED!!",
                    index, section_offset + image_phys_base, section.virt as usize + section_offset
                );
            }
            section_offset += section.len();
        }
    }
}
#[cfg(feature="debug-print")]
fn dump_addr(addr: usize, label: &str) {
    print!("{}", label);
    let slice = unsafe{core::slice::from_raw_parts(addr as *const u8, 20)};
    for &b in slice {
        print!( "{:02x}", b);
    }
    print!("\n");
}
#[cfg(feature="debug-print")]
pub fn pt_walk(root: usize, va: usize) -> Option<usize> {
    let l1_pt = unsafe { &mut (*((root << 12) as *mut PageTable)) };
    let l1_entry = l1_pt.entries[(va & 0xFFC0_0000) >> 22];
    if l1_entry != 0 {
        let l0_pt = unsafe { &mut (*(((l1_entry >> 10) << 12) as *mut PageTable)) };
        let l0_entry = l0_pt.entries[(va & 0x003F_F000) >> 12];
        if l0_entry & 1 != 0 { // bit 1 is the "valid" bit
            Some(((l0_entry >> 10) << 12) | va & 0xFFF)
        } else {
            None
        }
    } else {
        None
    }
}
/// This describes the kernel as well as initially-loaded processes
#[repr(C)]
pub struct ProgramDescription {
    /// Physical source address of this program in RAM (i.e. SPI flash).
    /// The image is assumed to contain a text section followed immediately
    /// by a data section.
    load_offset: u32,

    /// Start of the virtual address where the .text section will go.
    /// This section will be marked non-writable, executable.
    text_offset: u32,

    /// How many bytes of data to load from the source to the target
    text_size: u32,

    /// Start of the virtual address of .data and .bss section in RAM.
    /// This will simply allocate this memory and mark it "read-write"
    /// without actually copying any data.
    data_offset: u32,

    /// Size of the .data section, in bytes..  This many bytes will
    /// be allocated for the data section.
    data_size: u32,

    /// Size of the .bss section, in bytes.
    bss_size: u32,

    /// Virtual address entry point.
    entrypoint: u32,
}

// Note: inline constants are not yet stable in Rust: https://github.com/rust-lang/rust/pull/104087
#[link_section = ".text.init"]
#[export_name = "_start"]
pub extern "C" fn _start(_kernel_args: usize, loader_sig: usize) {
    #[cfg(feature="precursor")]
    let _kernel_args = _kernel_args;
    #[cfg(feature="cramium")]
    let _kernel_args = _start as *const usize as usize + platform::KERNEL_OFFSET;
    unsafe {
        asm! (
            "li          t0, 0xffffffff",
            "csrw        mideleg, t0",
            "csrw        medeleg, t0",

            // decorate our stack area with a canary pattern
            "li          t1, 0xACE0BACE",
            "mv          t0, {stack_limit}",
            "mv          t2, {ram_top}",
        "100:", // fillstack
            "sw          t1, 0(t0)",
            "addi        t0, t0, 4",
            "bltu        t0, t2, 100b",

            // Place the stack pointer at the end of RAM
            "mv          sp, {ram_top}",

            // Install a machine mode trap handler
            "la          t0, abort",
            "csrw        mtvec, t0",

            // this forces a0/a1 to be "used" and thus not allocated for other parameters passed in
            "mv          a0, {kernel_args}",
            "mv          a1, {loader_sig}",
            // Start Rust
            "j   rust_entry",

            kernel_args = in(reg) _kernel_args,
            loader_sig = in(reg) loader_sig,
            ram_top = in(reg) (platform::RAM_BASE + platform::RAM_SIZE),
            // On Precursor - 0x40FFE01C: currently allowed stack extent - 8k - (7 words). 7 words are for kernel backup args - see bootloader in betrusted-soc
            stack_limit = in(reg) (platform::RAM_BASE + platform::RAM_SIZE - 8192 + 7 * core::mem::size_of::<usize>()),
            options(noreturn)
        );
    }
}

#[link_section = ".text.init"]
#[export_name = "abort"]
/// This is only used in debug mode
pub extern "C" fn abort() {
    unsafe {
        asm! (
            "300:", // abort
                "j 300b",
            options(noreturn)
        );
    }
}

#[inline(never)]
#[export_name = "start_kernel"]
pub extern "C" fn start_kernel(
    args: usize,
    ss: usize,
    rpt: usize,
    satp: usize,
    entrypoint: usize,
    stack: usize,
    debug_: bool,
    resume_: bool,
) -> ! {
    let debug: usize = if debug_ { 1 } else { 0 };
    let resume: usize = if resume_ { 1 } else { 0 };
    unsafe {
        asm! (
            // these generate redundant mv's but it ensures that the arguments are marked as used
            "mv          a0, {args}",
            "mv          a1, {ss}",
            "mv          a2, {rpt}",
            "mv          a7, {resume}",
            // Delegate as much as we can supervisor mode
            "li          t0, 0xffffffff",
            "csrw        mideleg, t0",
            "csrw        medeleg, t0",

            // Return to Supervisor mode (1 << 11) when we call `reti`.
            // Disable interrupts (0 << 5)
            "li		     t0, (1 << 11) | (0 << 5)",
            // If arg6 is "true", also set mstatus.SUM to allow the kernel
            // to access userspace memory.
            "mv          a6, {debug}",
            "andi        a6, a6, 1",
            "slli        a6, a6, 18",
            "or          t0, t0, a6",
            "csrw	     mstatus, t0",

            // Enable the MMU (once we issue `mret`) and flush the cache
            "csrw        satp, {satp}",
            "sfence.vma",

            // Return to the address pointed to by $a4
            "csrw        mepc, {entrypoint}",

            // Reposition the stack at the offset passed by $a5
            "mv          sp, {stack}",

            // Issue the return, which will jump to $mepc in Supervisor mode
            "mret",
            args = in(reg) args,
            ss = in(reg) ss,
            rpt = in(reg) rpt,
            satp = in(reg) satp,
            entrypoint = in(reg) entrypoint,
            stack = in(reg) stack,
            debug = in(reg) debug,
            resume = in(reg) resume,
            options(noreturn)
        );
    }
}

impl ProgramDescription {
    /// Map this ProgramDescription into RAM.
    /// The program may already have been relocated, and so may be
    /// either on SPI flash or in RAM.  The `load_offset` argument
    /// that is passed in should be used instead of `self.load_offset`
    /// for this reason.
    pub fn load(&self, allocator: &mut BootConfig, load_offset: usize, pid: XousPid) {
        assert!(pid != 0);
        println!("Mapping PID {} into offset {:08x}", pid, load_offset);
        let pid_idx = (pid - 1) as usize;
        let is_kernel = pid == 1;
        let flag_defaults = FLG_R | FLG_W | FLG_VALID | if is_kernel { 0 } else { FLG_U };
        let stack_addr = USER_STACK_TOP - 16;
        if is_kernel {
            assert!(self.text_offset as usize == KERNEL_LOAD_OFFSET);
            assert!(((self.text_offset + self.text_size) as usize) < EXCEPTION_STACK_TOP);
            assert!(
                ((self.data_offset + self.data_size + self.bss_size) as usize)
                    < EXCEPTION_STACK_TOP - 16
            );
            assert!(self.data_offset as usize >= KERNEL_LOAD_OFFSET);
        } else {
            assert!(((self.text_offset + self.text_size) as usize) < USER_AREA_END);
            assert!(((self.data_offset + self.data_size) as usize) < USER_AREA_END);
        }

        // SATP must be nonzero
        if allocator.processes[pid_idx].satp != 0 {
            panic!("tried to re-use a process id");
        }

        // Allocate a page to handle the top-level memory translation
        let satp_address = allocator.alloc() as usize;
        allocator.change_owner(pid as XousPid, satp_address);

        // Turn the satp address into a pointer
        let satp = unsafe { &mut *(satp_address as *mut PageTable) };
        allocator.map_page(satp, satp_address, PAGE_TABLE_ROOT_OFFSET, FLG_R | FLG_W | FLG_VALID);
        allocator.change_owner(pid as XousPid, satp_address as usize);

        // Allocate context for this process
        let thread_address = allocator.alloc() as usize;
        allocator.map_page(satp, thread_address, CONTEXT_OFFSET, FLG_R | FLG_W | FLG_VALID);
        allocator.change_owner(pid as XousPid, thread_address as usize);

        // Allocate stack pages.
        for i in 0..if is_kernel {
            KERNEL_STACK_PAGE_COUNT
        } else {
            STACK_PAGE_COUNT
        } {
            if i == 0 {
                let sp_page = allocator.alloc() as usize;
                allocator.map_page(
                    satp,
                    sp_page,
                    (stack_addr - PAGE_SIZE * i) & !(PAGE_SIZE - 1),
                    flag_defaults,
                );
                allocator.change_owner(pid as XousPid, sp_page);
            } else {
                // Reserve every page other than the 1st stack page
                allocator.map_page(
                    satp,
                    0,
                    (stack_addr - PAGE_SIZE * i) & !(PAGE_SIZE - 1),
                    flag_defaults & !FLG_VALID,
                );
            }

            // If it's the kernel, also allocate an exception page
            if is_kernel {
                let sp_page = allocator.alloc() as usize;
                allocator.map_page(
                    satp,
                    sp_page,
                    (EXCEPTION_STACK_TOP - 16 - PAGE_SIZE * i) & !(PAGE_SIZE - 1),
                    flag_defaults,
                );
                allocator.change_owner(pid as XousPid, sp_page);
            }
        }

        assert!((self.text_offset as usize & (PAGE_SIZE - 1)) == 0);
        assert!((self.data_offset as usize & (PAGE_SIZE - 1)) == 0);
        if allocator.no_copy {
            assert!((self.load_offset as usize & (PAGE_SIZE - 1)) == 0);
        }

        // Map the process text section into RAM.
        // Either this is on SPI flash at an aligned address, or it
        // has been copied into RAM already.  This is why we ignore `self.load_offset`
        // and use the `load_offset` parameter instead.
        let rounded_data_bss =
            ((self.data_size + self.bss_size) as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

        // let load_size_rounded = (self.text_size as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        for offset in (0..self.text_size as usize).step_by(PAGE_SIZE) {
            if VDBG {println!(
                "   TEXT: Mapping {:08x} -> {:08x}",
                load_offset + offset + rounded_data_bss,
                self.text_offset as usize + offset
            );}
            allocator.map_page(
                satp,
                load_offset + offset + rounded_data_bss,
                self.text_offset as usize + offset,
                flag_defaults | FLG_X | FLG_VALID,
            );
            allocator.change_owner(pid as XousPid, load_offset + offset + rounded_data_bss);
        }

        // Map the process data section into RAM.
        for offset in (0..(self.data_size + self.bss_size) as usize).step_by(PAGE_SIZE as usize) {
            // let page_addr = allocator.alloc();
            if VDBG {println!(
                "   DATA: Mapping {:08x} -> {:08x}",
                load_offset + offset,
                self.data_offset as usize + offset
            );}
            allocator.map_page(
                satp,
                load_offset + offset,
                self.data_offset as usize + offset,
                flag_defaults,
            );
            allocator.change_owner(pid as XousPid, load_offset as usize + offset);
        }

        // Allocate pages for .bss, if necessary

        // Our "earlyprintk" equivalent
        if cfg!(feature = "earlyprintk") && is_kernel {
            allocator.map_page(satp, 0xF000_2000, 0xffcf_0000, FLG_R | FLG_W | FLG_VALID);
            allocator.change_owner(pid as XousPid, 0xF000_2000);
        }

        let mut process = &mut allocator.processes[pid_idx];
        process.entrypoint = self.entrypoint as usize;
        process.sp = stack_addr;
        process.satp = 0x8000_0000 | ((pid as usize) << 22) | (satp_address >> 12);
    }
}

unsafe fn bzero<T>(mut sbss: *mut T, ebss: *mut T)
where
    T: Copy,
{
    if VDBG {println!("ZERO: {:08x} - {:08x}", sbss as usize, ebss as usize);}
    while sbss < ebss {
        // NOTE(volatile) to prevent this from being transformed into `memclr`
        ptr::write_volatile(sbss, mem::zeroed());
        sbss = sbss.offset(1);
    }
}

unsafe fn memcpy<T>(dest: *mut T, src: *const T, count: usize)
where
    T: Copy,
{
    if VDBG {println!(
        "COPY (align {}): {:08x} - {:08x} {} {:08x} - {:08x}",
        mem::size_of::<T>(),
        src as usize,
        src as usize + count,
        count,
        dest as usize,
        dest as usize + count
    );}
    core::ptr::copy_nonoverlapping(src, dest, count / mem::size_of::<T>());
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
            assert!(
                tag.size as usize == mem::size_of::<ProgramDescription>(),
                "invalid XKrn size"
            );
            kernel_seen = true;
        } else if tag.name == u32::from_le_bytes(*b"IniE") || tag.name == u32::from_le_bytes(*b"IniF") {
            assert!(tag.size >= 4, "invalid Init size");
            init_seen = true;
            cfg.init_process_count += 1;
        }
    }

    assert!(kernel_seen, "no kernel definition");
    assert!(init_seen, "no initial programs found");
}

/// Copy program data from the SPI flash into newly-allocated RAM
/// located at the end of memory space.
fn copy_processes(cfg: &mut BootConfig) {
    let mut _pid = 1;
    for tag in cfg.args.iter() {
        if tag.name == u32::from_le_bytes(*b"IniE") {
            _pid += 1;
            let mut page_addr: usize = 0;
            let mut previous_addr: usize = 0;
            let mut top = core::ptr::null_mut::<u8>();

            let inie = MiniElf::new(&tag);
            let mut src_addr = unsafe {
                cfg.base_addr
                    .add(inie.load_offset as usize / mem::size_of::<usize>())
            } as *const u8;

            // Example: Page starts at 0xf0c0 and is 8192 bytes long.
            // 1. Copy 3094 bytes to page 1
            // 2. Copy 4096 bytes to page 2
            // 3. Copy 192 bytes to page 3
            //
            // Example: Page starts at 0xf000 and is 4096 bytes long
            // 1. Copy 4096 bytes to page 1
            //
            // Example: Page starts at 0xf000 and is 128 bytes long
            // 1. Copy 128 bytes to page 1
            //
            // Example: Page starts at oxf0c0 and is 128 bytes long
            // 1. Copy 128 bytes to page 1
            println!("\n\nIniE {} has {} sections", _pid, inie.sections.len());
            println!("Initial top: {:x}, extra_pages: {:x}, init_size: {:x}, base_addr: {:x}",
                cfg.get_top() as *mut u8 as u32, cfg.extra_pages, cfg.init_size, cfg.base_addr as u32);
            for section in inie.sections.iter() {
                if (section.virt as usize) < previous_addr {
                    panic!("init section addresses are not strictly increasing (new virt: {:08x}, last virt: {:08x})", section.virt, previous_addr);
                }

                let this_page = section.virt as usize & !(PAGE_SIZE - 1);
                let mut bytes_to_copy = section.len();

                if VDBG {println!(
                    "Section is {} bytes long, loaded to {:08x}",
                    bytes_to_copy, section.virt
                );}
                // If this is not a new page, ensure the uninitialized values from between
                // this section and the previous one are all zeroed out.
                if this_page != page_addr || previous_addr == page_addr {
                    if VDBG {println!("New page @ {:08x}", this_page);}
                    if previous_addr != 0 && previous_addr != page_addr {
                        if VDBG {println!(
                            "Zeroing-out remainder of previous page: {:08x} (mapped to physical address {:08x})",
                            previous_addr, top as usize,
                        );}
                        unsafe {
                            bzero(
                                top.add(previous_addr as usize & (PAGE_SIZE - 1)),
                                top.add(PAGE_SIZE as usize),
                            )
                        };
                    }

                    // Allocate a new page.
                    cfg.extra_pages += 1;
                    top = cfg.get_top() as *mut u8;

                    // Zero out the page, if necessary.
                    unsafe { bzero(top, top.add(section.virt as usize & (PAGE_SIZE - 1))) };
                } else {
                    if VDBG {println!("Reusing existing page @ {:08x}", this_page);}
                }

                // Part 1: Copy the first chunk over.
                let mut first_chunk_size = PAGE_SIZE - (section.virt as usize & (PAGE_SIZE - 1));
                if first_chunk_size > section.len() {
                    first_chunk_size = section.len();
                }
                let first_chunk_offset = section.virt as usize & (PAGE_SIZE - 1);
                if VDBG {println!(
                    "Section chunk is {} bytes, {} from {:08x}:{:08x} -> {:08x}:{:08x} (virt: {:08x})",
                    first_chunk_size,
                    if section.no_copy() { "zeroing" } else { "copying" },
                    src_addr as usize,
                    unsafe { src_addr.add(first_chunk_size) as usize },
                    unsafe { top.add(first_chunk_offset) as usize },
                    unsafe { top.add(first_chunk_size + first_chunk_offset)
                        as usize },
                    this_page + first_chunk_offset,
                );}
                // Perform the copy, if NOCOPY is not set
                if !section.no_copy() {
                    unsafe {
                        memcpy(top.add(first_chunk_offset), src_addr, first_chunk_size);
                        src_addr = src_addr.add(first_chunk_size);
                    }
                } else {
                    unsafe {
                        bzero(
                            top.add(first_chunk_offset),
                            top.add(first_chunk_offset + first_chunk_size),
                        );
                    }
                }
                bytes_to_copy -= first_chunk_size;

                // Part 2: Copy any full pages.
                while bytes_to_copy >= PAGE_SIZE {
                    cfg.extra_pages += 1;
                    top = cfg.get_top() as *mut u8;
                    // println!(
                    //     "Copying next page from {:08x} {:08x} ({} bytes to go...)",
                    //     src_addr as usize, top as usize, bytes_to_copy
                    // );
                    if !section.no_copy() {
                        unsafe {
                            memcpy(top, src_addr, PAGE_SIZE);
                            src_addr = src_addr.add(PAGE_SIZE);
                        }
                    } else {
                        unsafe { bzero(top, top.add(PAGE_SIZE)) };
                    }
                    bytes_to_copy -= PAGE_SIZE;
                }

                // Part 3: Copy the final residual partial page
                if bytes_to_copy > 0 {
                    if VDBG {println!(
                        "Copying final section -- {} bytes @ {:08x}",
                        bytes_to_copy,
                        section.virt + (section.len() as u32) - (bytes_to_copy as u32)
                    );}
                    cfg.extra_pages += 1;
                    top = cfg.get_top() as *mut u8;
                    if !section.no_copy() {
                        unsafe {
                            memcpy(top, src_addr, bytes_to_copy);
                            src_addr = src_addr.add(bytes_to_copy);
                        }
                    } else {
                        unsafe { bzero(top, top.add(bytes_to_copy)) };
                    }
                }

                previous_addr = section.virt as usize + section.len();
                page_addr = previous_addr & !(PAGE_SIZE - 1);
                if VDBG {
                    println!("Looping to the next section");
                    println!("top: {:x}, extra_pages: {:x}, init_size: {:x}, base_addr: {:x}", cfg.get_top() as *mut u8 as u32, cfg.extra_pages, cfg.init_size, cfg.base_addr as u32);
                    println!("previous_addr: {:x}, page_addr: {:x}", previous_addr, page_addr);
                }
            }

            println!("Done with sections");
            if previous_addr as usize & (PAGE_SIZE - 1) != 0 {
                println!("Zeroing out remaining data");
                // Zero-out the trailing bytes
                unsafe {
                    bzero(
                        top.add(previous_addr as usize & (PAGE_SIZE - 1)),
                        top.add(PAGE_SIZE as usize),
                    )
                };
            } else {
                println!("Skipping zero step -- we ended on a page boundary");
            }
        } else if tag.name == u32::from_le_bytes(*b"XKrn") {
            let prog = unsafe { &*(tag.data.as_ptr() as *const ProgramDescription) };

            // TEXT SECTION
            // Round it off to a page boundary
            let load_size_rounded = (prog.text_size as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
            cfg.extra_pages += load_size_rounded / PAGE_SIZE;
            let top = cfg.get_top();
            println!("\n\nKernel top: {:x}, extra_pages: {:x}", top as u32, cfg.extra_pages);
            unsafe {
                // Copy the program to the target address, rounding it off to the load size.
                let src_addr = cfg
                    .base_addr
                    .add(prog.load_offset as usize / mem::size_of::<usize>());
                println!(
                    "    Copying TEXT from {:08x}-{:08x} to {:08x}-{:08x} ({} bytes long)",
                    src_addr as usize,
                    src_addr as u32 + prog.text_size,
                    top as usize,
                    top as u32 + prog.text_size + 4,
                    prog.text_size + 4
                );
                println!(
                    "    Zeroing out TEXT from {:08x}-{:08x}",
                    top.add(prog.text_size as usize / mem::size_of::<usize>()) as usize,
                    top.add(load_size_rounded as usize / mem::size_of::<usize>()) as usize,
                );

                memcpy(top, src_addr, prog.text_size as usize + 1);

                // Zero out the remaining data.
                bzero(
                    top.add(prog.text_size as usize / mem::size_of::<usize>()),
                    top.add(load_size_rounded as usize / mem::size_of::<usize>()),
                )
            };

            // DATA SECTION
            // Round it off to a page boundary
            let load_size_rounded =
                ((prog.data_size + prog.bss_size) as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
            cfg.extra_pages += load_size_rounded / PAGE_SIZE;
            let top = cfg.get_top();
            unsafe {
                // Copy the program to the target address, rounding it off to the load size.
                let src_addr = cfg.base_addr.add(
                    (prog.load_offset + prog.text_size + 4) as usize / mem::size_of::<usize>() - 1,
                );
                println!(
                    "    Copying DATA from {:08x}-{:08x} to {:08x}-{:08x} ({} bytes long)",
                    src_addr as usize,
                    src_addr as u32 + prog.data_size,
                    top as usize,
                    top as u32 + prog.data_size,
                    prog.data_size
                );
                memcpy(top, src_addr, prog.data_size as usize + 1);

                // Zero out the remaining data.
                println!(
                    "    Zeroing out DATA from {:08x} - {:08x}",
                    top.add(prog.data_size as usize / mem::size_of::<usize>()) as usize,
                    top.add(load_size_rounded as usize / mem::size_of::<usize>()) as usize
                );
                bzero(
                    top.add(prog.data_size as usize / mem::size_of::<usize>()),
                    top.add(load_size_rounded as usize / mem::size_of::<usize>()),
                )
            };
        } else if tag.name == u32::from_le_bytes(*b"IniF") {
            _pid += 1;
            let mut page_addr: usize = 0;
            let mut previous_addr: usize = 0;
            let mut previous_ram_zero_addr: usize = 0;
            let mut top = core::ptr::null_mut::<u8>();

            let inif = MiniElf::new(&tag);
            let mut src_addr = unsafe {
                cfg.base_addr
                    .add(inif.load_offset as usize / mem::size_of::<usize>())
            } as *const u8;
            // let mut accumulated_len = 0;

            println!("\n\nIniF {} has {} sections", _pid, inif.sections.len());
            println!("Initial top: {:x}, extra_pages: {:x}, init_size: {:x}, base_addr: {:x}", cfg.get_top() as *mut u8 as u32, cfg.extra_pages, cfg.init_size, cfg.base_addr as u32);
            for section in inif.sections.iter() {
                let flags = section.flags() as u8;
                // any section that requires "write" must be copied to RAM
                // note that ELF helpfully adds a 4096-byte gap between non-write pages and write-pages
                // allowing us to just trundle through the pages and not have to deal with partially
                // writeable pages.
                let copy_to_ram = flags & MINIELF_FLG_W != 0;
                if (section.virt as usize) < previous_addr {
                    panic!("init section addresses are not strictly increasing (new virt: {:08x}, last virt: {:08x})", section.virt, previous_addr);
                }
                if copy_to_ram {
                    let this_page = section.virt as usize & !(PAGE_SIZE - 1);
                    let mut bytes_to_copy = section.len();

                    if VDBG {println!(
                        "Section is {} bytes long, loaded to {:08x}",
                        bytes_to_copy, section.virt
                    );}
                    // If this is not a new page, ensure the uninitialized values from between
                    // this section and the previous one are all zeroed out.
                    if this_page != page_addr || previous_addr == page_addr {
                        if VDBG {println!("New page @ {:08x}", this_page);}
                        if previous_ram_zero_addr != 0 && previous_ram_zero_addr != page_addr && cfg.extra_pages > 0 {
                            if VDBG {println!(
                                "Zeroing-out remainder of previous page: {:08x} (mapped to physical address {:08x})",
                                previous_ram_zero_addr, top as usize,
                            );}
                            unsafe {
                                bzero(
                                    top.add(previous_ram_zero_addr as usize & (PAGE_SIZE - 1)),
                                    top.add(PAGE_SIZE as usize),
                                )
                            };
                        }

                        // Allocate a new page.
                        cfg.extra_pages += 1;
                        top = cfg.get_top() as *mut u8;

                        // Zero out the page, if necessary.
                        unsafe { bzero(top, top.add(section.virt as usize & (PAGE_SIZE - 1))) };
                    } else {
                        if VDBG {println!("Reusing existing page @ {:08x}", this_page);}
                    }

                    // Part 1: Copy the first chunk over.
                    let mut first_chunk_size = PAGE_SIZE - (section.virt as usize & (PAGE_SIZE - 1));
                    if first_chunk_size > section.len() {
                        first_chunk_size = section.len();
                    }
                    let first_chunk_offset = section.virt as usize & (PAGE_SIZE - 1);
                    if VDBG {println!(
                        "Section chunk is {} bytes, {} from {:08x}:{:08x} -> {:08x}:{:08x} (virt: {:08x})",
                        first_chunk_size,
                        if section.no_copy() { "zeroing" } else { "copying" },
                        src_addr as usize,
                        unsafe { src_addr.add(first_chunk_size) as usize },
                        unsafe { top.add(first_chunk_offset) as usize },
                        unsafe { top.add(first_chunk_size + first_chunk_offset)
                            as usize },
                        this_page + first_chunk_offset,
                    );}
                    // Perform the copy, if NOCOPY is not set
                    if !section.no_copy() {
                        unsafe {
                            memcpy(top.add(first_chunk_offset), src_addr, first_chunk_size);
                            src_addr = src_addr.add(first_chunk_size);
                        }
                    } else {
                        unsafe {
                            bzero(
                                top.add(first_chunk_offset),
                                top.add(first_chunk_offset + first_chunk_size),
                            );
                        }
                    }
                    bytes_to_copy -= first_chunk_size;

                    // Part 2: Copy any full pages.
                    while bytes_to_copy >= PAGE_SIZE {
                        cfg.extra_pages += 1;
                        top = cfg.get_top() as *mut u8;
                        // println!(
                        //     "Copying next page from {:08x} {:08x} ({} bytes to go...)",
                        //     src_addr as usize, top as usize, bytes_to_copy
                        // );
                        if !section.no_copy() {
                            unsafe {
                                memcpy(top, src_addr, PAGE_SIZE);
                                src_addr = src_addr.add(PAGE_SIZE);
                            }
                        } else {
                            unsafe { bzero(top, top.add(PAGE_SIZE)) };
                        }
                        bytes_to_copy -= PAGE_SIZE;
                    }

                    // Part 3: Copy the final residual partial page
                    if bytes_to_copy > 0 {
                        if VDBG {println!(
                            "Copying final section -- {} bytes @ {:08x}",
                            bytes_to_copy,
                            section.virt + (section.len() as u32) - (bytes_to_copy as u32)
                        );}
                        cfg.extra_pages += 1;
                        top = cfg.get_top() as *mut u8;
                        if !section.no_copy() {
                            unsafe {
                                memcpy(top, src_addr, bytes_to_copy);
                                src_addr = src_addr.add(bytes_to_copy);
                            }
                        } else {
                            unsafe { bzero(top, top.add(bytes_to_copy)) };
                        }
                    }
                    previous_ram_zero_addr = section.virt as usize + section.len();
                } else {
                    top = cfg.get_top() as *mut u8;
                    src_addr = unsafe {src_addr.add(section.len())};
                }

                previous_addr = section.virt as usize + section.len();
                page_addr = previous_addr & !(PAGE_SIZE - 1);
                if VDBG {
                    println!("Looping to the next section");
                    println!("top: {:x}, extra_pages: {:x}, init_size: {:x}, base_addr: {:x}", cfg.get_top() as *mut u8 as u32, cfg.extra_pages, cfg.init_size, cfg.base_addr as u32);
                    println!("previous_addr: {:x}, page_addr: {:x}", previous_addr, page_addr);
                }
            }

            println!("Done with sections");
            if previous_addr as usize & (PAGE_SIZE - 1) != 0 {
                println!("Zeroing out remaining data");
                // Zero-out the trailing bytes
                unsafe {
                    bzero(
                        top.add(previous_addr as usize & (PAGE_SIZE - 1)),
                        top.add(PAGE_SIZE as usize),
                    )
                };
            } else {
                println!("Skipping zero step -- we ended on a page boundary");
            }
        }
    }
}

impl BootConfig {
    fn get_top(&self) -> *mut usize {
        let val = unsafe {
            self.sram_start.add(
                (self.sram_size - self.init_size - self.extra_pages * PAGE_SIZE)
                    / mem::size_of::<usize>(),
            )
        };
        assert!((val as usize) >= (self.sram_start as usize));
        assert!(
            (val as usize) < (self.sram_start as usize) + self.sram_size,
            "top address {:08x} > (start + size) {:08x} + {} = {:08x}",
            val as usize,
            self.sram_start as usize,
            self.sram_size,
            self.sram_start as usize + self.sram_size
        );
        val
    }

    /// Zero-alloc a new page, mark it as owned by PID1, and return it.
    /// Decrement the `next_page_offset` (npo) variable by one page.
    pub fn alloc(&mut self) -> *mut usize {
        self.extra_pages += 1;
        let pg = self.get_top();
        unsafe {
            // Grab the page address and zero it out
            bzero(
                pg as *mut usize,
                pg.add(PAGE_SIZE / mem::size_of::<usize>()) as *mut usize,
            );
        }
        // Mark this page as in-use by the kernel
        let extra_bytes = self.extra_pages * PAGE_SIZE;
        self.runtime_page_tracker[(self.sram_size - (extra_bytes + self.init_size)) / PAGE_SIZE] =
            1;

        // Return the address
        pg as *mut usize
    }

    pub fn change_owner(&mut self, pid: XousPid, addr: usize) {
        // First, check to see if the region is in RAM,
        if addr >= self.sram_start as usize && addr < self.sram_start as usize + self.sram_size {
            // Mark this page as in-use by the kernel
            self.runtime_page_tracker[(addr - self.sram_start as usize) / PAGE_SIZE] = pid;
            return;
        }
        // The region isn't in RAM, so check the other memory regions.
        let mut rpt_offset = self.sram_size / PAGE_SIZE;

        for region in self.regions.iter() {
            let rstart = region.start as usize;
            let rlen = region.length as usize;
            if addr >= rstart && addr < rstart + rlen {
                self.runtime_page_tracker[rpt_offset + (addr - rstart) / PAGE_SIZE] = pid;
                return;
            }
            rpt_offset += rlen / PAGE_SIZE;
        }
        panic!(
            "Tried to change region {:08x} that isn't in defined memory!",
            addr
        );
    }

    /// Map the given page to the specified process table.  If necessary,
    /// allocate a new page.
    ///
    /// # Panics
    ///
    /// * If you try to map a page twice
    pub fn map_page(&mut self, root: &mut PageTable, phys: usize, virt: usize, flags: usize) {
        if VDBG {println!("    map pa {:x} -> va {:x} (satp {:x})", phys, virt, root as *mut PageTable as u32);}
        match WORD_SIZE {
            4 => self.map_page_32(root, phys, virt, flags),
            8 => panic!("map_page doesn't work on 64-bit devices"),
            _ => panic!("unrecognized word size: {}", WORD_SIZE),
        }
    }

    pub fn map_page_32(&mut self, root: &mut PageTable, phys: usize, virt: usize, flags: usize) {
        let ppn1 = (phys >> 22) & ((1 << 12) - 1);
        let ppn0 = (phys >> 12) & ((1 << 10) - 1);
        let ppo = (phys) & ((1 << 12) - 1);

        let vpn1 = (virt >> 22) & ((1 << 10) - 1);
        let vpn0 = (virt >> 12) & ((1 << 10) - 1);
        let vpo = (virt) & ((1 << 12) - 1);

        assert!(ppn1 < 4096);
        assert!(ppn0 < 1024);
        assert!(ppo < 4096);
        assert!(vpn1 < 1024);
        assert!(vpn0 < 1024);
        assert!(vpo < 4096);

        let l1_pt = &mut root.entries;
        let mut new_addr = None;

        // Allocate a new level 1 pagetable entry if one doesn't exist.
        if l1_pt[vpn1] & FLG_VALID == 0 {
            let na = self.alloc() as usize;
            if VDBG {println!("The Level 1 page table is invalid ({:08x}) @ {:08x} -- allocating a new one @ {:08x}",
                unsafe { l1_pt.as_ptr().add(vpn1) } as usize, l1_pt[vpn1], na);}
            // Mark this entry as a leaf node (WRX as 0), and indicate
            // it is a valid page by setting "V".
            l1_pt[vpn1] = ((na >> 12) << 10) | FLG_VALID;
            new_addr = Some(NonZeroUsize::new(na).unwrap());
        }

        let l0_pt_idx =
            unsafe { &mut (*(((l1_pt[vpn1] << 2) & !((1 << 12) - 1)) as *mut PageTable)) };
        let l0_pt = &mut l0_pt_idx.entries;

        // Ensure the entry hasn't already been mapped to a different address.
        if l0_pt[vpn0] & 1 != 0 && (l0_pt[vpn0] & 0xffff_fc00) != ((ppn1 << 20) | (ppn0 << 10)) {
            panic!(
                "Page {:08x} was already allocated to {:08x}, so cannot map to {:08x}!",
                phys,
                (l0_pt[vpn0] >> 10) << 12,
                virt
            );
        }
        let previous_flags = l0_pt[vpn0] & 0xf;
        l0_pt[vpn0] =
            (ppn1 << 20) | (ppn0 << 10) | flags | previous_flags | FLG_D | FLG_A;

        // If we had to allocate a level 1 pagetable entry, ensure that it's
        // mapped into our address space, owned by PID 1.
        if let Some(addr) = new_addr {
            if VDBG {println!(
                ">>> Mapping new address {:08x} -> {:08x}",
                addr.get(),
                PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE
            );}
            self.map_page(
                root,
                addr.get(),
                PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE,
                FLG_R | FLG_W | FLG_VALID,
            );
            self.change_owner(1 as XousPid, addr.get());
            if VDBG {println!("<<< Done mapping new address");}
        }
    }
}

/// Allocate and initialize memory regions.
/// Returns a pointer to the start of the memory region.
pub fn allocate_regions(cfg: &mut BootConfig) {
    // Number of individual pages in the system
    let mut rpt_pages = cfg.sram_size / PAGE_SIZE;

    for region in cfg.regions.iter() {
        println!(
            "Discovered memory region {:08x} ({:08x} - {:08x}) -- {} bytes",
            region.name,
            region.start,
            region.start + region.length,
            region.length
        );
        let region_length_rounded = (region.length as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        rpt_pages += region_length_rounded / PAGE_SIZE;
    }

    // Round the tracker to a multiple of the page size, so as to keep memory
    // operations fast.
    rpt_pages = (rpt_pages + mem::size_of::<usize>() - 1) & !(mem::size_of::<usize>() - 1);

    cfg.init_size += rpt_pages * mem::size_of::<XousPid>();

    // Clear all memory pages such that they're not owned by anyone
    let runtime_page_tracker = cfg.get_top();
    assert!((runtime_page_tracker as usize) < (cfg.sram_start as usize) + cfg.sram_size);
    unsafe {
        bzero(
            runtime_page_tracker,
            runtime_page_tracker.add(rpt_pages / mem::size_of::<usize>()),
        );
    }

    cfg.runtime_page_tracker =
        unsafe { slice::from_raw_parts_mut(runtime_page_tracker as *mut XousPid, rpt_pages) };
}

pub fn allocate_processes(cfg: &mut BootConfig) {
    let process_count = cfg.init_process_count + 1;
    let table_size = process_count * mem::size_of::<InitialProcess>();
    // Allocate the process table
    cfg.init_size += table_size;
    let processes = cfg.get_top();
    unsafe {
        bzero(
            processes,
            processes.add((table_size / mem::size_of::<usize>()) as usize),
        );
    }
    cfg.processes = unsafe {
        slice::from_raw_parts_mut(processes as *mut InitialProcess, process_count as usize)
    };
}

pub fn copy_args(cfg: &mut BootConfig) {
    // Copy the args list to target RAM
    cfg.init_size += cfg.args.size();
    let runtime_arg_buffer = cfg.get_top();
    unsafe {
        #[allow(clippy::cast_ptr_alignment)]
        memcpy(
            runtime_arg_buffer,
            cfg.args.base as *const usize,
            cfg.args.size() as usize,
        )
    };
    cfg.args = KernelArguments::new(runtime_arg_buffer);
}

/// Stage 1 Bootloader
/// This makes the program self-sufficient by setting up memory page assignment
/// and copying the arguments to RAM.
/// Assume the bootloader has already set up the stack to point to the end of RAM.
///
/// # Safety
///
/// This function is safe to call exactly once.
#[export_name = "rust_entry"]
pub unsafe extern "C" fn rust_entry(signed_buffer: *const usize, signature: u32) -> ! {
    // initially validate the whole image on disk (including kernel args)
    // kernel args must be validated because tampering with them can change critical assumptions about
    // how data is loaded into memory
    #[cfg(feature="secboot")]
    if !secboot::validate_xous_img(signed_buffer as *const u32) {
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
    boot_sequence(kab, signature);
}


fn boot_sequence(args: KernelArguments, _signature: u32) -> ! {
    // Store the initial boot config on the stack.  We don't know
    // where in heap this memory will go.
    #[allow(clippy::cast_ptr_alignment)] // This test only works on 32-bit systems
    #[cfg(feature="platform-tests")]
    platform_tests();

    let mut cfg = BootConfig {
        base_addr: args.base as *const usize,
        args,
        ..Default::default()
    };
    read_initial_config(&mut cfg);

    // check to see if we are recovering from a clean suspend or not
    #[cfg(feature="resume")]
    let (clean, was_forced_suspend, susres_pid) = check_resume(&mut cfg);
    #[cfg(not(feature="resume"))]
    let clean = {
        // cold boot path
        println!("No suspend marker found, doing a cold boot!");
        #[cfg(feature="simulation-only")]
        println!("Configured for simulation. Skipping RAM clear!");
        #[cfg(not(feature="simulation-only"))]
        clear_ram(&mut cfg);
        phase_1(&mut cfg);
        phase_2(&mut cfg);
        #[cfg(feature="debug-print")]
        if VDBG { check_load(&mut cfg); }
        println!("done initializing for cold boot.");
        false
    };
    #[cfg(feature="resume")]
    if !clean {
        // cold boot path
        println!("No suspend marker found, doing a cold boot!");
        clear_ram(&mut cfg);
        phase_1(&mut cfg);
        phase_2(&mut cfg);
        #[cfg(feature="debug-print")]
        if VDBG { check_load(&mut cfg); }
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
            resume_csr.wo(utra::susres::STATE,
                resume_csr.ms(utra::susres::STATE_RESUME, 1) |
                resume_csr.ms(utra::susres::STATE_WAS_FORCED, 1)
            );
        } else {
            resume_csr.wfo(utra::susres::STATE_RESUME, 1);
        }
        resume_csr.wfo(utra::susres::CONTROL_PAUSE, 1); // ensure that the ticktimer is paused before resuming
        resume_csr.wfo(utra::susres::EV_ENABLE_SOFT_INT, 1); // ensure that the soft interrupt is enabled for the kernel to kick
        println!("clean suspend marker found, doing a resume!");

        // trigger the interrupt; it's not immediately handled, but rather checked later on by the kernel on clean resume
        resume_csr.wfo(utra::susres::INTERRUPT_INTERRUPT, 1);
    }

    if !clean {
        // The MMU should be set up now, and memory pages assigned to their
        // respective processes.
        let krn_struct_start = cfg.sram_start as usize + cfg.sram_size - cfg.init_size;
        let arg_offset = cfg.args.base as usize - krn_struct_start + KERNEL_ARGUMENT_OFFSET;
        let ip_offset = cfg.processes.as_ptr() as usize - krn_struct_start + KERNEL_ARGUMENT_OFFSET;
        let rpt_offset =
            cfg.runtime_page_tracker.as_ptr() as usize - krn_struct_start + KERNEL_ARGUMENT_OFFSET;
        println!(
            "Jumping to kernel @ {:08x} with map @ {:08x} and stack @ {:08x} (kargs: {:08x}, ip: {:08x}, rpt: {:08x})",
            cfg.processes[0].entrypoint, cfg.processes[0].satp, cfg.processes[0].sp,
            arg_offset, ip_offset, rpt_offset,
        );

        // save a copy of the computed kernel registers at the bottom of the page reserved
        // for the bootloader stack. Note there is no stack-smash protection for these arguments,
        // so we're definitely vulnerable to e.g. buffer overrun attacks in the early bootloader.
        //
        // Probably the right long-term solution to this is to turn all the bootloader "loading"
        // actions into "verify" actions during a clean resume (right now we just don't run them),
        // so that attempts to mess with the args during a resume can't lead to overwriting
        // critical parameters like these kernel arguments.
        unsafe {
            let backup_args: *mut [u32; 7] = BACKUP_ARGS_ADDR as *mut[u32; 7];
            (*backup_args)[0] = arg_offset as u32;
            (*backup_args)[1] = ip_offset as u32;
            (*backup_args)[2] = rpt_offset as u32;
            (*backup_args)[3] = cfg.processes[0].satp as u32;
            (*backup_args)[4] = cfg.processes[0].entrypoint as u32;
            (*backup_args)[5] = cfg.processes[0].sp as u32;
            (*backup_args)[6] = if cfg.debug {1} else {0};
            #[cfg(feature="debug-print")]
            {
                if VDBG {
                    println!("Backup kernel args:");
                    for i in 0..7 {
                        println!("0x{:08x}", (*backup_args)[i]);
                    }
                }
            }
        }
        use utralib::generated::*;
        let mut gpio_csr = CSR::new(utra::gpio::HW_GPIO_BASE as *mut u32);
        gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, 1); // patch us over to a different UART for debug (1=LOG 2=APP, 0=KERNEL(hw reset default))

        start_kernel(
            arg_offset,
            ip_offset,
            rpt_offset,
            cfg.processes[0].satp,
            cfg.processes[0].entrypoint,
            cfg.processes[0].sp,
            cfg.debug,
            clean,
        );
    } else {
        #[cfg(feature="resume")]
        unsafe {
            let backup_args: *mut [u32; 7] = BACKUP_ARGS_ADDR as *mut[u32; 7];
            #[cfg(feature="debug-print")]
            {
                println!("Using backed up kernel args:");
                for i in 0..7 {
                    println!("0x{:08x}", (*backup_args)[i]);
                }
            }
            let satp = ((*backup_args)[3] as usize) & 0x803F_FFFF | (((susres_pid as usize) & 0x1FF) << 22);
            //let satp = (*backup_args)[3];
            println!("Adjusting SATP to the sures process. Was: 0x{:08x} now: 0x{:08x}", (*backup_args)[3], satp);

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
                satp as usize,
                (*backup_args)[4] as usize,
                (*backup_args)[5] as usize,
                if (*backup_args)[6] == 0 {false} else {true},
                clean,
            );
        }
        #[cfg(not(feature="resume"))]
        panic!("Unreachable code executed");
    }
}
#[cfg(feature="resume")]
fn check_resume(cfg: &mut BootConfig) -> (bool, bool, u32) {
    use utralib::generated::*;
    const WORDS_PER_SECTOR: usize = 128;
    const NUM_SECTORS: usize = 8;
    const WORDS_PER_PAGE: usize = PAGE_SIZE / 4;

    let suspend_marker = cfg.sram_start as usize + cfg.sram_size - PAGE_SIZE * 3;
    let marker: *mut[u32; WORDS_PER_PAGE] = suspend_marker as *mut[u32; WORDS_PER_PAGE];

    let boot_seed = CSR::new(utra::seed::HW_SEED_BASE as *mut u32);
    let seed0 = boot_seed.r(utra::seed::SEED0);
    let seed1 = boot_seed.r(utra::seed::SEED1);
    let was_forced_suspend: bool = if unsafe{(*marker)[0]} != 0 { true } else { false };

    let mut clean = true;
    let mut hashbuf: [u32; WORDS_PER_SECTOR - 1] = [0; WORDS_PER_SECTOR - 1];
    let mut index: usize = 0;
    let mut pid: u32 = 0;
    for sector in 0..NUM_SECTORS {
        for i in 0..hashbuf.len() {
            hashbuf[i] = unsafe{(*marker)[index * WORDS_PER_SECTOR + i]};
        }
        // sector 0 contains the boot seeds, which we replace with our own as read out from our FPGA before computing the hash
        // it also contains the PID of the suspend/resume process manager, which we need to inject into the SATP
        if sector == 0 {
            hashbuf[1] = seed0;
            hashbuf[2] = seed1;
            pid = hashbuf[3];
        }
        let hash = crate::murmur3::murmur3_32(&hashbuf, 0);
        if hash != unsafe{(*marker)[(index+1) * WORDS_PER_SECTOR - 1]} {
            println!("* computed 0x{:08x} - stored 0x{:08x}", hash, unsafe{(*marker)[(index+1) * (WORDS_PER_SECTOR) - 1]});
            clean = false;
        } else {
            println!("  computed 0x{:08x} - match", hash);
        }
        index += 1;
    }
    // zero out the clean suspend marker, so if something goes wrong during resume we don't try to resume again
    for i in 0..WORDS_PER_PAGE {
        unsafe{(*marker)[i] = 0;}
    }

    (clean, was_forced_suspend, pid)
}

#[cfg(not(feature="simulation-only"))]
fn clear_ram(cfg: &mut BootConfig) {
    // clear RAM on a cold boot.
    // RAM is persistent and battery-backed. This means secret material could potentially
    // stay there forever, if not explicitly cleared. This clear adds a couple seconds
    // to a cold boot, but it's probably worth it. Note that it doesn't happen on a suspend/resume.
    let ram: *mut u32 = cfg.sram_start as *mut u32;
    unsafe {
        for addr in 0..(cfg.sram_size - 8192) / 4 { // 8k is reserved for our own stack
            ram.add(addr).write_volatile(0);
        }
    }
}

fn phase_1(cfg: &mut BootConfig) {
    // Allocate space for the stack pointer.
    // The bootloader should have placed the stack pointer at the end of RAM
    // prior to jumping to our program, so allocate one page of data for
    // stack.
    // All other allocations will be placed below the stack pointer.
    //
    // As of Xous 0.8, the top page is bootloader stack, and the page below that is the 'clean suspend' page.
    cfg.init_size += GUARD_MEMORY_BYTES;

    // The first region is defined as being "main RAM", which will be used
    // to keep track of allocations.
    println!("Allocating regions");
    allocate_regions(cfg);

    // The kernel, as well as initial processes, are all stored in RAM.
    println!("Allocating processes");
    allocate_processes(cfg);

    // Copy the arguments, if requested
    if cfg.no_copy {
        // TODO: place args into cfg.args
    } else {
        println!("Copying args");
        copy_args(cfg);
    }

    // All further allocations must be page-aligned.
    cfg.init_size = (cfg.init_size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

    // Additionally, from this point on all allocations come from
    // their respective processes rather than kernel memory.

    // Copy the processes to RAM, if requested.
    if !cfg.no_copy {
        println!("Copying processes");
        copy_processes(cfg);
    }

    // Mark all pages as in-use by the kernel.
    // NOTE: This causes the .text section to be owned by the kernel!  This
    // will require us to transfer ownership in `stage3`.
    // Note also that we skip the first four indices, causing the stack to be
    // returned to the process pool.

    // We also skip the an additional index as that is the clean suspend page. This
    // needs to be claimed by the susres server before the kernel allocates it.
    // Lower numbered indices corresponding to higher address pages.
    println!("Marking pages as in-use");
    for i in 4..(cfg.init_size / PAGE_SIZE) {
        cfg.runtime_page_tracker[cfg.sram_size / PAGE_SIZE - i] = 1;
    }
}

/// Stage 2 bootloader
/// This sets up the MMU and loads both PID1 and the kernel into RAM.
pub fn phase_2(cfg: &mut BootConfig) {
    let args = cfg.args;

    // This is the offset in RAM where programs are loaded from.
    let mut process_offset = cfg.sram_start as usize + cfg.sram_size - cfg.init_size;
    println!("\n\nPhase2: Processess start out @ {:08x}", process_offset);

    // Go through all Init processes and the kernel, setting up their
    // page tables and mapping memory to them.
    let mut pid = 2;
    for tag in args.iter() {
        if tag.name == u32::from_le_bytes(*b"IniE") {
            let inie = MiniElf::new(&tag);
            println!("\n\nCopying IniE program into memory");
            let allocated = inie.load(cfg, process_offset, pid, false);
            println!("IniE Allocated {:x}", allocated);
            process_offset -= allocated;
            pid += 1;
        } else if tag.name == u32::from_le_bytes(*b"IniF") {
            let inif = MiniElf::new(&tag);
            println!("\n\nMapping IniF program into memory");
            let allocated = inif.load(cfg, process_offset, pid, true);
            println!("IniF Allocated {:x}", allocated);
            process_offset -= allocated;
            pid += 1;
        } else if tag.name == u32::from_le_bytes(*b"XKrn") {
            println!("\n\nCopying kernel into memory");
            let xkrn = unsafe { &*(tag.data.as_ptr() as *const ProgramDescription) };
            let load_size_rounded = ((xkrn.text_size as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1))
                + (((xkrn.data_size + xkrn.bss_size) as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1));
            xkrn.load(cfg, process_offset - load_size_rounded, 1);
            process_offset -= load_size_rounded;
        }
    }

    println!("Done loading.");
    let krn_struct_start = cfg.sram_start as usize + cfg.sram_size - cfg.init_size;
    let krn_l1_pt_addr = cfg.processes[0].satp << 12;
    assert!(krn_struct_start & (PAGE_SIZE - 1) == 0);
    let krn_pg1023_ptr = unsafe { (krn_l1_pt_addr as *const usize).add(1023).read() };

    // Map boot-generated kernel structures into the kernel
    let satp = unsafe { &mut *(krn_l1_pt_addr as *mut PageTable) };
    for addr in (0..cfg.init_size-GUARD_MEMORY_BYTES).step_by(PAGE_SIZE as usize) {
        cfg.map_page(
            satp,
            addr + krn_struct_start,
            addr + KERNEL_ARGUMENT_OFFSET,
            FLG_R | FLG_W | FLG_VALID,
        );
        cfg.change_owner(1 as XousPid, (addr + krn_struct_start) as usize);
    }

    // Copy the kernel's "MMU Page 1023" into every process.
    // This ensures a context switch into the kernel can
    // always be made, and that the `stvec` is always valid.
    // Since it's a megapage, all we need to do is write
    // the one address to get all 4MB mapped.
    println!("Mapping MMU page 1023 to all processes");
    for process in cfg.processes[1..].iter() {
        let l1_pt_addr = process.satp << 12;
        unsafe { (l1_pt_addr as *mut usize).add(1023).write(krn_pg1023_ptr) };
    }

    if VVDBG {
        println!("PID1 pagetables:");
        debug::print_pagetable(cfg.processes[0].satp);
        println!();
        println!();
        for (_pid, process) in cfg.processes[1..].iter().enumerate() {
            println!("PID{} pagetables:", _pid + 2);
            debug::print_pagetable(process.satp);
            println!();
            println!();
        }
    }
    println!(
        "Runtime Page Tracker: {} bytes",
        cfg.runtime_page_tracker.len()
    );
    // mark pages used by suspend/resume according to their needs
    cfg.runtime_page_tracker[cfg.sram_size / PAGE_SIZE - 1] = 1; // claim the loader stack -- do not allow tampering, as it contains backup kernel args
    cfg.runtime_page_tracker[cfg.sram_size / PAGE_SIZE - 2] = 1; // 8k in total (to allow for digital signatures to be computed)
    cfg.runtime_page_tracker[cfg.sram_size / PAGE_SIZE - 3] = 0; // allow clean suspend page to be mapped in Xous
}

/// This function allows us to check the final loader results
/// It will print to the console the first 32 bytes of each loaded
/// region top/bottom, based upon extractions from the page table.
#[cfg(feature="debug-print")]
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
            inie.check(cfg, inie.load_offset as usize, pid, false);
            pid += 1;
        } else if tag.name == u32::from_le_bytes(*b"IniF") {
            let inif = MiniElf::new(&tag);
            println!("\n\nChecking IniF region");
            inif.check(cfg, inif.load_offset as usize, pid, true);
            pid += 1;
        }
    }
}
