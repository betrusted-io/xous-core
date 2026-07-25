#[allow(unused_imports)]
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
#[cfg(feature = "uf2-spim")]
use core::convert::TryInto;

use bao1x_api::pubkeys::BOOT0_TO_BOOT1;
#[allow(unused_imports)]
use bao1x_api::*;
use bao1x_hal::acram::{OneWayCounter, SlotManager};
use bao1x_hal::board::PDDB_LEN;
use bao1x_hal::hardening::{Csprng, skipping_enabled};
use utralib::*;

#[cfg(feature = "uf2-spim")]
use crate::platform::usb::glue::{PageCallback, write_spim_page};
#[cfg(feature = "uf2-spim")]
use crate::platform::usb::page_defrag::PageAssembler;

pub struct Error {
    pub message: Option<&'static str>,
}
impl Error {
    pub fn none() -> Self { Self { message: None } }

    pub fn help(message: &'static str) -> Self { Self { message: Some(message) } }
}

pub struct Repl {
    cmdline: String,
    do_cmd: bool,
    local_echo: bool,
    lockdown_armed: bool,
    perclk: u32,
    #[cfg(feature = "uf2-spim")]
    serial_assembler: PageAssembler<PageCallback>,
}

impl Repl {
    pub fn new(perclk: u32) -> Self {
        Self {
            cmdline: String::new(),
            do_cmd: false,
            local_echo: true,
            lockdown_armed: false,
            #[cfg(feature = "uf2-spim")]
            serial_assembler: PageAssembler::new(write_spim_page),
            perclk,
        }
    }

    #[allow(dead_code)]
    pub fn init_cmd(&mut self, cmd: &str) {
        self.cmdline.push_str(cmd);
        self.cmdline.push('\n');
        self.do_cmd = true;
    }

    pub fn rx_char(&mut self, c: u8) {
        if c == b'\r' {
            crate::println!("");
            // carriage return
            self.do_cmd = true;
        } else if c == b'\x08' {
            // backspace
            crate::print!("\u{0008}");
            if self.cmdline.len() != 0 {
                self.cmdline.pop();
            }
        } else {
            // everything else
            match char::from_u32(c as u32) {
                Some(c) => {
                    if self.local_echo {
                        crate::print!("{}", c);
                    }
                    self.cmdline.push(c);
                }
                None => {
                    crate::println!("Warning: bad char received, ignoring")
                }
            }
        }
    }

    pub fn process(&mut self) -> Result<(), Error> {
        if !self.do_cmd {
            return Err(Error::none());
        }
        // crate::println!("got {}", self.cmdline);

        let mut parts = self.cmdline.split_whitespace();
        let cmd = parts.next().unwrap_or("").to_string();
        let args: Vec<String> = parts.map(|s| s.to_string()).collect();

        // process two-phase lockdown command
        if self.lockdown_armed {
            if args.len() == 0 && cmd.as_str() == "YES" {
                let owc = OneWayCounter::new();
                let devkey_offsets = [
                    ("loader", LOADER_REVOCATION_OFFSET + bao1x_api::pubkeys::DEVELOPER_KEY_SLOT),
                    ("boot0", BOOT0_REVOCATION_OFFSET + bao1x_api::pubkeys::DEVELOPER_KEY_SLOT),
                    ("boot1", BOOT1_REVOCATION_OFFSET + bao1x_api::pubkeys::DEVELOPER_KEY_SLOT),
                    ("loader+", LOADER_REVOCATION_DUPE_OFFSET + bao1x_api::pubkeys::DEVELOPER_KEY_SLOT),
                    ("boot0+", BOOT0_REVOCATION_DUPE_OFFSET + bao1x_api::pubkeys::DEVELOPER_KEY_SLOT),
                    ("boot1+", BOOT1_REVOCATION_DUPE_OFFSET + bao1x_api::pubkeys::DEVELOPER_KEY_SLOT),
                    ("loader-pq", PQ_LOADER_REVOCATION_OFFSET + bao1x_api::pubkeys::DEVELOPER_KEY_SLOT),
                    ("boot0-pq", PQ_BOOT0_REVOCATION_OFFSET + bao1x_api::pubkeys::DEVELOPER_KEY_SLOT),
                    ("boot1-pq", PQ_BOOT1_REVOCATION_OFFSET + bao1x_api::pubkeys::DEVELOPER_KEY_SLOT),
                    ("loader-pq+", PQ_LOADER_REVOCATION_DUPE_OFFSET + bao1x_api::pubkeys::DEVELOPER_KEY_SLOT),
                    ("boot0-pq+", PQ_BOOT0_REVOCATION_DUPE_OFFSET + bao1x_api::pubkeys::DEVELOPER_KEY_SLOT),
                    ("boot1-pq+", PQ_BOOT1_REVOCATION_DUPE_OFFSET + bao1x_api::pubkeys::DEVELOPER_KEY_SLOT),
                    ("paranoid1", PARANOID_MODE), /* let's try CI testing with this active, and see how
                                                   * bad it is... */
                    ("paranoid2", PARANOID_MODE_DUPE),
                    ("pq", REQUIRE_PQ),
                    ("pq+", REQUIRE_PQ_DUPE),
                ];
                for &(desc, devkey) in devkey_offsets.iter() {
                    match unsafe { owc.inc(devkey) } {
                        Ok(_) => crate::println!("{} locked", desc),
                        Err(e) => crate::println!("Couldn't lock {}: {:?}", desc, e),
                    }
                }
            } else {
                crate::println!("Lockdown aborted.");
            }
            self.lockdown_armed = false;
            self.abort_cmd();
            return Ok(());
        }
        self.lockdown_armed = false;

        // now process any further commands
        match cmd.as_str() {
            "reset" => {
                let mut rcurst = CSR::new(utra::sysctrl::HW_SYSCTRL_BASE as *mut u32);
                rcurst.wo(utra::sysctrl::SFR_RCURST0, 0x55AA);
            }
            "boot" => {
                use bao1x_hal::iox::Iox;
                let one_way = OneWayCounter::new();
                let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
                let (port, pin) = match one_way.get_decoded::<bao1x_api::BoardTypeCoding>() {
                    // the default map is baosec in boot1
                    Ok(bao1x_api::BoardTypeCoding::Baosec) => bao1x_hal::board::setup_usb_pins(&iox),
                    // otherwise assume dabao mapping
                    _ => crate::setup_dabao_se0_pin(&iox),
                };

                // assert SE0 pin here. We add a delay even though crate:boot() calls this, because
                // a button press initiated SE0 includes a certain minimum "low"; a direct serial command
                // does not.
                iox.set_gpio_pin(port, pin, IoxValue::Low);
                crate::platform::delay(20); // minimum is 2.5ms

                // note: the SE0 pin is now asserted & configured as an output as it goes to the next stage
                // it us up to the next USB stack to de-assert this.
                let mut csprng = Csprng::new();
                crate::boot(&iox, None, port, pin, &mut csprng);
            }
            #[cfg(not(feature = "uf2-spim"))]
            "uf2" => {
                use base64::{Engine as _, engine::general_purpose};
                if args.len() != 1 {
                    crate::println_d!("u2f query malformed");
                    return Err(Error::help("uf2 [base64 data]"));
                }
                match general_purpose::STANDARD.decode(&args[0]) {
                    Ok(uf2_data) => {
                        if let Some(record) = crate::uf2::Uf2Block::from_bytes(&uf2_data) {
                            #[cfg(not(feature = "alt-boot1"))]
                            let low_limit = bao1x_api::BAREMETAL_START;
                            #[cfg(not(feature = "alt-boot1"))]
                            let high_limit = utralib::HW_RERAM_MEM + bao1x_api::RRAM_STORAGE_LEN;
                            #[cfg(feature = "alt-boot1")]
                            let low_limit = bao1x_api::BOOT1_START;
                            #[cfg(feature = "alt-boot1")]
                            let high_limit = bao1x_api::BAREMETAL_START;

                            if record.address() as usize >= low_limit
                                && (record.address() as usize) < high_limit
                                && record.family() == bao1x_api::BAOCHIP_1X_UF2_FAMILY
                            {
                                let mut rram = bao1x_hal::rram::Reram::new();
                                let offset = record.address() as usize - utralib::HW_RERAM_MEM;
                                match rram.write_slice(offset, record.data()) {
                                    Err(e) => crate::print_d!("Write error {:?} @ {:x}", e, offset),
                                    Ok(_) => (),
                                };
                                crate::println!("Wrote {} to 0x{:x}", record.data().len(), record.address());
                                crate::println_d!("{:x}", record.address());
                            } else {
                                crate::println!(
                                    "Invalid write address {:x}, block ignored!",
                                    record.address()
                                );
                            }
                        } else {
                            crate::println_d!("invalid u2f data");
                        }
                    }
                    Err(e) => {
                        crate::println_d!("Decode error {:?}", e);
                        return Err(Error::help("Corrupt base64"));
                    }
                }
                crate::usb::flush();
            }
            #[cfg(feature = "uf2-spim")]
            "has-crc" => {
                crate::println!("true");
            }
            #[cfg(feature = "uf2-spim")]
            "uf2" => {
                use core::sync::atomic::Ordering;

                use bao1x_api::*;
                use bao1x_hal::iox::Iox;
                use bao1x_hal::sh1107::Oled128x128;
                use bao1x_hal::udma::*;
                use base64::{Engine as _, engine::general_purpose};

                // Standard CRC-32 (IEEE 802.3 / zlib reflected, poly 0xEDB88320).
                // Matches Python's zlib.crc32. Bitwise is fine for 512-byte blocks.
                fn crc32(data: &[u8]) -> u32 {
                    let mut crc: u32 = 0xFFFF_FFFF;
                    for &b in data {
                        crc ^= b as u32;
                        for _ in 0..8 {
                            let mask = (crc & 1).wrapping_neg();
                            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
                        }
                    }
                    !crc
                }

                const UX_UPDATE_INTERVAL_BYTES: u32 = 0x1_0000;

                use crate::{APP_BYTES, BAREMETAL_BYTES, IS_BAOSEC, KERNEL_BYTES, SWAP_BYTES};
                const APP_RAM_ADDR: usize =
                    utralib::HW_RERAM_MEM + bao1x_api::offsets::dabao::APP_RRAM_OFFSET;
                const STORAGE_END_ADDR: usize = utralib::HW_RERAM_MEM + bao1x_api::RRAM_STORAGE_LEN;
                const SWAP_END_ADDR: usize =
                    bao1x_api::offsets::SWAP_START_UF2 + bao1x_api::offsets::SWAP_UF2_LEN;

                #[cfg(not(feature = "alt-boot1"))]
                const START_RANGE: usize = bao1x_api::BAREMETAL_START;
                #[cfg(feature = "alt-boot1")]
                const START_RANGE: usize = bao1x_api::BOOT1_START;

                if args.len() != 2 {
                    crate::println_d!("uf2 query malformed CRC");
                    return Err(Error::help("uf2 [base64 data] [crc32]"));
                }
                match general_purpose::STANDARD.decode(&args[0]) {
                    Ok(uf2_data) => {
                        let actual_crc = crc32(&uf2_data);
                        let expected_crc = u32::from_str_radix(args[1].trim(), 16).ok();
                        if expected_crc != Some(actual_crc) {
                            // Don't write. The sender's ACK regex won't match this line,
                            // and it explicitly detects "CRC error" for an immediate retry.
                            crate::println!("CRC error 0x{:08x}", actual_crc);
                        } else if let Some(record) = crate::uf2::Uf2Block::from_bytes(&uf2_data) {
                            #[cfg(not(feature = "alt-boot1"))]
                            let low_limit = bao1x_api::BAREMETAL_START;
                            #[cfg(not(feature = "alt-boot1"))]
                            let high_limit = utralib::HW_RERAM_MEM + bao1x_api::RRAM_STORAGE_LEN;
                            #[cfg(feature = "alt-boot1")]
                            let low_limit = bao1x_api::BOOT1_START;
                            #[cfg(feature = "alt-boot1")]
                            let high_limit = bao1x_api::BAREMETAL_START;

                            if record.address() as usize >= low_limit
                                && (record.address() as usize) < high_limit
                                && record.family() == bao1x_api::BAOCHIP_1X_UF2_FAMILY
                            {
                                let mut rram = bao1x_hal::rram::Reram::new();
                                let offset = record.address() as usize - utralib::HW_RERAM_MEM;
                                match rram.write_slice(offset, record.data()) {
                                    Err(e) => crate::print_d!("Write error {:?} @ {:x}", e, offset),
                                    Ok(_) => (),
                                };
                                crate::println!(
                                    "Wrote {} to 0x{:x} crc {:08x}",
                                    record.data().len(),
                                    record.address(),
                                    actual_crc
                                );
                            } else if record.address() as usize >= bao1x_api::SWAP_START_UF2
                                && (record.address() as usize)
                                    < bao1x_api::SWAP_START_UF2 + bao1x_api::SWAP_UF2_LEN
                                && record.family() == bao1x_api::BAOCHIP_1X_UF2_FAMILY
                            {
                                let spim_addr = record.address() & (bao1x_api::SWAP_UF2_LEN as u32 - 1);
                                crate::println!(
                                    "Wrote {} to 0x{:x} crc {:08x}",
                                    record.data().len(),
                                    record.address(),
                                    actual_crc
                                );
                                if let Err(e) = self
                                    .serial_assembler
                                    .add_page(spim_addr as usize, record.data().try_into().unwrap())
                                {
                                    crate::println!("Failed to add SPIM {:?}", e);
                                }
                            } else {
                                crate::println!(
                                    "Invalid write address {:x}, block ignored!",
                                    record.address()
                                );
                            }

                            let (partition, status) = if !IS_BAOSEC.load(Ordering::SeqCst) {
                                if matches!(record.address() as usize, START_RANGE..=APP_RAM_ADDR) {
                                    (
                                        "core",
                                        BAREMETAL_BYTES
                                            .fetch_add(record.data().len() as u32, Ordering::SeqCst),
                                    )
                                } else if matches!(record.address() as usize, APP_RAM_ADDR..=STORAGE_END_ADDR)
                                {
                                    ("app", APP_BYTES.fetch_add(record.data().len() as u32, Ordering::SeqCst))
                                } else {
                                    ("none", 0)
                                }
                            } else {
                                if matches!(record.address() as usize, START_RANGE..=KERNEL_START) {
                                    (
                                        "loader",
                                        BAREMETAL_BYTES
                                            .fetch_add(record.data().len() as u32, Ordering::SeqCst),
                                    )
                                } else if matches!(record.address() as usize, KERNEL_START..=STORAGE_END_ADDR)
                                {
                                    (
                                        "kernel",
                                        KERNEL_BYTES.fetch_add(record.data().len() as u32, Ordering::SeqCst),
                                    )
                                } else if matches!(
                                    record.address() as usize,
                                    bao1x_api::offsets::SWAP_START_UF2..=SWAP_END_ADDR
                                ) {
                                    (
                                        "swap",
                                        SWAP_BYTES.fetch_add(record.data().len() as u32, Ordering::SeqCst),
                                    )
                                } else {
                                    ("none", 0)
                                }
                            };
                            if status != 0 && status % UX_UPDATE_INTERVAL_BYTES == 0 {
                                let mut wdt = bao1x_hal::wdt::Wdt::new();
                                wdt.feed();

                                if IS_BAOSEC.load(Ordering::SeqCst) {
                                    // conjure a pointer to the sh1107 object
                                    let iox = Iox::new(utralib::utra::iox::HW_IOX_BASE as *mut u32);
                                    let (channel, _, _, _) = bao1x_hal::board::get_display_pins();
                                    // these parameters are copied out of the sh1107 driver. Maybe we should
                                    // just create a convenience function
                                    // that "just sets these" since
                                    // hardware peripherals don't
                                    // spontaneously move around, and when they do you'd like to have a single
                                    // spot to maintain the changes...
                                    let mut sh1107 = unsafe {
                                        Oled128x128::from_raw_parts(
                                            (
                                                (
                                                    match channel {
                                                        SpimChannel::Channel0 => {
                                                            utra::udma_spim_0::HW_UDMA_SPIM_0_BASE
                                                        }
                                                        SpimChannel::Channel1 => {
                                                            utra::udma_spim_1::HW_UDMA_SPIM_1_BASE
                                                        }
                                                        SpimChannel::Channel2 => {
                                                            utra::udma_spim_2::HW_UDMA_SPIM_2_BASE
                                                        }
                                                        SpimChannel::Channel3 => {
                                                            utra::udma_spim_3::HW_UDMA_SPIM_3_BASE
                                                        }
                                                    },
                                                    SpimCs::Cs0,
                                                    0,
                                                    0,
                                                    None,
                                                    SpimMode::Standard,
                                                    SpimByteAlign::Disable,
                                                    bao1x_hal::ifram::IframRange::from_raw_parts(
                                                        bao1x_hal::board::DISPLAY_IFRAM_ADDR,
                                                        bao1x_hal::board::DISPLAY_IFRAM_ADDR,
                                                        4096 * 2,
                                                    ),
                                                    2048 + 256,
                                                    2048,
                                                    0,
                                                    None,
                                                ),
                                                false,
                                                ((100_000_000 / 2) / 2_000_000) as u8,
                                            ),
                                            &iox,
                                        )
                                    };
                                    // have to restore this because the frame buffer is lost on the raw-parts
                                    // conversion
                                    sh1107.blit_screen(&ux_api::bitmaps::baochip128x128::BITMAP);
                                    let msg = alloc::format!("{} - {}k", partition, status / 1024);
                                    crate::marquee(&mut sh1107, &msg);
                                } else {
                                    crate::println_d!("{} - {}k", partition, status / 1024);
                                }
                            }
                        } else {
                            crate::println_d!("invalid u2f data");
                        }
                    }
                    Err(e) => {
                        crate::println_d!("Decode error {:?}", e);
                        return Err(Error::help("Corrupt base64"));
                    }
                }
                crate::usb::flush();
            }
            // callers *must* use this to flush SPIM writes after upload is done. This is NOT automatic on
            // boot.
            #[cfg(feature = "uf2-spim")]
            "uf2_flush" => {
                if self.serial_assembler.active_pages() > 0 {
                    loop {
                        if let Some((addr, data)) = self.serial_assembler.take_next_incomplete() {
                            // the "holes" will just have 0 in them, which is fine for these purposes
                            // the primary case that triggers this is when the last sector written doesn't
                            // fill up a whole page.
                            crate::println!("Flushing final swap page at {:x}", addr);
                            crate::glue::write_spim_page(addr, data);
                        } else {
                            break;
                        }
                    }
                }
            }
            "localecho" => {
                if args.len() != 1 {
                    return Err(Error::help("localecho [on | off]"));
                }
                if args[0] == "on" {
                    self.local_echo = true;
                } else {
                    self.local_echo = false;
                }
            }
            "bootwait" => {
                let one_way = OneWayCounter::new();
                if args.len() != 1 {
                    return Err(Error::help("bootwait [check | toggle | enable | disable]"));
                }
                if args[0] == "toggle" {
                    // this toggles the bootwait flag by incrementing its one-way counter
                    match one_way.inc_coded::<bao1x_api::BootWaitCoding>() {
                        Ok(_) => {
                            let state = one_way
                                .get_decoded::<bao1x_api::BootWaitCoding>()
                                .expect("couldn't fetch flag");
                            crate::println!("bootwait is now set to {:?}", state);
                        }
                        Err(e) => crate::println!("Couldn't toggle bootwait: {:?}", e),
                    }
                } else if args[0] == "check" {
                    let state =
                        one_way.get_decoded::<bao1x_api::BootWaitCoding>().expect("couldn't fetch flag");
                    crate::println!("bootwait is {:?}", state);
                } else if args[0] == "enable" {
                    while one_way.get_decoded::<bao1x_api::BootWaitCoding>().expect("couldn't fetch flag")
                        != bao1x_api::BootWaitCoding::Enable
                    {
                        one_way.inc_coded::<bao1x_api::BootWaitCoding>().unwrap();
                    }
                } else if args[0] == "disable" {
                    while one_way.get_decoded::<bao1x_api::BootWaitCoding>().expect("couldn't fetch flag")
                        != bao1x_api::BootWaitCoding::Disable
                    {
                        one_way.inc_coded::<bao1x_api::BootWaitCoding>().unwrap();
                    }
                } else {
                    return Err(Error::help("bootwait [check | toggle | enable | disable]"));
                }
            }
            "paranoid" => {
                let one_way = OneWayCounter::new();
                if args.len() != 1 {
                    return Err(Error::help(
                        "paranoid [check | enable] (Note: it cannot be unset once set!)",
                    ));
                }
                if args[0] == "check" {
                    let state = one_way.get(bao1x_api::PARANOID_MODE).unwrap() != 0
                        || one_way.get(bao1x_api::PARANOID_MODE_DUPE).unwrap() != 0;
                    crate::println!("paranoid mode is {:?} (Note: it cannot be unset once set!)", state);
                } else if args[0] == "enable" {
                    unsafe {
                        one_way.inc(bao1x_api::PARANOID_MODE).unwrap();
                        one_way.inc(bao1x_api::PARANOID_MODE_DUPE).unwrap();
                    }
                } else {
                    return Err(Error::help(
                        "paranoid [check | enable] (Note: it cannot be unset once set!)",
                    ));
                }
            }
            "skipping" => {
                let slot_mgr = SlotManager::new();
                if args.len() != 1 {
                    return Err(Error::help("skipping [check | enable | disable]"));
                }
                if args[0] == "check" {
                    let skipping_cfg = slot_mgr.read(&bao1x_api::CLOCK_SCRAMBLE_PARAMS).unwrap();
                    crate::println!("Clock skipping: {:?}", skipping_enabled(skipping_cfg));
                } else if args[0] == "enable" {
                    bao1x_hal::hardening::enable_skipping();
                } else if args[0] == "disable" {
                    bao1x_hal::hardening::disable_skipping();
                } else {
                    return Err(Error::help("skipping [check | enable | disable]"));
                }
            }
            #[cfg(feature = "qe-debug")]
            "qe" => {
                use bao1x_hal::{
                    ifram::IframRange,
                    iox::Iox,
                    udma::{Spim, *},
                };
                let perclk = 100_000_000;
                let udma_global = GlobalConfig::new();

                // setup the I/O pins
                let iox = Iox::new(utralib::generated::HW_IOX_BASE as *mut u32);
                let channel = bao1x_hal::board::setup_memory_pins(&iox);
                udma_global.clock_on(PeriphId::from(channel));
                // safety: this is safe because clocks have been set up
                let mut flash_spim = unsafe {
                    Spim::new_with_ifram(
                        channel,
                        // has to be half the clock frequency reaching the block, but
                        // run it as fast
                        // as we can run perclk
                        perclk / 4,
                        perclk / 2,
                        SpimClkPol::LeadingEdgeRise,
                        SpimClkPha::CaptureOnLeading,
                        SpimCs::Cs0,
                        0,
                        0,
                        None,
                        256 + 16, /* just enough space to send commands + programming
                                   * page */
                        4096,
                        Some(6),
                        Some(SpimMode::Standard), // guess Standard
                        IframRange::from_raw_parts(
                            bao1x_hal::board::SPIM_FLASH_IFRAM_ADDR,
                            bao1x_hal::board::SPIM_FLASH_IFRAM_ADDR,
                            4096 * 2,
                        ),
                    )
                };
                let init_id = flash_spim.identify_flash_reset_qpi();
                crate::println!("boot id: {:x}", init_id);
                // turn off QPI mode, in case it was set from a reboot in a bad state
                flash_spim.mem_qpi_mode(false);

                // sanity check: read ID
                let flash_id = flash_spim.mem_read_id_flash();
                crate::println!("flash ID (init): {:x}", flash_id);
                flash_spim.mem_qpi_mode(true);

                // re-check the ID to confirm we entered QPI mode correctly
                let flash_id = flash_spim.mem_read_id_flash();
                crate::println!("QPI flash ID: {:x}", flash_id);
                flash_spim.mem_qpi_mode(false);
                let flash_id = flash_spim.mem_read_id_flash();
                crate::println!("SPI flash ID: {:x}", flash_id);
                flash_spim.mem_qpi_mode(true);
                let flash_id = flash_spim.mem_read_id_flash();
                crate::println!("QPI flash ID: {:x}", flash_id);
            }
            #[cfg(feature = "test-clock-skipping")]
            "bogomips" => {
                crate::println!("start test");
                bao1x_hal::hardening::enable_skipping();
                // start the RTC
                let mut ao_sysctrl = CSR::new(utralib::HW_AO_SYSCTRL_BASE as *mut u32);
                ao_sysctrl.wo(utra::ao_sysctrl::CR_CLK1HZFD, 0x3fff);
                unsafe { (0x4006100c as *mut u32).write_volatile(1) };
                let mut count: usize;
                unsafe {
                    #[rustfmt::skip]
                    core::arch::asm!(
                        // grab the RTC value
                        "li t0, 0x40061000",
                        "lw t1, 0x0(t0)",
                        "li t3, 0",
                        // wait until the next second
                    "10:",
                        "lw t2, 0x0(t0)",
                        "beq t1, t2, 10b",
                        // start of test
                    "20:",
                        // count outer loops
                        "addi t3, t3, 1",
                        // inner loop 10,000 times
                        "li t4, 10000",
                    "30:",
                        "addi t4, t4, -1",
                        "bne  x0, t4, 30b",
                        // after inner loop, check current time; do another outer loop if time is same
                        "lw t1, 0x0(t0)",
                        "beq t1, t2, 20b",
                        out("t0") _,
                        out("t1") _,
                        out("t2") _,
                        out("t3") count,
                        out("t4") _,
                    );
                }
                crate::println!("{}.{} bogomips", (count * 2 * 10_000) / 1_000_000, (count * 2) % 10_000);
                bao1x_hal::hardening::disable_skipping();
                ao_sysctrl.wo(utra::ao_sysctrl::CR_CLK1HZFD, 15);
            }
            "boardtype" => {
                let one_way = OneWayCounter::new();
                if args.len() == 0 {
                    crate::println!(
                        "Board type is set to: {:?}",
                        one_way.get_decoded::<bao1x_api::BoardTypeCoding>().expect("owc coding error")
                    );
                    self.abort_cmd();
                    return Ok(());
                } else if args.len() != 1 {
                    return Err(Error::help("boardtype [dabao | baosec | oem]"));
                }
                let new_type = match args[0].as_str() {
                    "dabao" => bao1x_api::BoardTypeCoding::Dabao,
                    "baosec" => bao1x_api::BoardTypeCoding::Baosec,
                    "oem" => bao1x_api::BoardTypeCoding::Oem,
                    _ => return Err(Error::help("boardtype [dabao | baosec | oem]")),
                };
                let mut count = 0;
                while one_way.get_decoded::<bao1x_api::BoardTypeCoding>().expect("owc coding error")
                    != new_type
                {
                    one_way.inc_coded::<bao1x_api::BoardTypeCoding>().expect("increment error");
                    count += 1;
                }
                crate::println!("Board type set to {:?} after {} increments", new_type, count);
                crate::platform::slots::check_slots();
                crate::println!("Key & data slots checked according to the new type");
            }
            "altboot" => {
                let owc = OneWayCounter::new();
                if args.len() == 0 {
                    crate::println!("Boot partition is: {:?}", owc.get_decoded::<AltBootCoding>());
                    self.abort_cmd();
                    return Ok(());
                } else if args.len() != 1 {
                    return Err(Error::help("altboot [toggle]"));
                }
                if args[0] == "toggle" {
                    owc.inc_coded::<bao1x_api::AltBootCoding>().unwrap();
                    crate::println!("Boot partition is now: {:?}", owc.get_decoded::<AltBootCoding>());
                } else {
                    return Err(Error::help("altboot [toggle]"));
                }
            }
            "idmode" => {
                let owc = OneWayCounter::new();
                if args.len() == 0 {
                    crate::println!("ID mode is: {:?}", owc.get_decoded::<ExternalIdentifiers>());
                    self.abort_cmd();
                    return Ok(());
                } else if args.len() != 1 {
                    return Err(Error::help("idmode [toggle]"));
                }
                if args[0] == "toggle" {
                    owc.inc_coded::<ExternalIdentifiers>().unwrap();
                    crate::println!("ID mode is now: {:?}", owc.get_decoded::<ExternalIdentifiers>());
                } else {
                    return Err(Error::help("idmode [toggle]"));
                }
            }
            "audit" => {
                crate::audit::audit();
            }
            "lockdown" => match bao1x_hal::sigcheck::validate_image(BOOT0_TO_BOOT1, None, None) {
                Ok((k, _k2, _tag, _target, _pq)) => {
                    if k != bao1x_api::pubkeys::DEVELOPER_KEY_SLOT {
                        crate::println!("This will permanently disable developer mode. It cannot be undone!");
                        crate::println!("Proceed? (type 'YES' in all caps to proceed)");
                        self.lockdown_armed = true;
                    } else {
                        crate::println!(
                            "Boot1 is signed with the developer key. Refusing to lockdown, as that would brick the chip."
                        )
                    }
                }
                Err(_e) => {
                    crate::println!("Boot1 has no valid signature, lockdown would brick the chip.")
                }
            },
            "self_destruct" => {
                if !matches!(args.as_slice(), [s] if s == "void_my_warrantee") {
                    return Err(Error::help(
                        "Usage: 'self_destruct void_my_warrantee'. This PERMANENTLY wipes the chip and bricks it. No returns or exchanges are allowed after executing this command.",
                    ));
                }
                let mut rram = bao1x_hal::rram::Reram::new();
                unsafe { rram.self_destruct() }
                // ... and all was null and void!
            }
            "require-pq" => {
                match args.as_slice() {
                    [s] if s == "confirm" => false,
                    _ => {
                        return Err(Error::help(
                            "Usage: 'require-pq confirm'. This command disallows firmwares without a valid PQ signature. WARNING: cannot be undone!",
                        ));
                    }
                };
                let one_way = OneWayCounter::new();
                // safety: the offsets come from the API and are guaranteed to be valid
                unsafe {
                    one_way.inc(bao1x_api::REQUIRE_PQ).unwrap();
                    one_way.inc(bao1x_api::REQUIRE_PQ_DUPE).unwrap();
                }
            }
            "baosec-init" => {
                let full = match args.as_slice() {
                    [s] if s == "confirm" => false,
                    [s, f] if s == "confirm" && f == "full" => true,
                    _ => {
                        return Err(Error::help(
                            "Usage: 'baosec-init confirm [full]'. WARNING: erases external storage!",
                        ));
                    }
                };

                // this routine is used to initialize baosec products - sets the board type and
                // erases the off-chip FLASH
                use bao1x_api::baosec::PDDB_ORIGIN;
                use bao1x_hal::{
                    board::SPINOR_BULK_ERASE_SIZE,
                    ifram::IframRange,
                    iox::Iox,
                    udma::{Spim, *},
                };
                let perclk = 100_000_000;
                let udma_global = GlobalConfig::new();

                // setup the I/O pins
                let iox = Iox::new(utralib::generated::HW_IOX_BASE as *mut u32);
                let channel = bao1x_hal::board::setup_memory_pins(&iox);
                udma_global.clock_on(PeriphId::from(channel));
                // safety: this is safe because clocks have been set up
                let mut flash_spim = unsafe {
                    Spim::new_with_ifram(
                        channel,
                        // has to be half the clock frequency reaching the block, but
                        // run it as fast
                        // as we can run perclk
                        perclk / 4,
                        perclk / 2,
                        SpimClkPol::LeadingEdgeRise,
                        SpimClkPha::CaptureOnLeading,
                        SpimCs::Cs0,
                        0,
                        0,
                        None,
                        256 + 16, /* just enough space to send commands + programming
                                   * page */
                        4096,
                        Some(6),
                        Some(SpimMode::Standard), // guess Standard
                        IframRange::from_raw_parts(
                            bao1x_hal::board::SPIM_FLASH_IFRAM_ADDR,
                            bao1x_hal::board::SPIM_FLASH_IFRAM_ADDR,
                            4096 * 2,
                        ),
                    )
                };
                flash_spim.identify_flash_reset_qpi();
                let flash_id = flash_spim.mem_read_id_flash();
                crate::println!("flash ID (init): {:x}", flash_id);
                if !SPI_FLASH_IDS.contains(&(flash_id & 0xFF_FF_FF)) {
                    return Err(Error::help("Supported SPI device not found. Aborting operation."));
                }

                crate::println!("Erasing headers...");
                for addr in (0..SPINOR_BULK_ERASE_SIZE as usize * 2).step_by(SPINOR_BULK_ERASE_SIZE as usize)
                {
                    crate::println!("  {:x}...", addr);
                    flash_spim.flash_erase_block(addr, SPINOR_BULK_ERASE_SIZE as usize);
                }
                let pddb_erase_len =
                    if full { PDDB_LEN as usize } else { SPINOR_BULK_ERASE_SIZE as usize * 2 };
                for addr in
                    (PDDB_ORIGIN..PDDB_ORIGIN + pddb_erase_len).step_by(SPINOR_BULK_ERASE_SIZE as usize)
                {
                    crate::println!("  {:x}...", addr);
                    flash_spim.flash_erase_block(addr, SPINOR_BULK_ERASE_SIZE as usize);
                }
                crate::println!("...done!");
                let one_way = bao1x_hal::acram::OneWayCounter::new();
                let board_type =
                    one_way.get_decoded::<bao1x_api::BoardTypeCoding>().expect("Board type coding error");
                #[cfg(not(feature = "oem-baosec-lite"))]
                if board_type != bao1x_api::BoardTypeCoding::Baosec {
                    while one_way.get_decoded::<bao1x_api::BoardTypeCoding>().expect("owc coding error")
                        != bao1x_api::BoardTypeCoding::Baosec
                    {
                        one_way.inc_coded::<bao1x_api::BoardTypeCoding>().expect("increment error");
                    }
                }
                #[cfg(feature = "oem-baosec-lite")]
                if board_type != bao1x_api::BoardTypeCoding::Oem {
                    while one_way.get_decoded::<bao1x_api::BoardTypeCoding>().expect("owc coding error")
                        != bao1x_api::BoardTypeCoding::Oem
                    {
                        one_way.inc_coded::<bao1x_api::BoardTypeCoding>().expect("increment error");
                    }
                }
                // reset the USB stack so that we'll re-enumerate correctly after this reboot.
                // This also has the side-effect of redirecting the console output back to the serial port.
                crate::platform::usb::glue::shutdown();
                let (se0_port, se0_pin) = bao1x_hal::board::setup_usb_pins(&iox);
                iox.set_gpio_dir(se0_port, se0_pin, bao1x_api::IoxDir::Output);
                iox.set_gpio_pin(se0_port, se0_pin, bao1x_api::IoxValue::Low); // put the USB port into SE0, so we re-enumerate with the OS stack

                use bao1x_hal::board::{BOOKEND_END, BOOKEND_START};
                // CI note: this appears on the "hard UART", not on USB serial. If we want this on USB
                // serial, we would want to add some wait time to ensure the USB packets get sent before
                // issuing the reboot command.
                #[cfg(not(feature = "oem-baosec-lite"))]
                {
                    crate::println!("{}BOOT1.SETBOARD,{}", BOOKEND_START, BOOKEND_END);
                    crate::println!("Board type set to baosec");
                }
                #[cfg(feature = "oem-baosec-lite")]
                {
                    crate::println!("Board type set to baosec-lite");
                    crate::println!("{}BOOT1.SETBOARD-LITE,{}", BOOKEND_START, BOOKEND_END);
                }
            }
            "ifr" => {
                // safety: the IFR region is aligned and exists here. It is sealed by hardware in USER mode,
                // and should report as all 0's.
                let ifr = unsafe { core::slice::from_raw_parts(0x6040_0000 as *const u8, 0x400) };
                for (i, chunk) in ifr.chunks(32).enumerate() {
                    // these "redundant" asserts make it harder to abuse this print as a memory dumping
                    // primitive, e.g. by glitching or other similar attack
                    assert!(ifr.as_ptr() as usize == 0x6040_0000);
                    crate::println!("  {:03x}: {:02x?}", i * 32, chunk);
                    assert!(i < 32);
                }
            }
            #[cfg(feature = "test-boot0-keys")]
            "rand_collateral" => {
                use bao1x_hal::acram::AccessSettings;
                // put random data in collateral - to simulate a third party keying
                let slot_mgr = bao1x_hal::acram::SlotManager::new();
                let mut rram = bao1x_hal::rram::Reram::new();
                let slot = &bao1x_api::offsets::COLLATERAL;
                let mut trng = super::trng::ManagedTrng::new();
                // only clear ACL if it isn't already cleared
                if slot_mgr
                    .get_acl(slot)
                    .unwrap_or(AccessSettings::Data(DataSlotAccess::new_with_raw_value(0xFFFF_FFFF)))
                    .raw_u32()
                    != 0
                {
                    // clear the ACL so we can operate on the data
                    slot_mgr
                        .set_acl(
                            &mut rram,
                            slot,
                            &AccessSettings::Data(DataSlotAccess::new_with_raw_value(0)),
                        )
                        .expect("couldn't reset ACL");
                }
                let mut random: Vec<u8> = alloc::vec::Vec::with_capacity(slot.len() * SLOT_ELEMENT_LEN_BYTES);
                random.resize(slot.len() * SLOT_ELEMENT_LEN_BYTES, 0);
                for chunk in random.chunks_mut(SLOT_ELEMENT_LEN_BYTES) {
                    let r = trng.generate_key();
                    chunk.copy_from_slice(&r);
                }

                slot_mgr.write(&mut rram, slot, &random).ok();

                let bytes = unsafe { slot_mgr.read_unchecked(slot) };
                crate::println!("Random test data:");
                for (i, chunk) in bytes.chunks(32).enumerate() {
                    crate::println!("  {:03x}: {:02x?}", i * 32, chunk);
                }
            }
            #[cfg(feature = "test-boot0-keys")]
            "publock" => {
                let rram = CSR::new(utra::rrc::HW_RRC_BASE as *mut u32);
                crate::println!("RRAM security settings: {:x}", rram.rf(utra::rrc::SFR_RRCCR_SFR_RRCCR));

                use bao1x_hal::acram::AccessSettings;
                let keys = [
                    bao1x_api::BAO1_PUBKEY,
                    bao1x_api::BAO2_PUBKEY,
                    bao1x_api::BETA_PUBKEY,
                    bao1x_api::DEV_PUBKEY,
                ];
                let ifr_slot = unsafe { core::slice::from_raw_parts(0x6040_0340 as *const u8, 32) };
                crate::println!("IFR permissions at 0x340: {:x?}", ifr_slot);
                let slot_mgr = bao1x_hal::acram::SlotManager::new();
                let mut rram = bao1x_hal::rram::Reram::new();
                // some value that's not 0, so we can differentiate it from access denied state
                const ERASE_VALUE: u8 = 7;
                let mut pass = true;
                // remember: we call these keys, but they live in data slots, because they are public keys.
                for key in keys {
                    // first print the key
                    let access = key.get_access_spec();
                    crate::println!("Permissions (spec): {:?}", access);
                    let acl = slot_mgr.get_acl(&key).unwrap();
                    crate::println!("Permissions (actual): {:x?}", acl);
                    // attempt to clear the permissions, making the keys malleable
                    slot_mgr
                        .set_acl(
                            &mut rram,
                            &key,
                            &AccessSettings::Data(DataSlotAccess::new_with_raw_value(0)),
                        )
                        .ok(); // if we can't clear, that's by design

                    let acl = slot_mgr.get_acl(&key).unwrap();
                    crate::println!("Permissions (attacked): {:x?}", acl);
                    crate::println!("Data: {:x?}", slot_mgr.read(&key).ok());
                    let eraser = [ERASE_VALUE; SLOT_ELEMENT_LEN_BYTES];
                    match slot_mgr.write(&mut rram, &key, &eraser) {
                        Ok(_) => {}
                        Err(e) => {
                            crate::println!("Couldn't erase pubkey in data slot {}: {:?}", key.get_base(), e)
                        }
                    }
                    let check = slot_mgr.read(&key).unwrap();
                    if check.iter().all(|&b| b == ERASE_VALUE) {
                        crate::println!("Data at {} was mutable, failure!", key.get_base());
                        pass = false;
                    }
                }
                use bao1x_hal::board::{BOOKEND_END, BOOKEND_START};
                crate::println!(
                    "{}SEC.PUBMUT-{},{}",
                    BOOKEND_START,
                    if pass { "PASS" } else { "FAIL" },
                    BOOKEND_END
                );
            }
            #[cfg(feature = "unsafe-debug")]
            "peek" => {
                const COLUMNS: usize = 4;
                if args.len() == 1 || args.len() == 2 {
                    let addr = usize::from_str_radix(&args[0], 16)
                        .map_err(|_| Error::help("Peek address is in hex, no leading 0x"))?;

                    if addr >= utralib::HW_RERAM_MEM + bao1x_api::RRAM_STORAGE_LEN
                        && addr < utralib::HW_RERAM_MEM + utralib::HW_RERAM_MEM_LEN
                    {
                        return Err(Error::help("Peek disallowed for security-related sectors"));
                    }
                    let count = if args.len() == 2 {
                        if let Ok(count) = u32::from_str_radix(&args[1], 10) { count } else { 1 }
                    } else {
                        1
                    };
                    // safety: it's not safe to do this, the user peeks at their own risk
                    let peek = unsafe { core::slice::from_raw_parts(addr as *const u32, count as usize) };
                    for (i, &d) in peek.iter().enumerate() {
                        if (i % COLUMNS) == 0 {
                            crate::print!("\n\r{:08x}: ", addr + i * size_of::<u32>());
                        }
                        crate::print!("{:08x} ", d);
                    }
                    crate::println!("");
                } else {
                    return Err(Error::help("Help: peek <addr> [count], addr is in hex, count in decimal"));
                }
            }
            #[cfg(feature = "pq-check")]
            "pq" => {
                // 7.48ms unaccelerated (software only)
                // 7.00ms with message hash accelerated
                // 70ms to verify a 256k block (so slightly less than 7ms for the signature verification, 63ms
                // for the message hash)
                use core::convert::TryFrom;

                use signature::*;
                use slh_dsa::*;
                const MSG: &[u8] = b"NIST SP 800-230 SLH-DSA vector\n";

                // --- shared KAT vector for SLH-DSA-SHA2-128-24 ---
                const SHA2_128_24_PK: &str =
                    "808182838485868788898a8b8c8d8e8fa04ad33acb292af0da32f74d3285b014";
                const SHA2_128_24_SIG: &str = "eabbc67b08a221583636efddfe80483c448c6e2fc068ac375b192a982c5b70d70d1850e1e67e7121999a9c396eac3977328a1f5cbcd0e6c1eeabf925adf3b00909b7599b458621012f9b555d9053b3edcb7bf0a4a9962ba3dc1a65e8023b8d1b98d14701d9c1778b28e4fdc8c3a8f178b422e42f0170f79467f8256dcbc0ddbd96865bae84872c993401af59b5dbf6f141d6bb23d93667380e9b05342ea97d412967ad4f24b9b1f80fb7d0f62de142d4a242f264f92b6b26f5c4352caa0036c19a9819b9b82cf287ba7eb43c2dbe5bbb45cd9f68c1e511b9f55f2b8a7b56a999ada47f0ec04ee3d9d34f03a3a77fe0c4ed33a20fcffa8de28ad6aa0eee2fa84b943b82a5c5dc37a34f7f69ca16c9bca0592efe3c7912f22502c3dbe5e29ef8ca18417c49dcf2a1dc5bca70c79f48e227dcef605d3b09ada25032041d7934f16842408df3e11d84396780f9324e27d5998f34ecb060f42f626ad4bfdacdfc06f67dfe946ec3cfc52ad6e68e127422ee4a690ba04faa169d031a35e1917fb0bf571b50b1ee5891b8ad5e9215410fd390282bfa99caa7ba11e885a1569edadf7d0ffde1c229504ecc786b5e195ad7183854700cdfc9be46eeec60a3f5db7f50228e75fb64d5c04e7c3fc64ef5606801f4cbf2a8f3a17cad9803cf8795bee9426146503760bf6b6572bc13658b7c66a88e9157e11241cb4556daeaa63221448427bcfd85da7c6096fbb59bdce5ff8be3de3a8de45c8868b0850d2b9f57a064857b807246939aa91955fbd023db52d3c461f7cc9798ef066c472d0cc018eb593263dba47a39656a2d8707981cd2a5ae0dac65e9fa5d288af50fc325c098c47939c4eef6ad7f94392cff5fc2b65d930b7280c7e9b2ae593ebe2c236a7461e51b3a7d1342972534535ae7d9d378c29a80f7869bc6a9aeb10da68abdb5a77d65a8ceaccd3c5b3122ace895b37a2fb51c47589f238e44d0050b698104ca50eb8a9cbeb92d586846869101b2fcea231cea0133eb6754229e3f083fe6554e4bd08c710b0b84beeec24355f4a45a20c36df8efe1c73607101ff553f0802493fc038018b16d598802a6f4091c4d39fe94291e24a0b539c0e4cbe5e7b16a2cb084ba502314b6c87df0bb59662cc808a023475507a74ae5a6a258e4b90d4168b02747347fc7d3f26e83ff49707c87cba57256409a07ae17a652fa529c77adcf8002ac8eaafcd79414ecffaca9b47c864ce029d0cb5c1b4aa589a27b41bb81e256a5c10d7732d3ff8b49e350ebdba86c17b73a79d143ad8fa7329223467fafbb409417ffba779d36f16cd683af9cfff2329f3e2c59f5cf7694df91286dfce6ab22be1d04ad8042c8483f28266902e84e9347c616d46e2e1ec3723ecad23652de23127aa47d10cc11150408fd3d619cd336c6013e1b898b14bc3feb66b4dc6f7b6d3ea0b6f30101c3660be72780a843a2a3324bbcc5b033a60f5413dd272e0f48062cafbc6e82eb6d06d120d86886a862b70d71f6f64d6e0566160588f86c78bd2989ca737dee12cb010aac2999d5c6e611429adbb44193aa5db2ecca9c72990d5f64f0f15b83529499addc02a4c29d1e8ea726ecad2714cc52599a030cd97a244f95357d0659357393eb54af6821c72a49e268241f4a52c172c7352b4ae786ab29f7ee3e15f90fe71eda43af66f95c2fc2f9d253285042d104814281e769cef547aecb93e252c64ad23627dff0824bc24edf46adad43377fd7f4172bde3453451b946b2c6bf192fa7f17825a168d500bcf2876002900aac4d9b0399222d731849b3b3a6687e3966cc3f4f3121e9feec94f7dcf782d553271788840857f4f1989ad55b4d42ac70af7e67845550ed0c56621efa20ebbc66938fc0d4a6592e6ce676d51acdff77f1e6f566e92a3d1a3546cb31ef076441e784d86a5302b9aaaf0ee91ca320289d41fa97b1be3e8da1a932fe029cd2e8cc6351806c4b3dcf42bfc9852e0ca156f3591a5dbb4451f81ff8fc3e4a0ad85d52120c561d99b87851a46b71500e0b9f1ad048452fff195b32c600b4fe053a359b2ec086aae9d8245add1dfa3918b2c83334911eb4d8960c96ac7c4c3cbf62802939b666b2cc56c777829395259c382006bb03c842a7200f9c7c13758c08a235bd29c67a307481336c1e334a52946c06dc28299b900eec05248d61c817f56ae55217405fc1e564874a55e1f025894ec05fe010a52d2e1fd86186923377736b4a5293c767072c754d99eca6ddca8deaabae566cdf379d8ec7f0563703d451bcd2814353f1f13a57c986d45d58295f90a4da01896bf2eae872a90aa364c574a6973a28a97305fedcc08d1d308d3651e64a280dd99979b54f83e9656e1fc31c5b86d1eac3b93c5ac2fa574369d587d9ade0df7686dcc9235ad4f81ef96a7e7957eb7476c1b23b18bc39714a734dc64b0140441bd61d0a7f86e959e4508ee8901afca72656e093f636f97031a28b31442ac23aad56a27201985ed0a57fd5f014a83a8912e618f27c12cec92d9a8626038bfd8a69325e4c63560c3e9ba304da81e0fcf5eaec249d9c524beb8af2d03725b1d483cb721b38faa5c4594406ab2ac39d1cac618f5ef995c6f2dbceac781a8b5fd25e4f8cef72be82873686921670d48645f69e6e473bdf77d8d4ac3d3fef5a214c563e1cea309fd899801ce3ada57466237ac77cea3d1411aee2133ec17f1c8d7ee8cc5865e2b0e3535691f891c353e68be1676b216c47f5f38c0e96f81563a9e85745ed048fb38a3387a2b3a5b74ed5cf7813fa6696ede3f3d481e5468cc80898758e2deb0960c9c428f34e394651082a297a79bf106a6818df7b3b7fa39b2e937e0ec6457cae0bd8bed3ef2e7104cb2b6ad3f7490f9be245e2d7584a42f12e134f8dc5b09bc08e9e36c166ca9f450abdb9a65408aee137f11b08bd7863735f658eaad3d2048bbe1cbdb55c6b823ee6a664f6ee4e0efd19f8587809b1905e4a3a8e0c4624673194cc121d983d76853c1b8844c47761efcb615350f40729c6ecfa8da513fae4324f938765c9470370248d3be3c2cd6e469df894256be0b06b742cd177d70d5828d3e3fc6c52a1ac43955f061fdcad872f77efcde65fc7d781f03180d77e64f891f38ef156ec279ce13f0418793e94b4e664363acac0e30342910459ac7eb5c80fabc37ba0fabf23f8dd7273d8fd24128a03fe1cab64a8fe6093361176693ca913ff9bb692fbbf451e9ede9013bcf609bc05c0b3fe1b17faf01e5131be4878ecf5696c5bc983997143a59a65f96ed9c11889331d3eef0381bd7343de8f42888775512abebc660a8379b43ce3b99898fa0e2dc92c0ae2aca027c2a3195d53aa382c395403998f2cfb8fb9b24a85871682286e875215c9f5e4ff4dc6e2f52280bd0230d7d420023cd70bd5ca3df7dd51758a474cf230dcf171ba02f10cb69c53a2238ebad14f475f829f41f82e76ea06e1b4232314947f3201bc7254a029e5ee9204f3202666b32c6995cc8d30df0492eb37614d52f2f7f580349d1375d19f4913fbe1be2c00a749d0cfaa7d878d223a6ca7723e2ff1f8d42bad5e12cf97dd08f3e182a546f97ed5e1eb90703cb987de044c6a6c51560df50bfeda8a9d3e2945a74edd218d746573d30ce3195e582c4432169b6cedca859e2dc049fa8cfc9c456cd4b5c2394c8c14deb4443cf1304954a268dc31b2863332c626e7cf0cf74497f7fff2911644e44dc696eacf3d75fa68de2757b1674817a28ace00a4ac53815dfd95c23cd87043c7545daeca10cb7842402d1720b62455b85403c5e17d009570a4dfaaa8898e496878c1d8ad821c50ed0bc327359ed3b2570447f6e3dde4881d860920532ba63b502824b576dacdf31abf8b6cdd078147e61210def9fc16e406fccca3fffe97734da5f6a0a5661fbc1b238665b951cc8aac7b04e26eafac82317838dd480b6af761bb1302c46bce7b37b94a1b2e640db4923bd6415597d08e1108d4e388f5894f58ca5a880baa0d36a9c17247c71a129fd89665f69abfc084b7f04721f0ef32e7b560b5d0506697ea24d8be0d272f530ebabb0987fee4a71177d9a804a07472209ac3d53600cdb96c7ce2b1b865da02014c7c14c7ed3b6810b5b925ac42ba7cf14843900f9bcbf04d5c807d9861fd3a569390dc08ebfa47f4b3cbb024de9219f7a0f6b6bc804acab8accd4a123fd54c30b2e940065d6b442abd8b4d43eec0433bb51db44bec775ccdd2186fad8e041c35dbf4d47bf907380b36e785c5ff3b65cea3246c5557370301f05013ef02fcf6261aae6a4a691e8e61cc0efa7e9820bbd27f7a30c583a32d7dba13f955607c0d64a0661f8fa746754da7f0e1983e033b47044e3b81e900b1af3d0af413ffffd4f5e2a1dbc6f7e62f6dc67aa26bd4df44224305875fc561d473ba5d8bafd978a52ba8db257cb15458253bdf56c6942dcae8869ed3c923f774f43380433be45c39d1381da466da4feee5a68c229208cf548bbd35cf848f4565388e0b2795e60519d96d9e680d5ebe2f436bc6260c010ec730d41bd296eca2d4146856b9b99050cdd3fbeeccf82bdc12d52a75fff600085a331e346e2beecced292feaf278a58a1dc1082214601d699e75fda40213ecefe6865ec8fb69e7f13863b6fa2305d118830eb58a53b20e229e01b1e1e6a6f0f35b62824dbc63705f498da306fbbdaa51c459ddd59e7d0f8b5f26c02d8c56ceb6888d4385fa2923124af946c745185ae2bf32fb22191b6885ca0b0908268f52971ef97372ee49e1e82e0d0c78a8767b193b6345e8ee6ac30b1cd476ddeb8c35d1d17cce1f3ee2a9f7dc37f3762e14978aae040af53922d0d8f284c09af7c303f0a7258a591d58f0341894bf0308516bc1e02de85a7c89e0868f4a0b5f9c6ad5a7b57137251a8288a955771fd720521b90eb9a5469b87e6269d44e1aaf9446a3f81818febd45b29790684ee07d7dd047bed958f216229a4088ed291135e1c9ec3b4e342feb552cac8bc24ba55c1c18f6e0bbb76f84a94c7b09abd62b8d509300011c59c3425a173c81491cce0a03d01497a35ecc540092b62b37557eac0d35c6826c7c2c07e6568f621f31906e7df2372a1cb47204fc2c7c5aabea685d856897f5b2943309546bbf35f974b81a8cfd48934a88df6e835b031164d024c8784fa3e4061f7e9e6c52618c4d44d38774a17de1721d996c2390c6734c8167188edc8167c0887ac17a0579cd00c1c92bfab3775a243723a88572858424bb42f334070ed33ef7db365a8fc91649d615dd07661cc9d8350ab612f7693077480a25029ce7cba2fd0777d141a1670a3d59be2b05515d99b34411ee0ac4832329d352ab391bf4a770956a7df10a94888b17d384324795b0b5e12d47e935bccf0a0a707b58ae560c726ce7ff3841500b3f053d17ff6a6bcf859c27a7ee039f7c920c47bcea78393b073f5799a1bbb07dd31";

                /// Verify a known-good (pk, signature) pair from the vector.
                fn run_kat_verify<P: ParameterSet>(pk_hex: &str, sig_hex: &str) {
                    use bao1x_hal::iox::Iox;
                    let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
                    iox.set_gpio_dir(IoxPort::PC, 6, IoxDir::Output);
                    iox.set_gpio_pin(IoxPort::PC, 6, IoxValue::Low);

                    let mut pk_bytes = [0u8; 32];
                    let mut sig_bytes = [0u8; 3856];
                    let _ = hex::decode_to_slice(pk_hex, &mut pk_bytes).unwrap();
                    let _ = hex::decode_to_slice(sig_hex, &mut sig_bytes).unwrap();
                    crate::println!("pk: {:x?}", &pk_bytes);
                    crate::println!("sig: {:x?}", &sig_bytes[..16]);
                    crate::println!("msg: {}", unsafe { core::str::from_utf8_unchecked(MSG) });

                    let vk = VerifyingKey::<P>::try_from(pk_bytes.as_slice()).expect("valid public key");
                    let sig = Signature::<P>::try_from(sig_bytes.as_slice()).expect("valid signature");

                    //let fw =
                    //     unsafe { core::slice::from_raw_parts(BOOT0_TO_BOOT1.image_ptr as *const u8, 262144)
                    // };

                    // internal (no-context) path — matches the C `crypto_sign_verify_internal`
                    crate::println!("starting verification");
                    iox.set_gpio_pin(IoxPort::PC, 6, IoxValue::High);
                    let result = vk.slh_verify_internal(&[MSG], &sig);
                    crate::println!("result: {:?}", result);
                    iox.set_gpio_pin(IoxPort::PC, 6, IoxValue::Low);
                    crate::println!("finished verification");

                    // negative control: a tampered message must NOT verify
                    let mut bad = [0u8; 31];
                    bad.copy_from_slice(MSG);
                    bad[0] ^= 0x01;
                    crate::println!("Negative control");
                    assert!(
                        vk.slh_verify_internal(&[&bad], &sig).is_err(),
                        "tampered message unexpectedly verified"
                    );
                    crate::println!("Negative control passed");
                }
                run_kat_verify::<Sha2_128_24>(SHA2_128_24_PK, SHA2_128_24_SIG);
            }
            "ate" => {
                use bao1x_api::{IoGpio, IoSetup};
                use bao1x_hal::iox::Iox;
                let iox = Iox::new(utralib::utra::iox::HW_IOX_BASE as *mut u32);
                // setup PF1 as an "index" pin
                iox.setup_pin(
                    bao1x_api::IoxPort::PF,
                    5,
                    Some(bao1x_api::IoxDir::Output),
                    Some(bao1x_api::IoxFunction::Gpio),
                    None,
                    Some(bao1x_api::IoxEnable::Disable),
                    None,
                    None,
                );
                // indicates test running
                iox.set_gpio_pin_value(bao1x_api::IoxPort::PF, 5, bao1x_api::IoxValue::Low);
                // force USB speed to full speed
                crate::glue::setup(Some(bao1x_hal::usb::driver::PortSpeed::Fs));

                let slot_mgr = bao1x_hal::acram::SlotManager::new();
                let mut rram = bao1x_hal::rram::Reram::new();
                let slot = &bao1x_api::offsets::ATE_RESERVED;
                let ate = crate::platform::ate::Ate::new(self.perclk);
                let mut data = [0u8; 32];
                ate.serialize_into(&mut data);
                slot_mgr.write(&mut rram, slot, &data).ok();

                // indicates test finish
                iox.set_gpio_pin_value(bao1x_api::IoxPort::PF, 5, bao1x_api::IoxValue::High);
            }
            "atecheck" => {
                let slot_mgr = bao1x_hal::acram::SlotManager::new();
                let slot = &bao1x_api::offsets::ATE_RESERVED;
                match slot_mgr.read(slot) {
                    Ok(d) => crate::println!("{:02x?}", d),
                    Err(e) => crate::println!("{:?}", e),
                }
            }
            "usb_speed" => {
                let one_way = OneWayCounter::new();
                if args.len() == 0 {
                    crate::println!(
                        "USB speed: {:?}",
                        one_way.get_decoded::<bao1x_api::UsbDefaultSpeed>().expect("owc coding error")
                    );
                    self.abort_cmd();
                    return Ok(());
                } else if args.len() != 1 {
                    return Err(Error::help("usb_speed [full | high]"));
                }
                let new_type = match args[0].as_str() {
                    "full" => bao1x_api::UsbDefaultSpeed::Full,
                    "high" => bao1x_api::UsbDefaultSpeed::High,
                    _ => return Err(Error::help("usb_speed [full | high]")),
                };
                let mut count = 0;
                while one_way.get_decoded::<bao1x_api::UsbDefaultSpeed>().expect("owc coding error")
                    != new_type
                {
                    one_way.inc_coded::<bao1x_api::UsbDefaultSpeed>().expect("increment error");
                    count += 1;
                }
                crate::println!(
                    "USB default speed is {:?} after {} increments. Takes effect after a reboot.",
                    new_type,
                    count
                );
            }
            "echo" => {
                for word in args {
                    crate::print!("{} ", word);
                }
                crate::println!("");
            }
            _ => {
                crate::println!("Command not recognized: {}", cmd);
                crate::print!(
                    "Commands include: altboot, audit, boot, boardtype, bootwait, echo, idmode, ifr, localecho, lockdown, paranoid, require-pq, reset, self_destruct, skipping, uf2, usb_speed"
                );
                #[cfg(feature = "test-boot0-keys")]
                crate::print!(", publock");
                crate::println!("");
            }
        }

        // reset for next loop
        self.abort_cmd();
        Ok(())
    }

    pub fn abort_cmd(&mut self) {
        self.do_cmd = false;
        self.cmdline.clear();
    }
}
