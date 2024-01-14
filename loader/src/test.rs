use std::sync::Mutex;

use lazy_static::lazy_static;

use crate::BootConfig;

fn get_args_bin(idx: usize) -> &'static [u8] {
    match idx {
        0 => include_bytes!("../test/args-default.bin"),
        1 => include_bytes!("../test/args-span-page.bin"),
        _ => panic!("unrecognized args index"),
    }
}

const REGION_COUNT: usize = 8;
static mut MEMORY_REGIONS: [[usize; 1024 * 1024]; REGION_COUNT] = [[0usize; 1024 * 1024]; REGION_COUNT];
lazy_static! {
    static ref REGIONS_CHECKED_OUT: Mutex<[bool; REGION_COUNT]> = Mutex::new([false; REGION_COUNT]);
}

struct FakeMemory {
    pub region: &'static mut [usize; 1024 * 1024],
    index: usize,
}

impl FakeMemory {
    pub fn get() -> Self {
        unsafe {
            let mut store = REGIONS_CHECKED_OUT.lock().unwrap();
            let mut found_idx = None;
            for (idx, flag) in store.iter().enumerate() {
                if !flag {
                    found_idx = Some(idx);
                    break;
                }
            }
            let found_idx = found_idx.expect("no available memory regions found");
            println!("Checking out region {}", found_idx);
            store[found_idx] = true;
            FakeMemory { region: &mut MEMORY_REGIONS[found_idx], index: found_idx }
        }
    }
}

impl Drop for FakeMemory {
    fn drop(&mut self) {
        println!("Checking region {} back in", self.index);
        match REGIONS_CHECKED_OUT.lock() {
            Ok(mut o) => o[self.index] = false,
            Err(e) => println!("not checking in because of a panic: {:?}", e),
        }
    }
}

struct TestEnvironment {
    pub cfg: BootConfig,
    _mem: FakeMemory,
}

impl TestEnvironment {
    pub fn new(idx: usize) -> TestEnvironment {
        use crate::args::KernelArguments;

        // Create a fake memory block into which the bootloader will write
        let fake_memory = FakeMemory::get();
        // use rand::prelude::*;
        // for mem in fake_memory.region.iter_mut() {
        //     *mem = random();
        // }

        let args = get_args_bin(idx);
        #[allow(clippy::cast_ptr_alignment)] // This test only works on 32-bit systems
        let ka = KernelArguments::new(args.as_ptr() as *const usize);
        #[allow(clippy::cast_ptr_alignment)] // This test only works on 32-bit systems
        let mut cfg = BootConfig { args: ka, base_addr: ka.base as *const usize, ..Default::default() };
        crate::read_initial_config(&mut cfg);

        // Patch up the config memory address.  Ensure the range is on a "page" boundary.
        let raw_ptr = fake_memory.region.as_mut_ptr() as usize;
        let raw_ptr_rounded = (raw_ptr + crate::PAGE_SIZE - 1) & !(crate::PAGE_SIZE - 1);

        cfg.sram_start = raw_ptr_rounded as *mut _;
        cfg.sram_size = fake_memory.region.len() * core::mem::size_of::<usize>() - crate::PAGE_SIZE;

        println!(
            "Patching RAM so it starts at {:016x} and is {} bytes long",
            fake_memory.region.as_ptr() as usize,
            fake_memory.region.len() * core::mem::size_of::<usize>()
        );

        TestEnvironment { cfg, _mem: fake_memory }
    }
}

#[test]
fn copy_processes() {
    let mut env = TestEnvironment::new(0);
    crate::copy_processes(&mut env.cfg);
}

#[test]
fn allocate_regions() {
    let mut env = TestEnvironment::new(0);
    crate::copy_processes(&mut env.cfg);

    // The first region is defined as being "main RAM", which will be used
    // to keep track of allocations.
    println!("Allocating regions");
    crate::allocate_regions(&mut env.cfg);

    // The kernel, as well as initial processes, are all stored in RAM.
    println!("Allocating processes");
    crate::allocate_processes(&mut env.cfg);
}

#[test]
fn parse_args_bin() {
    use crate::args::KernelArguments;
    let args = get_args_bin(0);
    #[allow(clippy::cast_ptr_alignment)] // This test only works on 32-bit systems
    let ka = KernelArguments::new(args.as_ptr() as *const usize);

    let mut ka_iter = ka.iter();
    let ka_first = ka_iter.next().expect("kernel args has no first tag");
    assert_eq!(ka_first.name, u32::from_le_bytes(*b"XArg"), "first tag was not valid");
    assert_eq!(ka_first.size, 20, "first tag had invalid size");
    assert_eq!(ka_first.data[1], 1, "tag version number unexpected");

    for arg in ka_iter {
        let tag_name_bytes = arg.name.to_le_bytes();
        let s = unsafe {
            use core::slice;
            use core::str;
            // First, we build a &[u8]...
            let slice = slice::from_raw_parts(tag_name_bytes.as_ptr(), 4);
            // ... and then convert that slice into a string slice
            str::from_utf8(slice).expect("tag had invalid utf8 characters")
        };

        println!("{} ({:08x}, {} bytes):", s, arg.name, arg.size);
        for word in arg.data {
            println!(" {:08x}", word);
        }
    }
}

#[test]
fn read_initial_config() {
    use crate::args::KernelArguments;
    use crate::BootConfig;

    let args = get_args_bin(0);
    #[allow(clippy::cast_ptr_alignment)] // This test only works on 32-bit systems
    let ka = KernelArguments::new(args.as_ptr() as *const usize);
    #[allow(clippy::cast_ptr_alignment)] // This test only works on 32-bit systems
    let mut cfg = BootConfig { args: ka, base_addr: ka.base as *const usize, ..Default::default() };
    crate::read_initial_config(&mut cfg);
}

fn read_word(satp: usize, virt: usize) -> Result<u32, &'static str> {
    if satp & 0x8000_0000 != 0x8000_0000 {
        return Err("satp valid bit isn't set");
    }
    // let ppn1 = (phys >> 22) & ((1 << 12) - 1);
    // let ppn0 = (phys >> 12) & ((1 << 10) - 1);
    // let ppo = (phys >> 0) & ((1 << 12) - 1);

    let vpn1 = (virt >> 22) & ((1 << 10) - 1);
    let vpn0 = (virt >> 12) & ((1 << 10) - 1);
    let vpo = (virt) & ((1 << 12) - 1);

    let l1_pt = unsafe { &mut (*((satp << 12) as *mut crate::PageTable)) };
    let l1_entry = l1_pt.entries[vpn1];
    // FIXME: This could also be a megapage
    if l1_entry & 7 != 1 {
        return Err("l1 page table not mapped");
    }
    let l0_pt = unsafe { &mut (*(((l1_entry >> 10) << 12) as *mut crate::PageTable)) };
    let l0_entry = l0_pt.entries[vpn0];
    if l0_entry & 1 != 1 {
        return Err("l0 page table not mapped");
    }
    // println!("l0_entry: {:08x}", l0_entry);
    let page_base = (((l0_entry as u32) >> 10) << 12) + vpo as u32;
    // println!("virt {:08x} -> phys {:08x}", virt, page_base);
    Ok(unsafe { (page_base as *mut u32).read() })
}

fn read_byte(satp: usize, virt: usize) -> Result<u8, &'static str> {
    let word = read_word(satp, virt & 0xffff_fffc)?;
    Ok(word.to_le_bytes()[virt & 3])
}

fn verify_kernel(cfg: &BootConfig, pid: usize, arg: &crate::args::KernelArgument) {
    let prog = unsafe { &*(arg.data.as_ptr() as *const crate::ProgramDescription) };
    let program_offset = prog.load_offset as usize;
    let mut src_text = vec![];
    let mut src_data = vec![];
    {
        for i in 0..(prog.text_size as usize) {
            let word = unsafe { (cfg.base_addr as *mut u32).add((program_offset + i) / 4).read() };
            src_text.push(word);
        }

        for i in 0..(prog.data_size as usize) {
            let word = unsafe {
                (cfg.base_addr as *mut u32).add((program_offset + prog.text_size as usize + i) / 4).read()
            };
            src_data.push(word);
        }
    }
    println!(
        "Inspecting {} bytes of PID ({} bytes of text, {} bytes of data) {}, starting from {:08x}",
        src_text.len() * 4 + src_data.len() * 4,
        src_text.len() * 4,
        src_data.len() * 4,
        pid,
        prog.load_offset
    );
    for addr in (0..(prog.text_size as usize)).step_by(4) {
        assert_eq!(
            src_text[addr],
            read_word(cfg.processes[pid].satp as usize, addr + prog.text_offset as usize).unwrap(),
            "program text doesn't match @ offset {:08x}",
            addr + prog.text_offset as usize
        );
    }

    for addr in (0..(prog.data_size as usize)).step_by(4) {
        assert_eq!(
            src_data[addr],
            read_word(cfg.processes[pid].satp as usize, addr + prog.data_offset as usize).unwrap(),
            "program data doesn't match @ offset {:08x}",
            addr + prog.data_offset as usize
        );
    }

    for addr in ((prog.data_size as usize)..((prog.data_size + prog.bss_size) as usize)).step_by(4) {
        println!("Verifying BSS @ {:08x} is 0", addr + prog.data_offset as usize);
        assert_eq!(
            0,
            read_word(cfg.processes[pid].satp as usize, addr + prog.data_offset as usize).unwrap(),
            "bss is not zero @ offset {:08x}",
            addr + prog.data_offset as usize
        );
    }
}

fn verify_program(cfg: &BootConfig, pid: usize, arg: &crate::args::KernelArgument) {
    let elf = crate::MiniElf::new(arg);
    let mut program_offset = elf.load_offset as usize;

    for section in elf.sections.iter() {
        for addr in section.virt..(section.virt + section.len() as u32) {
            let addr = addr as usize;
            let word = read_byte(cfg.processes[pid].satp as usize, addr).unwrap();
            if section.no_copy() {
                assert!(word == 0, "bss is {:08x}, not 0 @ {:08x}", word, addr);
            } else {
                let check_word = unsafe { (cfg.base_addr as *mut u8).add(program_offset).read() };
                program_offset += 1;
                assert!(
                    word == check_word,
                    "program doesn't match @ {:08x} (expected: {:02x}  found: {:02x})",
                    addr,
                    check_word,
                    word,
                );
            }
        }
    }
}

#[test]
fn full_boot() {
    let mut env = TestEnvironment::new(0);

    println!("Running phase_1");
    crate::phase_1(&mut env.cfg);
    println!("Running phase_2");
    crate::phase_2(&mut env.cfg);
    println!("Done with phases");

    println!("Examining memory layout");
    let mut xkrn_inspected = false;
    let mut init_index = 0;
    for arg in env.cfg.args.iter() {
        if arg.name == u32::from_le_bytes(*b"XKrn") {
            verify_kernel(&env.cfg, 0, &arg);
            assert!(!xkrn_inspected, "multiple kernels found");
            xkrn_inspected = true;
            println!("Kernel PASS");
        } else if arg.name == u32::from_le_bytes(*b"IniE") {
            init_index += 1;
            verify_program(&env.cfg, init_index, &arg);
            println!("PID {} PASS", init_index + 1);
        }
    }
    assert_eq!(xkrn_inspected, true, "didn't see kernel in output list");
}

#[test]
fn spanning_section() {
    let mut env = TestEnvironment::new(1);

    println!("Running phase_1");
    crate::phase_1(&mut env.cfg);
    println!("Running phase_2");
    crate::phase_2(&mut env.cfg);
    println!("Done with phases");

    println!("Examining memory layout");
    let mut xkrn_inspected = false;
    let mut init_index = 0;
    for arg in env.cfg.args.iter() {
        if arg.name == u32::from_le_bytes(*b"XKrn") {
            verify_kernel(&env.cfg, 0, &arg);
            assert!(!xkrn_inspected, "multiple kernels found");
            xkrn_inspected = true;
            println!("Kernel PASS");
        } else if arg.name == u32::from_le_bytes(*b"IniE") {
            init_index += 1;
            verify_program(&env.cfg, init_index, &arg);
            println!("PID {} PASS", init_index + 1);
        }
    }
    assert_eq!(xkrn_inspected, true, "didn't see kernel in output list");
}

#[test]
fn tracker_sane() {
    let mut env = TestEnvironment::new(0);

    crate::phase_1(&mut env.cfg);
    crate::phase_2(&mut env.cfg);

    let mut max_pid = 0;
    for process in env.cfg.processes.iter() {
        let satp = process.satp;
        let pid = (satp >> 22 & ((1 << 9) - 1)) as u8;
        if pid > max_pid {
            max_pid = pid;
        }
        let mem_base = satp << 12;
        println!(
            "Process {} @ {:08x} ({:08x}), entrypoint {:08x}, sp {:08x}",
            pid, mem_base, satp, process.entrypoint, process.sp
        );
    }

    for (idx, addr) in env.cfg.runtime_page_tracker.iter().enumerate() {
        assert!(
            *addr <= max_pid,
            "runtime page tracker contains invalid values @ {} ({:08x})! {} > {}",
            idx,
            addr as *const u8 as usize,
            *addr,
            max_pid
        );
    }
}

// Create a fake "start_kernel" function to allow
// this module to compile when not running natively.
#[export_name = "start_kernel"]
pub unsafe extern "C" fn start_kernel(
    _args: usize,
    _ss: usize,
    _rpt: usize,
    _satp: usize,
    _entrypoint: usize,
    _stack: usize,
) -> ! {
    panic!("not running natively");
}
