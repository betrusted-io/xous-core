mod api;
mod hw;

use api::*;
use bitfield::*;
use cramium_api::*;
use cramium_hal::{iox::Iox, udma::GlobalConfig};
use num_traits::*;
use utralib::CSR;
#[cfg(feature = "quantum-timer")]
use utralib::utra;
#[cfg(feature = "quantum-timer")]
use utralib::*;
#[cfg(feature = "swap")]
use xous::SWAPPER_PID;
use xous::{ScalarMessage, sender::Sender};
#[cfg(feature = "pio")]
use xous_pio::*;

#[cfg(feature = "quantum-timer")]
struct PreemptionHw {
    pub timer_sm: PioSm,
    pub irq_csr: CSR<u32>,
}

#[cfg(feature = "quantum-timer")]
fn timer_tick(_irq_no: usize, arg: *mut usize) {
    let ptimer = unsafe { &mut *(arg as *mut PreemptionHw) };
    // this call forces preemption every timer tick
    // rsyscalls are "raw syscalls" -- used for syscalls that don't have a friendly wrapper around them
    // since ReturnToParent is only used here, we haven't wrapped it, so we use an rsyscall
    xous::rsyscall(xous::SysCall::ReturnToParent(xous::PID::new(1).unwrap(), 0))
        .expect("couldn't return to parent");

    // acknowledge the timer
    ptimer.timer_sm.sm_interrupt_clear(0);
    // clear the pending bit
    ptimer.irq_csr.wo(utra::irqarray18::EV_PENDING, ptimer.irq_csr.r(utra::irqarray18::EV_PENDING));
}

#[repr(u32)]
#[allow(dead_code)]
enum IntMode {
    RisingEdge = 0,
    FallingEdge = 1,
    HighLevel = 2,
    LowLevel = 3,
}
bitfield! {
    #[derive(Copy, Clone, PartialEq, Eq, Default)]
    pub struct IntCr(u32);
    impl Debug;
    pub u32, select, set_select: 6, 0;
    pub u32, mode, set_mode: 8, 7;
    pub enable, set_enable: 9;
    pub wakeup, set_wakeup: 10;
}
fn irq_select_from_port(port: IoxPort, pin: u8) -> u32 { (port as u32) * 16 + pin as u32 }
fn find_first_none<T>(arr: &[Option<T>]) -> Option<usize> { arr.iter().position(|item| item.is_none()) }

#[derive(Debug, Copy, Clone)]
#[allow(dead_code)]
struct IrqLocalRegistration {
    pub cid: xous::CID,
    pub opcode: usize,
    pub port: IoxPort,
    pub pin: u8,
    pub active: IoxValue,
}

struct IrqHandler {
    pub irq_csr: CSR<u32>,
    pub cid: xous::CID,
}
fn iox_irq_handler(_irq_no: usize, arg: *mut usize) {
    let handler = unsafe { &mut *(arg as *mut IrqHandler) };
    let pending = handler.irq_csr.r(utralib::utra::irqarray10::EV_PENDING);
    handler.irq_csr.wo(utralib::utra::irqarray10::EV_PENDING, pending);
    xous::try_send_message(
        handler.cid,
        xous::Message::Scalar(ScalarMessage::from_usize(
            HalOpcode::IrqLocalHandler.to_usize().unwrap(),
            pending as usize,
            0,
            0,
            0,
        )),
    )
    .ok();
}

fn try_alloc(ifram_allocs: &mut Vec<Option<Sender>>, size: usize, sender: Sender) -> Option<usize> {
    let mut size_pages = size / 4096;
    if size % 4096 != 0 {
        size_pages += 1;
    }
    log::trace!("try_alloc search for {} pages in alloc vector {:?}", size_pages, ifram_allocs);
    let mut free_start = None;
    let mut found_len = 0;
    for (index, page) in ifram_allocs.iter().enumerate() {
        log::trace!("Checking index {}: {:?}", index, page);
        if page.is_some() {
            log::trace!("Page was allocated, restarting search");
            free_start = None;
            found_len = 0;
            continue;
        } else {
            if free_start.is_some() {
                log::trace!("Adding unallocated page at {} to length", index);
                found_len += 1;
                if found_len >= size_pages {
                    break;
                }
            } else {
                log::trace!("Starting allocation search at {}", index);
                free_start = Some(index);
                found_len = 1;
            }
        }
    }
    if let Some(start) = free_start {
        if found_len >= size_pages {
            // starting point found, and enough pages
            assert!(
                found_len == size_pages,
                "Found pages should be exactly equal to size_pages at this point"
            );
            for i in ifram_allocs[start..start + found_len].iter_mut() {
                *i = Some(sender);
            }
            // offset relative to start of IFRAM bank
            Some(start * 4096)
        } else {
            // starting point found, but not enough pages
            None
        }
    } else {
        // no starting point found
        None
    }
}
fn main() {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let sid = xns.register_name(cramium_api::SERVER_NAME_CRAM_HAL, None).expect("can't register server");
    let self_cid = xous::connect(sid).expect("couldn't create self-connection");

    let mut ifram_allocs = [Vec::new(), Vec::new()];
    // code is written assuming the IFRAM blocks have the same size. Since this is fixed in
    // hardware, it's a good assumption; but the assert is put here in case we port this to
    // a new system where for some reason they have different sizes.
    assert!(utralib::generated::HW_IFRAM0_MEM_LEN == utralib::generated::HW_IFRAM1_MEM_LEN);
    let pages = utralib::generated::HW_IFRAM0_MEM_LEN / 4096;
    for _ in 0..pages {
        ifram_allocs[0].push(None);
        ifram_allocs[1].push(None);
    }
    // mark loader-hardwired pages for IFRAM0
    for i in
        cramium_hal::board::IFRAM0_RESERVED_PAGE_RANGE[0]..=cramium_hal::board::IFRAM0_RESERVED_PAGE_RANGE[1]
    {
        ifram_allocs[0][i] = Some(Sender::from_usize(usize::MAX));
    }
    // mark loader-hardwired pages for IFRAM1
    for i in
        cramium_hal::board::IFRAM1_RESERVED_PAGE_RANGE[0]..=cramium_hal::board::IFRAM1_RESERVED_PAGE_RANGE[1]
    {
        ifram_allocs[1][i] = Some(Sender::from_usize(usize::MAX));
    }
    // Second page from top of IFRAM0 is occupied by the swap handler. This was allocated
    // by the loader, before the kernel even started. Mark the PID properly.
    #[cfg(feature = "swap")]
    {
        ifram_allocs[0][30] = Some(Sender::from_usize(SWAPPER_PID as usize));
    }

    let iox_page = xous::syscall::map_memory(
        xous::MemoryAddress::new(utralib::generated::HW_IOX_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't claim the IOX hardware page");
    let iox = Iox::new(iox_page.as_ptr() as *mut u32);

    let udma_global_csr = xous::syscall::map_memory(
        xous::MemoryAddress::new(utralib::generated::HW_UDMA_CTRL_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map UDMA global control");
    let udma_global = GlobalConfig::new(udma_global_csr.as_mut_ptr() as *mut u32);

    // Note: the I2C handler can be put into a separate thread if we need the main
    // HAL server to not block while a large I2C transaction is being handled. For
    // now this is all placed into a single thread. However, if we ever had a situation
    // where, for example, you had to do a compound I2C transaction and flip a GPIO pin
    // in the middle of that transaction in order for the set of I2C transactions to
    // complete, this implementation would deadlock as it would block on the I2C transaction
    // before handling the GPIO request.
    let i2c_channel = cramium_hal::board::setup_i2c_pins(&iox);
    udma_global.clock_on(PeriphId::from(i2c_channel));
    let i2c_pages = xous::syscall::map_memory(
        xous::MemoryAddress::new(cramium_hal::board::I2C_IFRAM_ADDR),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't claim I2C IFRAM page");

    let i2c_ifram = unsafe {
        cramium_hal::ifram::IframRange::from_raw_parts(
            cramium_hal::board::I2C_IFRAM_ADDR,
            i2c_pages.as_ptr() as usize,
            i2c_pages.len(),
        )
    };
    let mut i2c = unsafe {
        cramium_hal::udma::I2c::new_with_ifram(
            i2c_channel,
            400_000,
            cramium_api::PERCLK,
            i2c_ifram,
            &udma_global,
        )
    };

    // -------------------- begin timer workaround code
    // This code should go away with NTO as we have a proper, private ticktimer unit.
    #[cfg(feature = "pio")]
    {
        let mut pio_ss = xous_pio::PioSharedState::new();
        // map and enable the interrupt for the PIO system timer
        let irq18_page = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::generated::HW_IRQARRAY18_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't claim irq18 csr");
        let mut ptimer = PreemptionHw {
            timer_sm: pio_ss.alloc_sm().unwrap(),
            irq_csr: CSR::new(irq18_page.as_mut_ptr() as *mut u32),
        };

        // claim the IRQ for the quanta timer
        xous::claim_interrupt(
            utralib::LITEX_IRQARRAY18_INTERRUPT,
            timer_tick,
            &mut ptimer as *mut PreemptionHw as *mut usize,
        )
        .expect("couldn't claim IRQ");

        pio_ss.clear_instruction_memory();
        pio_ss.pio.rmwf(utra::rp_pio::SFR_CTRL_EN, 0);
        #[rustfmt::skip]
        let timer_code = pio_proc::pio_asm!(
            "restart:",
            "set x, 6",  // 4 cycles overhead gets us to 10 iterations per pulse
            "waitloop:",
            "jmp x-- waitloop",
            "irq set 0",
            "jmp restart",
        );
        let a_prog = LoadedProg::load(timer_code.program, &mut pio_ss).unwrap();
        ptimer.timer_sm.sm_set_enabled(false);
        a_prog.setup_default_config(&mut ptimer.timer_sm);
        ptimer.timer_sm.config_set_clkdiv(50_000.0f32); // set to 1ms per cycle
        ptimer.timer_sm.sm_init(a_prog.entry());
        ptimer.timer_sm.sm_irq0_source_enabled(PioIntSource::Sm, true);
        ptimer.timer_sm.sm_set_enabled(true);

        #[cfg(feature = "quantum-timer")]
        {
            ptimer.irq_csr.wfo(utra::irqarray18::EV_ENABLE_PIOIRQ0_DUPE, 1);
            log::info!("Quantum timer setup!");
        }
    }
    // -------------------- end timer workaround code

    // ---- "own" the Iox IRQ bank. This might need revision once NTO aliasing is available. ---
    let irq_page = xous::syscall::map_memory(
        xous::MemoryAddress::new(utralib::utra::irqarray10::HW_IRQARRAY10_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't claim IRQ control page");
    let irq_csr = CSR::new(irq_page.as_mut_ptr() as *mut u32);
    let mut irq = IrqHandler { irq_csr, cid: self_cid };
    xous::claim_interrupt(
        utralib::utra::irqarray10::IRQARRAY10_IRQ,
        iox_irq_handler,
        &mut irq as *mut IrqHandler as *mut usize,
    )
    .expect("couldn't claim Iox interrupt");
    irq.irq_csr.wo(utralib::utra::irqarray10::EV_PENDING, 0xFFFF_FFFF);
    irq.irq_csr.wfo(utralib::utra::irqarray10::EV_ENABLE_IOXIRQ, 1);
    // Up to 8 slots where we can populate interrupt mappings in the hardware
    // The index of the array corresponds to the slot.
    let mut irq_table: [Option<IrqLocalRegistration>; 8] = [None; 8];

    // start keyboard emulator service
    hw::keyboard::start_keyboard_service();

    let mut msg_opt = None;
    log::debug!("Starting main loop");
    loop {
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        let msg = msg_opt.as_mut().unwrap();
        let opcode =
            num_traits::FromPrimitive::from_usize(msg.body.id()).unwrap_or(api::HalOpcode::InvalidCall);
        log::debug!("{:?}", opcode);
        match opcode {
            HalOpcode::MapIfram => {
                if let Some(scalar) = msg.body.scalar_message_mut() {
                    let requested_size = scalar.arg1; // requested size
                    let requested_bank = scalar.arg2; // Specifies bank 0, 1, or don't care (any number but 0 or 1)

                    let mut allocated_address = None;
                    for (bank, table) in ifram_allocs.iter_mut().enumerate() {
                        if bank == requested_bank || requested_bank > 1 {
                            match try_alloc(table, requested_size, msg.sender) {
                                Some(offset) => {
                                    let base = if bank == 0 {
                                        utralib::generated::HW_IFRAM0_MEM
                                    } else {
                                        utralib::generated::HW_IFRAM1_MEM
                                    };
                                    allocated_address = Some(base + offset);
                                    break;
                                }
                                None => {}
                            }
                        }
                    }
                    // responds with size in arg1 (0 means could not be allocated/OOM)
                    // and address of allocation in arg2
                    if let Some(addr) = allocated_address {
                        log::debug!(
                            "Allocated IFRAM at 0x{:x} to hold at least 0x{:x} bytes",
                            addr,
                            requested_size
                        );
                        log::debug!("Alloc[0]: {:x?}", ifram_allocs[0]);
                        log::debug!("Alloc[1]: {:x?}", ifram_allocs[1]);
                        scalar.arg1 = requested_size;
                        scalar.arg2 = addr;
                    } else {
                        log::debug!(
                            "Could not allocate IFRAM request of 0x{:x} bytes in bank {}",
                            requested_size,
                            requested_bank
                        );
                        scalar.arg1 = 0;
                        scalar.arg2 = 0;
                    }
                }
            }
            HalOpcode::UnmapIfram => {
                if let Some(scalar) = msg.body.scalar_message() {
                    let mapped_size = scalar.arg1;
                    let phys_addr = scalar.arg2;

                    let bank: usize;
                    let offset = if utralib::generated::HW_IFRAM0_MEM <= phys_addr
                        && phys_addr
                            < utralib::generated::HW_IFRAM0_MEM + utralib::generated::HW_IFRAM0_MEM_LEN
                    {
                        bank = 0;
                        phys_addr - utralib::generated::HW_IFRAM0_MEM
                    } else if utralib::generated::HW_IFRAM1_MEM <= phys_addr
                        && phys_addr
                            < utralib::generated::HW_IFRAM1_MEM + utralib::generated::HW_IFRAM1_MEM_LEN
                    {
                        bank = 1;
                        phys_addr - utralib::generated::HW_IFRAM1_MEM
                    } else {
                        log::error!("Mapped IFRAM address 0x{:x} is invalid", phys_addr);
                        panic!("Mapped IFRAM address is invalid");
                    };
                    let mut mapped_pages = mapped_size / 4096;
                    if mapped_size % 4096 != 0 {
                        mapped_pages += 1;
                    }
                    for record in ifram_allocs[bank][offset..offset + mapped_pages].iter_mut() {
                        *record = None;
                    }
                }
            }
            HalOpcode::ConfigureIox => {
                let buf =
                    unsafe { xous_ipc::Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let config = buf.to_original::<IoxConfigMessage, _>().unwrap();
                if let Some(f) = config.function {
                    iox.set_alternate_function(config.port, config.pin, f);
                }
                if let Some(d) = config.direction {
                    iox.set_gpio_dir(config.port, config.pin, d);
                }
                if let Some(t) = config.schmitt_trigger {
                    iox.set_gpio_schmitt_trigger(config.port, config.pin, t);
                }
                if let Some(p) = config.pullup {
                    iox.set_gpio_pullup(config.port, config.pin, p);
                }
                if let Some(s) = config.slow_slew {
                    iox.set_slow_slew_rate(config.port, config.pin, s);
                }
                if let Some(s) = config.strength {
                    iox.set_drive_strength(config.port, config.pin, s);
                }
            }
            HalOpcode::ConfigureIoxIrq => {
                let buf =
                    unsafe { xous_ipc::Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let registration = buf.to_original::<IoxIrqRegistration, _>().unwrap();
                log::info!("Got registration request: {:?}", registration);
                if let Some(index) = find_first_none(&irq_table) {
                    // create the reverse-lookup registration
                    let local_conn = xns
                        .request_connection(&registration.server)
                        .expect("couldn't connect to IRQ registree");
                    let local_reg = IrqLocalRegistration {
                        cid: local_conn,
                        opcode: registration.opcode,
                        port: registration.port,
                        pin: registration.pin,
                        active: registration.active,
                    };
                    irq_table[index] = Some(local_reg);

                    // now activate the hardware register
                    let select = irq_select_from_port(registration.port, registration.pin);
                    let mut int_cr = IntCr(0);
                    int_cr.set_select(select);
                    match registration.active {
                        IoxValue::Low => int_cr.set_mode(IntMode::FallingEdge as u32),
                        IoxValue::High => int_cr.set_mode(IntMode::RisingEdge as u32),
                    }
                    int_cr.set_enable(true);
                    // safety: the index and offset are mapped to the intended range because the index is
                    // bounded by the size of irq_table, and the offset comes from the generated header file.
                    log::debug!(
                        "writing {:x} to {:x} at index {}",
                        int_cr.0,
                        unsafe {
                            iox.csr.base().add(utralib::utra::iox::SFR_INTCR_CRINT0.offset()).add(index)
                                as usize
                        },
                        index
                    );
                    unsafe {
                        iox.csr
                            .base()
                            .add(utralib::utra::iox::SFR_INTCR_CRINT0.offset())
                            .add(index)
                            .write_volatile(int_cr.0);
                    }
                } else {
                    panic!("Ran out of Iox interrupt slots: maximum 8 available");
                }
            }
            HalOpcode::IrqLocalHandler => {
                // Figure out which port(s) caused the IRQ
                let irq_flag = iox.csr.r(utralib::utra::iox::SFR_INTFR);
                // clear the set bit by writing it back
                iox.csr.wo(utralib::utra::iox::SFR_INTFR, irq_flag);
                let mut found = false;
                for bitpos in 0..8 {
                    // the bit position is flipped versus register order in memory
                    if ((irq_flag << (bitpos as u32)) & 0x80) != 0 {
                        if let Some(local_reg) = irq_table[bitpos] {
                            found = true;
                            // interrupts are "Best effort" and can gracefully fail if the receiver has been
                            // overwhelmed by too many interrupts
                            xous::try_send_message(
                                local_reg.cid,
                                xous::Message::new_scalar(local_reg.opcode, 0, 0, 0, 0),
                            )
                            .ok();
                        } else {
                            log::warn!(
                                "Got IRQ on position {} but no registration was found, ignoring!",
                                bitpos
                            );
                        }
                    }
                }
                if !found {
                    log::warn!(
                        "No handler was found for raw flag: {:x} (note bit order is reversed)",
                        irq_flag
                    );
                }
            }
            HalOpcode::SetGpioBank => {
                if let Some(scalar) = msg.body.scalar_message() {
                    let port: IoxPort = num_traits::FromPrimitive::from_usize(scalar.arg1).unwrap();
                    let value = scalar.arg2 as u16;
                    let bitmask = scalar.arg3 as u16;
                    iox.set_gpio_bank(port, value, bitmask);
                }
            }
            HalOpcode::GetGpioBank => {
                if let Some(scalar) = msg.body.scalar_message_mut() {
                    let port: IoxPort = num_traits::FromPrimitive::from_usize(scalar.arg1).unwrap();
                    scalar.arg1 = iox.get_gpio_bank(port) as usize;
                }
            }
            HalOpcode::ConfigureUdmaClock => {
                if let Some(scalar) = msg.body.scalar_message() {
                    let periph: PeriphId = num_traits::FromPrimitive::from_usize(scalar.arg1).unwrap();
                    let enable = if scalar.arg2 != 0 { true } else { false };
                    if enable {
                        udma_global.clock_on(periph);
                    } else {
                        udma_global.clock_off(periph);
                    }
                }
            }
            HalOpcode::ConfigureUdmaEvent => {
                if let Some(scalar) = msg.body.scalar_message() {
                    let periph: PeriphId = num_traits::FromPrimitive::from_usize(scalar.arg1).unwrap();
                    let event_offset = scalar.arg2 as u32;
                    let to_channel: EventChannel =
                        num_traits::FromPrimitive::from_usize(scalar.arg3).unwrap();
                    // note: no "air traffic control" is done to prevent mapping other
                    // events. Maybe this should be done? but for now, let's leave it
                    // as bare iron.
                    udma_global.map_event_with_offset(periph, event_offset, to_channel);
                }
            }
            HalOpcode::PeriphReset => {
                if let Some(scalar) = msg.body.scalar_message() {
                    let periph: PeriphId = num_traits::FromPrimitive::from_usize(scalar.arg1).unwrap();
                    udma_global.reset(periph);
                }
            }
            HalOpcode::I2c => {
                let mut buf = unsafe {
                    xous_ipc::Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let mut list = buf.to_original::<I2cTransactions, _>().expect("I2c message format error");
                for transaction in list.transactions.iter_mut() {
                    match transaction.i2c_type {
                        I2cTransactionType::Write => {
                            match i2c.i2c_write(transaction.device, transaction.address, &transaction.data) {
                                Ok(b) => transaction.result = I2cResult::Ack(b),
                                _ => transaction.result = I2cResult::Nack,
                            }
                        }
                        I2cTransactionType::Read | I2cTransactionType::ReadRepeatedStart => {
                            match i2c.i2c_read(
                                transaction.device,
                                transaction.address,
                                &mut transaction.data,
                                transaction.i2c_type == I2cTransactionType::ReadRepeatedStart,
                            ) {
                                Ok(b) => transaction.result = I2cResult::Ack(b),
                                _ => transaction.result = I2cResult::Nack,
                            }
                        }
                    }
                }
                buf.replace(list).expect("I2c message format error");
            }
            HalOpcode::InvalidCall => {
                log::error!("Invalid opcode received: {:?}", msg);
            }
            HalOpcode::Quit => {
                log::info!("Received quit opcode, exiting.");
                break;
            }
        }
    }
    xns.unregister_server(sid).unwrap();
    xous::destroy_server(sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
