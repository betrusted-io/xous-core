use crate::{ShellCmdApi, CommonEnv};
use xous_ipc::{String, Buffer};

use core::sync::atomic::{AtomicU32, Ordering};
static CB_ID: AtomicU32 = AtomicU32::new(0);
use num_traits::*;

use sha2::*;
use digest::Digest;

use core::fmt::Write;

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum UpdateOp {
    UpdateGateware,
    UpdateFirmware,
    UpdateWf200,
    UpdateAll,
    Quit,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum UpdateResult {
    PackageValid,
    ProgramDone,
    AutoDone,
    Abort,
}
#[derive(PartialEq, Eq)]
enum PackageType {
    Ec,
    Wf200
}

fn validate_package(pkg: &[u8], pkg_type: PackageType) -> bool {
    let mut hasher = sha2::Sha512Trunc256::new_with_strategy(FallbackStrategy::HardwareThenSoftware);
    let mut temp: [u8; 4] = Default::default();
    temp.copy_from_slice(&pkg[0x20..0x24]);
    if pkg_type == PackageType::Ec {
        if temp != [0x70, 0x72, 0x65, 0x63] { // 'prec'
            log::error!("EC firmware update package does not have the correct signature");
            return false;
        }
    } else {
        if temp != [0x77, 0x66, 0x32, 0x30] { // 'wf20'
            log::error!("WF200 update package does not have the correct signature");
            return false;
        }
    }
    temp.copy_from_slice(&pkg[0x24..0x28]);
    if u32::from_le_bytes(temp) != 1 {
        log::error!("update package version mismatch");
        return false;
    }
    temp.copy_from_slice(&pkg[0x28..0x2c]);
    let length = u32::from_le_bytes(temp);
    hasher.update(&pkg[0x20..(length as usize + 4096)]);
    let digest = hasher.finalize();
    log::debug!("digest: {:x?}", digest);
    for(&stored, &computed) in pkg[..0x20].iter().zip(digest.iter()) {
        if stored != computed {
            log::error!("update package hash mismatch");
            return false;
        }
    }
    true
}

const EC_GATEWARE_BASE: u32 = 0x0;
const EC_GATEWARE_LEN: u32 = 0x1_a000;
const EC_FIRMWARE_BASE: u32 = 0x1_a000;
const WF200_FIRMWARE_BASE: u32 = 0x9_C000;
const CTRL_PAGE_LEN: u32 = 0x1000;

/// copies an image stored in a `package` slice, starting from `pkg_offset` in the `package` with length `len`
/// and writing to FLASH starting at a hardware offset of `flash_start`
fn do_update(com: &mut com::Com, callback_conn: xous::CID, package: &[u8], pkg_offset: u32, flash_start: u32, image_len: u32, name: &str) -> bool {
    if (pkg_offset + image_len) > package.len() as u32 {
        log::error!("Requested image is larger than the package length");
        return false;
    }
    // erase
    let mut update_str = String::<1024>::new();
    write!(update_str, "Erasing {}...", name).unwrap();
    Buffer::into_buf(update_str).unwrap().lend(callback_conn, CB_ID.load(Ordering::Relaxed)).unwrap();
    update_str.clear();
    log::debug!("erasing from 0x{:08x}, 0x{:x} bytes", flash_start, image_len);
    if com.flash_erase(flash_start, image_len).unwrap() {
        write!(update_str, "Done.").unwrap();
    } else {
        write!(update_str, "Erase failed, aborting!").unwrap();
        return false;
    }
    Buffer::into_buf(update_str).unwrap().lend(callback_conn, CB_ID.load(Ordering::Relaxed)).unwrap();
    update_str.clear();

    // program
    write!(update_str, "Writing {}...", name).unwrap();
    Buffer::into_buf(update_str).unwrap().lend(callback_conn, CB_ID.load(Ordering::Relaxed)).unwrap();
    update_str.clear();

    // divide into 1k chunks and send over
    let exact_chunks = package[pkg_offset as usize..(pkg_offset + image_len) as usize].chunks_exact(1024);
    let lessthan_1k = exact_chunks.remainder();
    let mut prog_addr = flash_start;
    let mut pages: [Option<[u8; 256]>; 4] = [None; 4];
    let mut progress_ctr = 0;
    for chunks in exact_chunks {
        for (full_page, dst_page) in chunks.chunks_exact(256).zip(pages.iter_mut()) {
            *dst_page = Some(
                {
                    let mut alloc_page:[u8; 256] = [0; 256];
                    for (&src, dst) in full_page.iter().zip(alloc_page.iter_mut()) {
                        *dst = src;
                    }
                    alloc_page
                }
            );
        }
        log::debug!("prog 0x{:08x} 4 pages", prog_addr);
        if com.flash_program(prog_addr, pages).unwrap() == false {
            write!(update_str, "Program failed, aborting!").unwrap();
            Buffer::into_buf(update_str).unwrap().lend(callback_conn, CB_ID.load(Ordering::Relaxed)).unwrap();
            update_str.clear();
            return false;
        }
        prog_addr += 1024;
        progress_ctr += 1;
        if (progress_ctr % 12) == 0 {
            write!(update_str, "{:.0}% complete", ((prog_addr - flash_start) as f64 / image_len as f64) * 100.0).unwrap();
            Buffer::into_buf(update_str).unwrap().lend(callback_conn, CB_ID.load(Ordering::Relaxed)).unwrap();
            update_str.clear();
        }
    }
    // take the remainder that's less than 1k, and divide into 256-byte pages
    if lessthan_1k.len() > 0 {
        let fullpages = lessthan_1k.chunks_exact(256);
        let leftovers = fullpages.remainder();
        let mut pages_written = 0;
        pages = [None; 4]; // clear the pages buffer
        for (full_page, dst_page) in fullpages.zip(pages.iter_mut()) {
            *dst_page = Some(
                {
                    let mut alloc_page:[u8; 256] = [0; 256];
                    for (&src, dst) in full_page.iter().zip(alloc_page.iter_mut()) {
                        *dst = src;
                    }
                    alloc_page
                }
            );
            pages_written += 1;
        }
        // take the remainder that's less than 256-bytes, pad it to 256 bytes, and stick it in the very last page
        if leftovers.len() > 0 {
            pages[pages_written] = Some({
                let mut alloc_page: [u8; 256] = [0; 256];
                for(&src, dst) in leftovers.iter().zip(alloc_page.iter_mut()) {
                    *dst = src;
                }
                alloc_page
            });
        }
        let mut dbgcnt = 0;
        for pg in pages.iter() {
            if pg.is_some() {
                dbgcnt += 1;
            }
        }
        log::debug!("prog 0x{:08x} {} pages (last op)", prog_addr, dbgcnt);
        if com.flash_program(prog_addr, pages).unwrap() == false {
            write!(update_str, "Program failed, aborting!").unwrap();
            Buffer::into_buf(update_str).unwrap().lend(callback_conn, CB_ID.load(Ordering::Relaxed)).unwrap();
            update_str.clear();
            return false
        }
    }
    true
}

fn ecupdate_thread(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    let xns = xous_names::XousNames::new().unwrap();
    let mut com = com::Com::new(&xns).unwrap();
    let ticktimer = ticktimer_server::Ticktimer::new().unwrap();
    let llio = llio::Llio::new(&xns);
    let gam = gam::Gam::new(&xns).unwrap();
    let callback_conn = xns.request_connection_blocking(crate::SERVER_NAME_SHELLCHAT).unwrap();
    let mut susres = susres::Susres::new_without_hook(&xns).unwrap();

    if com.flash_acquire().unwrap() == false {
        log::error!("couldn't acquire exclusive access to the EC updater mechanism. All other operations will fail!");
    }

    #[cfg(any(target_os = "none", target_os = "xous"))]
    let ec_package = xous::syscall::map_memory(
        xous::MemoryAddress::new((xous::EC_FW_PKG_LOC + xous::FLASH_PHYS_BASE) as usize),
        None,
        xous::EC_FW_PKG_LEN as usize,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    ).expect("couldn't map EC firmware package memory range");
    #[cfg(any(target_os = "none", target_os = "xous"))]
    let wf_package = xous::syscall::map_memory(
        xous::MemoryAddress::new((xous::EC_WF200_PKG_LOC + xous::FLASH_PHYS_BASE) as usize),
        None,
        xous::EC_WF200_PKG_LEN as usize,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    ).expect("couldn't map EC wf200 package memory range");
    #[cfg(not(any(target_os = "none", target_os = "xous")))]
    let ec_package = xous::syscall::map_memory(
        None,
        None,
        xous::EC_FW_PKG_LEN as usize,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    ).expect("couldn't map EC firmware package memory range");
    #[cfg(not(any(target_os = "none", target_os = "xous")))]
    let wf_package = xous::syscall::map_memory(
        None,
        None,
        xous::EC_WF200_PKG_LEN as usize,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    ).expect("couldn't map EC wf200 package memory range");

    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("EC updater got msg {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(UpdateOp::UpdateGateware) => {
                let package = unsafe{ core::slice::from_raw_parts(ec_package.as_ptr() as *const u8, xous::EC_FW_PKG_LEN as usize)};

                if !validate_package(package,PackageType::Ec) {
                    log::error!("firmware package did not pass validation");
                    xous::send_message(callback_conn,
                        xous::Message::new_scalar(CB_ID.load(Ordering::Relaxed) as usize,
                        UpdateResult::Abort.to_usize().unwrap(),
                        0, 0, 0)).unwrap();
                } else {
                    susres.set_suspendable(false).unwrap(); // block suspend/resume operations
                    do_update(&mut com, callback_conn, package, CTRL_PAGE_LEN, EC_GATEWARE_BASE, EC_GATEWARE_LEN, "gateware");
                    susres.set_suspendable(true).unwrap(); // resume suspend/resume operations

                    xous::send_message(callback_conn,
                        xous::Message::new_scalar(CB_ID.load(Ordering::Relaxed) as usize,
                        UpdateResult::ProgramDone.to_usize().unwrap(),
                        EC_GATEWARE_LEN as usize, 0, 0)).unwrap();
                }
            },
            Some(UpdateOp::UpdateFirmware) => {
                let package = unsafe{ core::slice::from_raw_parts(ec_package.as_ptr() as *const u8, xous::EC_FW_PKG_LEN as usize)};
                let mut temp: [u8; 4] = Default::default();
                temp.copy_from_slice(&package[0x28..0x2c]);
                let length = u32::from_le_bytes(temp); // total length of package

                if !validate_package(package,PackageType::Ec) {
                    log::error!("firmware package did not pass validation");
                    xous::send_message(callback_conn,
                        xous::Message::new_scalar(CB_ID.load(Ordering::Relaxed) as usize,
                        UpdateResult::Abort.to_usize().unwrap(),
                        0, 0, 0)).unwrap();
                } else {
                    susres.set_suspendable(false).unwrap(); // block suspend/resume operations
                    do_update(&mut com, callback_conn, package, EC_GATEWARE_LEN + CTRL_PAGE_LEN,
                        EC_FIRMWARE_BASE, length - (EC_GATEWARE_LEN), "firmware");
                    susres.set_suspendable(true).unwrap(); // resume suspend/resume operations

                    xous::send_message(callback_conn,
                        xous::Message::new_scalar(CB_ID.load(Ordering::Relaxed) as usize,
                        UpdateResult::ProgramDone.to_usize().unwrap(),
                        (length - (EC_GATEWARE_LEN)) as usize, 0, 0)).unwrap();
                }
            },
            Some(UpdateOp::UpdateWf200) => {
                let package = unsafe{ core::slice::from_raw_parts(wf_package.as_ptr() as *const u8, xous::EC_WF200_PKG_LEN as usize)};
                let mut temp: [u8; 4] = Default::default();
                temp.copy_from_slice(&package[0x28..0x2c]);
                let length = u32::from_le_bytes(temp); // total length of package

                if !validate_package(package,PackageType::Wf200) {
                    log::error!("WF200 firmware package did not pass validation");
                    xous::send_message(callback_conn,
                        xous::Message::new_scalar(CB_ID.load(Ordering::Relaxed) as usize,
                        UpdateResult::Abort.to_usize().unwrap(),
                        0, 0, 0)).unwrap();
                } else {
                    susres.set_suspendable(false).unwrap(); // block suspend/resume operations
                    do_update(&mut com, callback_conn, package, CTRL_PAGE_LEN,
                        WF200_FIRMWARE_BASE, length, "wf200");
                    susres.set_suspendable(true).unwrap(); // resume suspend/resume operations

                    xous::send_message(callback_conn,
                        xous::Message::new_scalar(CB_ID.load(Ordering::Relaxed) as usize,
                        UpdateResult::ProgramDone.to_usize().unwrap(),
                        length as usize, 0, 0)).unwrap();
                }
            },
            Some(UpdateOp::UpdateAll) => {
                // gateware
                let package = unsafe{ core::slice::from_raw_parts(ec_package.as_ptr() as *const u8, xous::EC_FW_PKG_LEN as usize)};

                if !validate_package(package,PackageType::Ec) {
                    log::error!("firmware package did not pass validation");
                    xous::send_message(callback_conn,
                        xous::Message::new_scalar(CB_ID.load(Ordering::Relaxed) as usize,
                        UpdateResult::Abort.to_usize().unwrap(),
                        0, 0, 0)).unwrap();
                    continue;
                } else {
                    susres.set_suspendable(false).unwrap(); // block suspend/resume operations
                    do_update(&mut com, callback_conn, package, CTRL_PAGE_LEN, EC_GATEWARE_BASE, EC_GATEWARE_LEN, "gateware");
                    susres.set_suspendable(true).unwrap(); // resume suspend/resume operations

                    xous::send_message(callback_conn,
                        xous::Message::new_scalar(CB_ID.load(Ordering::Relaxed) as usize,
                        UpdateResult::ProgramDone.to_usize().unwrap(),
                        EC_GATEWARE_LEN as usize, 0, 0)).unwrap();
                }

                // firmware
                let package = unsafe{ core::slice::from_raw_parts(ec_package.as_ptr() as *const u8, xous::EC_FW_PKG_LEN as usize)};
                let mut temp: [u8; 4] = Default::default();
                temp.copy_from_slice(&package[0x28..0x2c]);
                let length = u32::from_le_bytes(temp); // total length of package

                if !validate_package(package,PackageType::Ec) {
                    log::error!("firmware package did not pass validation");
                    xous::send_message(callback_conn,
                        xous::Message::new_scalar(CB_ID.load(Ordering::Relaxed) as usize,
                        UpdateResult::Abort.to_usize().unwrap(),
                        0, 0, 0)).unwrap();
                    continue;
                } else {
                    susres.set_suspendable(false).unwrap(); // block suspend/resume operations
                    do_update(&mut com, callback_conn, package, EC_GATEWARE_LEN + CTRL_PAGE_LEN,
                        EC_FIRMWARE_BASE, length - (EC_GATEWARE_LEN), "firmware");
                    susres.set_suspendable(true).unwrap(); // resume suspend/resume operations

                    xous::send_message(callback_conn,
                        xous::Message::new_scalar(CB_ID.load(Ordering::Relaxed) as usize,
                        UpdateResult::ProgramDone.to_usize().unwrap(),
                        (length - (EC_GATEWARE_LEN)) as usize, 0, 0)).unwrap();
                }

                // wf200
                let package = unsafe{ core::slice::from_raw_parts(wf_package.as_ptr() as *const u8, xous::EC_WF200_PKG_LEN as usize)};
                let mut temp: [u8; 4] = Default::default();
                temp.copy_from_slice(&package[0x28..0x2c]);
                let length = u32::from_le_bytes(temp); // total length of package

                if !validate_package(package,PackageType::Wf200) {
                    log::error!("WF200 firmware package did not pass validation");
                    xous::send_message(callback_conn,
                        xous::Message::new_scalar(CB_ID.load(Ordering::Relaxed) as usize,
                        UpdateResult::Abort.to_usize().unwrap(),
                        0, 0, 0)).unwrap();
                    continue;
                } else {
                    susres.set_suspendable(false).unwrap(); // block suspend/resume operations
                    do_update(&mut com, callback_conn, package, CTRL_PAGE_LEN,
                        WF200_FIRMWARE_BASE, length, "wf200");
                    susres.set_suspendable(true).unwrap(); // resume suspend/resume operations

                    xous::send_message(callback_conn,
                        xous::Message::new_scalar(CB_ID.load(Ordering::Relaxed) as usize,
                        UpdateResult::ProgramDone.to_usize().unwrap(),
                        length as usize, 0, 0)).unwrap();
                }
                ticktimer.sleep_ms(500).unwrap(); // paranoia wait
                llio.ec_reset().unwrap();
                ticktimer.sleep_ms(4000).unwrap();
                com.link_reset().unwrap();
                com.reseed_ec_trng().unwrap();
                xous::send_message(callback_conn,
                    xous::Message::new_scalar(CB_ID.load(Ordering::Relaxed) as usize,
                    UpdateResult::AutoDone.to_usize().unwrap(),
                    0, 0, 0)).unwrap();

                ticktimer.sleep_ms(2000).unwrap();
                gam.shipmode_blank_request().unwrap();
                ticktimer.sleep_ms(500).unwrap(); // let the screen redraw

                // allow EC to snoop, so that it can wake up the system
                llio.allow_ec_snoop(true).unwrap();
                // allow the EC to power me down
                llio.allow_power_off(true).unwrap();
                // now send the power off command
                com.ship_mode().unwrap();

                // now send the power off command
                com.power_off_soc().unwrap();

                log::info!("CMD: ship mode now!");
                // pause execution, nothing after this should be reachable
                ticktimer.sleep_ms(10000).unwrap(); // ship mode happens in 10 seconds
                log::info!("CMD: if you can read this, ship mode failed!");
            },
            Some(UpdateOp::Quit) => {
                log::info!("quitting updater thread");
                break;
            },
            None => {
                log::error!("received unknown opcode");
            }
        }
    }
    xous::destroy_server(sid).unwrap();
}
#[derive(Debug)]
pub struct EcUpdate {
    start_time: Option<u64>,
    update_cid: xous::CID,
    in_progress: bool,
}
impl EcUpdate {
    pub fn new(env: &mut CommonEnv) -> Self {
        let sid = xous::create_server().unwrap();
        let sid_tuple = sid.to_u32();

        let cb_id = env.register_handler(String::<256>::from_str("ecup"));
        CB_ID.store(cb_id, Ordering::Relaxed);

        xous::create_thread_4(ecupdate_thread, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
        EcUpdate {
            start_time: None,
            update_cid: xous::connect(sid).unwrap(),
            in_progress: false,
        }
    }
}

impl<'a> ShellCmdApi<'a> for EcUpdate {
    cmd_api!(ecup); // inserts boilerplate for command API

    fn process(&mut self, args: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();
        let helpstring = "ecup [gw] [fw] [wf200] [reset] [auto]";

        log::debug!("ecup handling {}", args.as_str().unwrap());
        let mut tokens = args.as_str().unwrap().split(' ');

        if self.in_progress {
            log::debug!("Programming already in progress, can't double-initiate!");
            write!(ret, "Programming already in progress, can't double-initiate!").unwrap();
        } else {
            if let Some(sub_cmd) = tokens.next() {
                match sub_cmd {
                    "fw" => {
                        env.netmgr.connection_manager_stop().unwrap();
                        self.in_progress = true;
                        let start = env.ticktimer.elapsed_ms();
                        self.start_time = Some(start);
                        xous::send_message(self.update_cid,
                            xous::Message::new_scalar(UpdateOp::UpdateFirmware.to_usize().unwrap(), 0, 0, 0, 0)
                        ).unwrap();
                        write!(ret, "Starting EC firmware update").unwrap();
                    }
                    "gw" => {
                        env.netmgr.connection_manager_stop().unwrap();
                        self.in_progress = true;
                        let start = env.ticktimer.elapsed_ms();
                        self.start_time = Some(start);
                        xous::send_message(self.update_cid,
                            xous::Message::new_scalar(UpdateOp::UpdateGateware.to_usize().unwrap(), 0, 0, 0, 0)
                        ).unwrap();
                        write!(ret, "Starting EC gateware update").unwrap();
                    }
                    "reset" => {
                        env.llio.ec_reset().unwrap();
                        env.ticktimer.sleep_ms(4000).unwrap();
                        env.com.link_reset().unwrap();
                        env.com.reseed_ec_trng().unwrap();
                        write!(ret, "EC has been reset, and new firmware loaded.").unwrap();
                    }
                    "wf200" => {
                        env.netmgr.connection_manager_stop().unwrap();
                        self.in_progress = true;
                        let start = env.ticktimer.elapsed_ms();
                        self.start_time = Some(start);
                        xous::send_message(self.update_cid,
                            xous::Message::new_scalar(UpdateOp::UpdateWf200.to_usize().unwrap(), 0, 0, 0, 0)
                        ).unwrap();
                        write!(ret, "Starting EC wf200 update").unwrap();
                    }
                    "auto" => {
                        if ((env.llio.adc_vbus().unwrap() as f64) * 0.005033) > 1.5 {
                            // if power is plugged in, deny powerdown request
                            write!(ret, "Can't EC auto update while charging. Unplug charging cable and try again.").unwrap();
                            return Ok(Some(ret));
                        }

                        env.netmgr.connection_manager_stop().unwrap();
                        env.com.wlan_leave().ok();
                        env.ticktimer.sleep_ms(4000).unwrap(); // give a few seconds for any packets/updates to clear so we don't tigger panics as the EC is about to disappear...

                        self.in_progress = true;
                        let start = env.ticktimer.elapsed_ms();
                        self.start_time = Some(start);
                        xous::send_message(self.update_cid,
                            xous::Message::new_scalar(UpdateOp::UpdateAll.to_usize().unwrap(), 0, 0, 0, 0)
                        ).unwrap();
                        write!(ret, "Starting full EC firmware update").unwrap();
                    }
                    _ => {
                        write!(ret, "{}", helpstring).unwrap();
                    }
                }

            } else {
                write!(ret, "{}", helpstring).unwrap();
            }
        }
        Ok(Some(ret))
    }

    fn callback(&mut self, msg: &xous::MessageEnvelope, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        log::debug!("update callback");
        let mut ret = String::<1024>::new();

        if let xous::Message::Borrow(m) = &msg.body {
            // a bit of a hack: we can't route on ID because that's consumed by the outer shell dispatch loop
            // however, if the form of the message is a Borrowed buffer, treat it as a string and print it.
            let update_buf = unsafe { Buffer::from_memory_message(m) };
            let update_str = update_buf.as_flat::<String::<1024>, _>().unwrap();
            write!(ret, "{}", update_str.as_str()).unwrap();
        } else { // otherwise, unpack it and use the first argument as a sub-opcode type
            xous::msg_scalar_unpack!(msg, result_code, progress, _, _, {
                let end = env.ticktimer.elapsed_ms();
                let elapsed: f64 = (end - self.start_time.unwrap()) as f64;
                match FromPrimitive::from_usize(result_code) {
                    Some(UpdateResult::PackageValid) => {
                        write!(ret, "Firmware package validated in {}ms", elapsed).unwrap();
                    },
                    Some(UpdateResult::ProgramDone) => {
                        write!(ret, "Programming of {} bytes done in {:.1}s. Please restart EC with `ecup reset`.", progress, elapsed / 1000.0).unwrap();
                        env.netmgr.connection_manager_run().unwrap();
                        self.in_progress = false;
                    },
                    Some(UpdateResult::AutoDone) => {
                        write!(ret, "Autoupdate of EC finished. Shutting down now...").unwrap();
                        env.netmgr.connection_manager_run().unwrap();
                        self.in_progress = false;
                    },
                    Some(UpdateResult::Abort) => {
                        write!(ret, "Programming aborted in {:.1}s. Did you stage all the firmware objects?", elapsed / 1000.0).unwrap();
                        env.netmgr.connection_manager_run().unwrap();
                        self.in_progress = false;
                    }
                    _ => write!(ret, "Got unknown update callback: {:?}", result_code).unwrap()
                }
            });
        }
        Ok(Some(ret))
    }
}
