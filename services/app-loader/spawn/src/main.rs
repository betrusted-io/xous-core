#![no_std]
#![no_main]

enum StartupCommand {
    Unhandled = 0,
    LoadElf = 1,
    PingResponse = 2,
}

impl From<xous::MessageId> for StartupCommand {
    fn from(src: xous::MessageId) -> StartupCommand {
        match src {
            1 => StartupCommand::LoadElf,
	    2 => StartupCommand::PingResponse,
            _ => StartupCommand::Unhandled,
        }
    }
}

#[panic_handler]
fn handle_panic(arg: &core::panic::PanicInfo) -> ! {
    log::info!("{arg}");
    loop {}
}

#[no_mangle]
pub extern "C" fn init(server1: u32, server2: u32, server3: u32, server4: u32) -> ! {
    let server = xous::SID::from_u32(server1, server2, server3, server4);

    // recreate the extra sections that were cut out of the stub
    let mut memory = xous::map_memory(
	None,
	core::num::NonZeroUsize::new(0x40000000),
	0x1000,
	xous::MemoryFlags::R | xous::MemoryFlags::W
    ).unwrap();
    let connection = core::sync::atomic::AtomicU32::new(0);
    let slice = unsafe { core::slice::from_raw_parts(&connection as *const _ as *const u8, core::mem::size_of::<core::sync::atomic::AtomicU32>()) };
    for (dest, src) in memory.as_slice_mut::<u8>().iter_mut().skip(8).zip(slice) {
	*dest = *src;
    }
    
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());
    loop {
        if let Ok(xous::Result::MessageEnvelope(envelope)) =
            xous::rsyscall(xous::SysCall::ReceiveMessage(server))
        {
            match envelope.id().into() {
                StartupCommand::LoadElf => {
		    let entry_point = read_elf(envelope.body.memory_message());
		    drop(envelope); // we have to get rid of all messages to destroy the server
		    // destroy the server
		    xous::destroy_server(server).expect("Couldn't destroy spawn server");
		    jump(entry_point);
		},
                StartupCommand::PingResponse => ping_response(envelope),
                _ => panic!("Unsupported"),
            }
        }
    }
}

fn ping_response(envelope: xous::MessageEnvelope) {
    if let Some(msg) = envelope.body.scalar_message() {
        if envelope.body.is_blocking() {
            xous::syscall::return_scalar(envelope.sender, msg.arg1 + 1).unwrap();
        }
    }
}

fn read_elf(memory: Option<&xous::MemoryMessage>) -> usize {
    let memory = match memory {
        Some(s) => s,
        None => panic!(),
    };
    
    // get the elf binary from the message
    let mut bin = memory.buf.as_slice::<u8>();

    // go to the beginning of the ELF file. The chance of us accidentally finding a false
    // beginning is very small.
    while !(bin[0] == 0x7F && bin[1] == 0x45 && bin[2] == 0x4c && bin[3] == 0x46) {
	bin = &bin[1..];
    }
    
    let max = bin.len();

    // make sure that this is a 32 bit
    assert!(bin[4] == 0x01);

    // a helper function to get a region of the file as a usize
    let to_usize = |start, size| {
	if size == 1 {
	    return bin[start] as usize;
	}
	if size == 2 {
	    // assumes little endianness
	    return u16::from_le_bytes(bin[start..start+size].try_into().unwrap()) as usize;
	}
	if size == 4 {
	    return u32::from_le_bytes(bin[start..start+size].try_into().unwrap()) as usize;
	}
	panic!("Tried to get usize of invalid size!");
    };

    // some basic stuff to know
    let entry_point = to_usize(0x18, 4);
    let ph_start = to_usize(0x1c, 4);
    let ph_size = to_usize(0x2A, 2);
    let ph_count = to_usize(0x2C, 2);

    // add the segments we should load
    for i in 0..ph_count {
	let start = ph_start + i * ph_size;
	// only load PT_LOAD segments
	if  to_usize(start, 4) == 0x00000001 {
	    let src_addr = to_usize(start+0x04, 4);
	    let vaddr = to_usize(start+0x08, 4);
	    let padding = if vaddr & 0xFFF == 0 { 0 } else { vaddr & 0xFFF };
	    let mem_size = to_usize(start+0x14, 4);
	    let mem_size = mem_size + padding;
	    let mem_size = mem_size + if mem_size & 0xFFF == 0 { 0 } else { 0x1000 - (mem_size & 0xFFF) };

	    assert_eq!(0, mem_size & 0xFFF);
	    assert_eq!(0, (vaddr - padding) & 0xFFF);

	    log::info!("Loading offset {} to virtual address {} with memory size {}", src_addr, vaddr, mem_size);
	    let mut target_memory = xous::map_memory(
		None,
		core::num::NonZeroUsize::new(vaddr-padding),
		mem_size,
		xous::MemoryFlags::R | xous::MemoryFlags::W | xous::MemoryFlags::X,
	    )
		.unwrap();

	    for (dest, src) in target_memory.as_slice_mut().iter_mut().skip(padding)
		.zip(bin[src_addr..core::cmp::min(max, src_addr+mem_size-padding)].iter())
	    {
		*dest = *src;
	    }
	}
    }
    log::info!("Finished writing");
    return entry_point;
}

fn jump(entry_point: usize) -> ! {
    log::info!("Jumping to {}", entry_point);
    let entry_fn = unsafe { core::mem::transmute::<_, fn() -> !>(entry_point as *const u8) };
    entry_fn();
}
