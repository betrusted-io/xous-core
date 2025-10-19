#[allow(unused_imports)]
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::convert::TryInto;

use bao1x_api::signatures::FunctionCode;
#[allow(unused_imports)]
use bao1x_api::*;
use bao1x_hal::acram::OneWayCounter;
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
                crate::secboot::boot_or_die();
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
                if args.len() != 1 {
                    return Err(Error::help("bootwait [check | toggle]"));
                }
                if args[0] == "toggle" {
                    // this toggles the bootwait flag by incrementing its one-way counter
                    let one_way = OneWayCounter::new();
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
                    let one_way = OneWayCounter::new();
                    let state =
                        one_way.get_decoded::<bao1x_api::BootWaitCoding>().expect("couldn't fetch flag");
                    crate::println!("bootwait is {:?}", state);
                } else {
                    return Err(Error::help("bootwait [check | toggle]"));
                }
            }
            "boardtype" => {
                let one_way = OneWayCounter::new();
                if args.len() != 1 {
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
            "audit" => {
                let owc = OneWayCounter::new();
                crate::println!(
                    "Board type reads as: {:?}",
                    owc.get_decoded::<bao1x_api::offsets::BoardTypeCoding>()
                );
                crate::println!(
                    "Boot partition is: {:?}",
                    owc.get_decoded::<bao1x_api::offsets::AltBootCoding>()
                );
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
                crate::println!("Revocations:");
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

                match bao1x_hal::sigcheck::validate_image(
                    bao1x_api::BOOT0_START as *const u32,
                    bao1x_api::BOOT0_START as *const u32,
                    bao1x_api::BOOT0_REVOCATION_OFFSET,
                    &[FunctionCode::Boot0 as u32],
                    false,
                    None,
                ) {
                    Ok((k, tag)) => crate::println!(
                        "Boot0: key {} ({})",
                        k,
                        core::str::from_utf8(&tag).unwrap_or("invalid tag")
                    ),
                    Err(e) => crate::println!("Boot0 did not validate: {:?}", e),
                }
                match bao1x_hal::sigcheck::validate_image(
                    bao1x_api::BOOT1_START as *const u32,
                    bao1x_api::BOOT0_START as *const u32,
                    bao1x_api::BOOT0_REVOCATION_OFFSET,
                    &[
                        FunctionCode::Boot1 as u32,
                        FunctionCode::UpdatedBoot1 as u32,
                        FunctionCode::Developer as u32,
                    ],
                    false,
                    None,
                ) {
                    Ok((k, tag)) => crate::println!(
                        "Boot1: key {} ({})",
                        k,
                        core::str::from_utf8(&tag).unwrap_or("invalid tag")
                    ),
                    Err(e) => crate::println!("Boot1 did not validate: {:?}", e),
                }
                match bao1x_hal::sigcheck::validate_image(
                    bao1x_api::LOADER_START as *const u32,
                    bao1x_api::BOOT1_START as *const u32,
                    bao1x_api::BOOT1_REVOCATION_OFFSET,
                    &crate::secboot::ALLOWED_FUNCTIONS,
                    false,
                    None,
                ) {
                    Ok((k, tag)) => crate::println!(
                        "Next stage: key {} ({})",
                        k,
                        core::str::from_utf8(&tag).unwrap_or("invalid tag")
                    ),
                    Err(e) => crate::println!("Next stage did not validate: {:?}", e),
                }
                // TODO:
                //   - devboot seen
                //   - oem keys erased
                //   - uuid
            }
            "lockdown" => {
                match bao1x_hal::sigcheck::validate_image(
                    bao1x_api::BOOT1_START as *const u32,
                    bao1x_api::BOOT0_START as *const u32,
                    bao1x_api::BOOT0_REVOCATION_OFFSET,
                    &[
                        FunctionCode::Boot1 as u32,
                        FunctionCode::UpdatedBoot1 as u32,
                        FunctionCode::Developer as u32,
                    ],
                    false,
                    None,
                ) {
                    Ok((k, _tag)) => {
                        if k != bao1x_api::pubkeys::DEVELOPER_KEY_SLOT {
                            crate::println!(
                                "This will permanently disable developer mode. It cannot be undone!"
                            );
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
                }
            }
            #[cfg(feature = "unsafe-debug")]
            "peek" => {
                const COLUMNS: usize = 4;
                if args.len() == 1 || args.len() == 2 {
                    let addr = usize::from_str_radix(&args[0], 16)
                        .map_err(|_| Error::help("Peek address is in hex"))?;

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
                    "Commands include: reset, echo, boot, bootwait, localecho, uf2, boardtype, audit, lockdown"
                );
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
