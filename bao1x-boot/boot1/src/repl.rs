#[allow(unused_imports)]
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::convert::TryInto;

use bao1x_api::pubkeys::{BOOT0_SELF_CHECK, BOOT0_TO_BOOT1, BOOT1_TO_LOADER_OR_BAREMETAL};
#[allow(unused_imports)]
use bao1x_api::*;
use bao1x_hal::acram::OneWayCounter;
use bao1x_hal::hardening::Csprng;
use utralib::*;

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
}

impl Repl {
    pub fn new() -> Self {
        Self { cmdline: String::new(), do_cmd: false, local_echo: true, lockdown_armed: false }
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
                    ("loader", LOADER_REVOCATION_DUPE_OFFSET + bao1x_api::pubkeys::DEVELOPER_KEY_SLOT),
                    ("boot0", BOOT0_REVOCATION_DUPE_OFFSET + bao1x_api::pubkeys::DEVELOPER_KEY_SLOT),
                    ("boot1", BOOT1_REVOCATION_DUPE_OFFSET + bao1x_api::pubkeys::DEVELOPER_KEY_SLOT),
                    ("paranoid1", PARANOID_MODE), /* let's try CI testing with this active, and see how
                                                   * bad it is... */
                    ("paranoid2", PARANOID_MODE_DUPE),
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
            "uf2" => {
                use base64::{Engine as _, engine::general_purpose};
                if args.len() != 1 {
                    crate::println_d!("u2f query malformed");
                    return Err(Error::help("uf2 [base64 data]"));
                }
                match general_purpose::STANDARD.decode(&args[0]) {
                    Ok(uf2_data) => {
                        if let Some(record) = crate::uf2::Uf2Block::from_bytes(&uf2_data) {
                            if record.address() as usize >= bao1x_api::BAREMETAL_START
                                && (record.address() as usize)
                                    < utralib::HW_RERAM_MEM + bao1x_api::RRAM_STORAGE_LEN
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
                crate::platform::slots::check_slots(&new_type);
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
                let owc = OneWayCounter::new();
                let boardtype = owc.get_decoded::<BoardTypeCoding>().unwrap();
                crate::println!("Board type reads as: {:?}", boardtype);
                crate::println!("Boot partition is: {:?}", owc.get_decoded::<AltBootCoding>());
                crate::println!("Semver is: {}", crate::version::SEMVER);
                crate::println!("Description is: {}", crate::RELEASE_DESCRIPTION);
                let slot_mgr = bao1x_hal::acram::SlotManager::new();
                let sn = slot_mgr.read(&bao1x_hal::board::SERIAL_NUMBER).unwrap();
                crate::println!(
                    "Device serializer: {:08x}-{:08x}-{:08x}-{:08x}",
                    u32::from_le_bytes(sn[12..16].try_into().unwrap()),
                    u32::from_le_bytes(sn[8..12].try_into().unwrap()),
                    u32::from_le_bytes(sn[4..8].try_into().unwrap()),
                    u32::from_le_bytes(sn[..4].try_into().unwrap())
                );
                crate::println!(
                    "Public serial number: {}",
                    bao1x_hal::usb::derive_usb_serial_number(&owc, &slot_mgr)
                );
                let uuid = slot_mgr.read(&bao1x_hal::board::UUID).unwrap();
                crate::println!(
                    "UUID: {:08x}-{:08x}-{:08x}-{:08x}",
                    u32::from_le_bytes(uuid[12..16].try_into().unwrap()),
                    u32::from_le_bytes(uuid[8..12].try_into().unwrap()),
                    u32::from_le_bytes(uuid[4..8].try_into().unwrap()),
                    u32::from_le_bytes(uuid[..4].try_into().unwrap())
                );
                crate::println!(
                    "Paranoid mode: {}/{}",
                    owc.get(PARANOID_MODE).unwrap(),
                    owc.get(PARANOID_MODE_DUPE).unwrap()
                );
                // this number may be non-zero because some of the sensors are on a hair-trigger
                crate::println!("Possible attack attempts: {}", owc.get(POSSIBLE_ATTACKS).unwrap());
                crate::println!("Revocations:");
                // only checks the main array, not the duplicate array
                crate::println!("Stage       key0     key1     key2     key3");
                let key_array = [
                    ("boot0       ", BOOT0_REVOCATION_OFFSET),
                    ("boot1       ", BOOT1_REVOCATION_OFFSET),
                    ("next stage  ", LOADER_REVOCATION_OFFSET),
                ];
                for (img_type, start) in key_array {
                    crate::print!("{}", img_type);
                    for offset in start..(start + PUBKEY_SLOTS) {
                        if owc.get(offset).unwrap_or(1) == 0 {
                            crate::print!("enabled  ");
                        } else {
                            crate::print!("revoked  ");
                        }
                    }
                    crate::println!("");
                }

                match bao1x_hal::sigcheck::validate_image(BOOT0_SELF_CHECK, None, None) {
                    Ok((k, k2, tag, target)) => crate::println!(
                        "Boot0: key {}/{} ({}) -> {:x}",
                        k,
                        !k2,
                        core::str::from_utf8(&tag).unwrap_or("invalid tag"),
                        target ^ u32::from_le_bytes(tag)
                    ),
                    Err(e) => crate::println!("Boot0 did not validate: {:?}", e),
                }
                match bao1x_hal::sigcheck::validate_image(BOOT0_TO_BOOT1, None, None) {
                    Ok((k, k2, tag, target)) => crate::println!(
                        "Boot1: key {}/{} ({}) -> {:x}",
                        k,
                        !k2,
                        core::str::from_utf8(&tag).unwrap_or("invalid tag"),
                        target ^ u32::from_le_bytes(tag)
                    ),
                    Err(e) => crate::println!("Boot1 did not validate: {:?}", e),
                }
                match bao1x_hal::sigcheck::validate_image(BOOT1_TO_LOADER_OR_BAREMETAL, None, None) {
                    Ok((k, k2, tag, target)) => crate::println!(
                        "Next stage: key {}/{} ({}) -> {:x}",
                        k,
                        !k2,
                        core::str::from_utf8(&tag).unwrap_or("invalid tag"),
                        target ^ u32::from_le_bytes(tag)
                    ),
                    Err(e) => crate::println!("Next stage did not validate: {:?}", e),
                }

                // detailed state checks
                let mut secure = true;
                // check that boot1 pubkeys match the indelible entries
                let pubkey_ptr = bao1x_api::BOOT1_START as *const bao1x_api::signatures::SignatureInFlash;
                let pk_src: &bao1x_api::signatures::SignatureInFlash =
                    unsafe { pubkey_ptr.as_ref().unwrap() };
                let reference_keys = [
                    bao1x_api::BAO1_PUBKEY,
                    bao1x_api::BAO2_PUBKEY,
                    bao1x_api::BETA_PUBKEY,
                    bao1x_api::DEV_PUBKEY,
                ];
                let slot_mgr = bao1x_hal::acram::SlotManager::new();
                let mut good_compare = true;
                for (boot0_key, ref_key) in pk_src.sealed_data.pubkeys.iter().zip(reference_keys.iter()) {
                    let ref_data = slot_mgr.read(&ref_key).unwrap();
                    if ref_data != &boot0_key.pk {
                        good_compare = false;
                    }
                }
                if !good_compare {
                    crate::println!("== BOOT1 FAILED PUBKEY CHECK ==");
                    // this may "not" be a security failure if boot1 was intentionally replaced
                    // but in that case, the developer customizing the image should have also edited this
                    // check out.
                    secure = false;
                }
                // check developer mode
                if owc.get(DEVELOPER_MODE).unwrap() != 0 {
                    crate::println!("== IN DEVELOPER MODE ==");
                    secure = false;
                }
                if owc.get(BOOT0_PUBKEY_FAIL).unwrap() != 0 {
                    crate::println!("== BOOT0 REPORTED PUBKEY CHECK FAILURE ==");
                    secure = false;
                }
                if owc.get(BOOT1_PUBKEY_FAIL).unwrap() != 0 {
                    crate::println!("== BOOT1 REPORTED PUBKEY CHECK FAILURE ==");
                    secure = false;
                }
                if owc.get(CP_BOOT_SETUP_DONE).unwrap() == 0 {
                    crate::println!("== CP SETUP FAILED ==");
                    secure = false;
                }
                if owc.get(IN_SYSTEM_BOOT_SETUP_DONE).unwrap() == 0 {
                    crate::println!("In-system keys have NOT been generated");
                    secure = false;
                } else {
                    crate::println!("In-system keys have been generated");
                }
                if !secure {
                    crate::println!("** System did not meet minimum requirements for security **");
                }
            }
            "lockdown" => match bao1x_hal::sigcheck::validate_image(BOOT0_TO_BOOT1, None, None) {
                Ok((k, _k2, _tag, _target)) => {
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
            "baosec-init" => {
                if !matches!(args.as_slice(), [s] if s == "confirm") {
                    return Err(Error::help(
                        "Usage: 'baosec-init confirm'. WARNING: erases external storage!",
                    ));
                }

                // this routine is used to initialize baosec products - sets the board type and
                // erases the off-chip FLASH
                use bao1x_api::{baosec::PDDB_LEN, baosec::PDDB_ORIGIN};
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

                crate::println!("Erasing from {:x}-{:x}...", 0, PDDB_ORIGIN + PDDB_LEN);
                for addr in (0..PDDB_ORIGIN + PDDB_LEN).step_by(SPINOR_BULK_ERASE_SIZE as usize) {
                    crate::println!("  {:x}...", addr);
                    flash_spim.flash_erase_block(addr, SPINOR_BULK_ERASE_SIZE as usize);
                }
                crate::println!("...done!");
                let one_way = bao1x_hal::acram::OneWayCounter::new();
                let board_type =
                    one_way.get_decoded::<bao1x_api::BoardTypeCoding>().expect("Board type coding error");
                if board_type != bao1x_api::BoardTypeCoding::Baosec {
                    while one_way.get_decoded::<bao1x_api::BoardTypeCoding>().expect("owc coding error")
                        != bao1x_api::BoardTypeCoding::Baosec
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
                crate::println!("{}BOOT1.SETBOARD,{}", BOOKEND_START, BOOKEND_END);
                crate::println!("Board type set to baosec, rebooting so boot1 can provision keys!");
                let mut rcurst = CSR::new(utra::sysctrl::HW_SYSCTRL_BASE as *mut u32);
                rcurst.wo(utra::sysctrl::SFR_RCURST0, 0x55AA);
            }
            "ifr" => {
                // safety: the IFR region is aligned and exists here. It is sealed by hardware in USER mode,
                // and should report as all 0's.
                let ifr = unsafe { core::slice::from_raw_parts(0x6040_0000 as *const u8, 0x400) };
                for (i, chunk) in ifr.chunks(32).enumerate() {
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
            "echo" => {
                for word in args {
                    crate::print!("{} ", word);
                }
                crate::println!("");
            }
            _ => {
                crate::println!("Command not recognized: {}", cmd);
                crate::print!(
                    "Commands include: reset, echo, altboot, boot, bootwait, idmode, localecho, uf2, boardtype, audit, lockdown, paranoid, self_destruct, ifr"
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
