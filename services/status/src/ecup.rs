use std::convert::TryInto;
use modals::Modals;
use num_traits::*;

use sha2::*;
use digest::Digest;

use locales::t;
use xous::msg_blocking_scalar_unpack;
use xous_semver::SemVer;

// The opcodes here are hard-wired so that the shellchat debug commands
// can invoke one of these ops. Normally, we would never do this -- this is
// more of a debugging convenience and eventually I think this dependency
// will go away.
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum UpdateOp {
    #[cfg(feature="dbg-ecupdate")]
    UpdateGateware = 0,
    #[cfg(feature="dbg-ecupdate")]
    UpdateFirmware = 1,
    #[cfg(feature="dbg-ecupdate")]
    UpdateWf200 = 2,
    UpdateAuto = 3,
    Quit = 4,
    CheckCompat = 5,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum UpdateResult {
    PackageInvalid,
    AutoDone,
    NothingToDo,
    Abort,
}
#[derive(PartialEq, Eq)]
enum PackageType {
    Ec,
    Wf200
}

const EC_GATEWARE_BASE: u32 = 0x0;
const EC_GATEWARE_LEN: u32 = 0x1_a000;
const EC_FIRMWARE_BASE: u32 = 0x1_a000;
const WF200_FIRMWARE_BASE: u32 = 0x9_C000;
const CTRL_PAGE_LEN: u32 = 0x1000;

pub(crate) fn ecupdate_thread(sid: xous::SID) {
    let xns = xous_names::XousNames::new().unwrap();
    let mut com = com::Com::new(&xns).unwrap();
    let ticktimer = ticktimer_server::Ticktimer::new().unwrap();
    let llio = llio::Llio::new(&xns);
    let modals = modals::Modals::new(&xns).unwrap();
    let mut susres = susres::Susres::new_without_hook(&xns).unwrap();
    let netmgr = net::NetManager::new();

    if com.flash_acquire().unwrap() == false {
        log::error!("couldn't acquire exclusive access to the EC updater mechanism. All other operations will fail!");
    }

    #[cfg(any(feature="precursor", feature="renode"))]
    let ec_package = xous::syscall::map_memory(
        xous::MemoryAddress::new((xous::EC_FW_PKG_LOC + xous::FLASH_PHYS_BASE) as usize),
        None,
        xous::EC_FW_PKG_LEN as usize,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    ).expect("couldn't map EC firmware package memory range");
    #[cfg(any(feature="precursor", feature="renode"))]
    let wf_package = xous::syscall::map_memory(
        xous::MemoryAddress::new((xous::EC_WF200_PKG_LOC + xous::FLASH_PHYS_BASE) as usize),
        None,
        xous::EC_WF200_PKG_LEN as usize,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    ).expect("couldn't map EC wf200 package memory range");
    #[cfg(not(target_os = "xous"))]
    let mut ec_package = xous::syscall::map_memory(
        None,
        None,
        xous::EC_FW_PKG_LEN as usize,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    ).expect("couldn't map EC firmware package memory range");
    #[cfg(not(target_os = "xous"))]
    for d in ec_package.as_slice_mut::<u8>().iter_mut() { *d = 0xFF } // simulate blank flash
    #[cfg(not(target_os = "xous"))]
    let mut wf_package = xous::syscall::map_memory(
        None,
        None,
        xous::EC_WF200_PKG_LEN as usize,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    ).expect("couldn't map EC wf200 package memory range");
    #[cfg(not(target_os = "xous"))]
    for d in wf_package.as_slice_mut::<u8>().iter_mut() { *d = 0xFF }

    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("EC updater got msg {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            #[cfg(feature="dbg-ecupdate")]
            Some(UpdateOp::UpdateGateware) => { // blocking scalar
                let package = unsafe{ core::slice::from_raw_parts(ec_package.as_ptr() as *const u8, xous::EC_FW_PKG_LEN as usize)};
                if !validate_package(package,PackageType::Ec) {
                    log::error!("firmware package did not pass validation");
                    modals.show_notification(
                        &format!("{} gateware", t!("ecup.invalid", xous::LANG)), None).unwrap();
                } else {
                    log::info!("updating GW");
                    netmgr.connection_manager_stop().ok();
                    llio.com_event_enable(false).ok();
                    susres.set_suspendable(false).unwrap(); // block suspend/resume operations
                    if !do_update(&mut com, &modals, package, CTRL_PAGE_LEN, EC_GATEWARE_BASE,
                    EC_GATEWARE_LEN,
                    "gateware") {
                        xous::return_scalar(msg.sender, UpdateResult::Abort.to_usize().unwrap()).unwrap();
                        continue;
                    }
                    susres.set_suspendable(true).unwrap(); // resume suspend/resume operations
                }
                xous::return_scalar(msg.sender, 0).unwrap();
            },
            #[cfg(feature="dbg-ecupdate")]
            Some(UpdateOp::UpdateFirmware) => { // blocking scalar
                let package = unsafe{ core::slice::from_raw_parts(ec_package.as_ptr() as *const u8, xous::EC_FW_PKG_LEN as usize)};
                if !validate_package(package,PackageType::Ec) {
                    log::error!("firmware package did not pass validation");
                    modals.show_notification(
                        &format!("{} firmware", t!("ecup.invalid", xous::LANG)), None).unwrap();
                } else {
                    let length = u32::from_le_bytes(package[0x28..0x2c].try_into().unwrap());
                    if length == 0xffff_ffff { // nothing was staged at all
                        xous::return_scalar(msg.sender, UpdateResult::PackageInvalid.to_usize().unwrap()).unwrap();
                        continue;
                    }
                    log::info!("updating FW");
                    netmgr.connection_manager_stop().unwrap();
                    llio.com_event_enable(false).ok();
                    susres.set_suspendable(false).unwrap(); // block suspend/resume operations
                    if !do_update(&mut com, &modals, package, EC_GATEWARE_LEN + CTRL_PAGE_LEN,
                    EC_FIRMWARE_BASE, length - (EC_GATEWARE_LEN),
                    "firmware") {
                        xous::return_scalar(msg.sender, UpdateResult::Abort.to_usize().unwrap()).unwrap();
                        continue;
                    }
                    susres.set_suspendable(true).unwrap(); // resume suspend/resume operations
                }
                xous::return_scalar(msg.sender, 0).unwrap();
            },
            #[cfg(feature="dbg-ecupdate")]
            Some(UpdateOp::UpdateWf200) => { // blocking scalar
                let package = unsafe{ core::slice::from_raw_parts(wf_package.as_ptr() as *const u8, xous::EC_WF200_PKG_LEN as usize)};
                if validate_package(package,PackageType::Wf200) {
                    log::info!("updating Wf200");
                    netmgr.connection_manager_stop().unwrap();
                    llio.com_event_enable(false).ok();
                    let length = u32::from_le_bytes(package[0x28..0x2c].try_into().unwrap());
                    susres.set_suspendable(false).unwrap(); // block suspend/resume operations
                    if !do_update(&mut com, &modals, package, CTRL_PAGE_LEN,
                    WF200_FIRMWARE_BASE, length,
                    "WF200") {
                        xous::return_scalar(msg.sender, UpdateResult::Abort.to_usize().unwrap()).unwrap();
                        continue;
                    }
                    susres.set_suspendable(true).unwrap(); // resume suspend/resume operations
                } else {
                    log::error!("wf200 package did not pass validation");
                    modals.show_notification(
                        &format!("{} WF200", t!("ecup.invalid", xous::LANG)), None).unwrap();
                    xous::return_scalar(msg.sender, UpdateResult::PackageInvalid.to_usize().unwrap()).unwrap();
                    continue;
                }

                xous::return_scalar(msg.sender, 0).unwrap();
            },
            Some(UpdateOp::CheckCompat) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let ec_rev = com.get_ec_sw_tag().unwrap(); // fetch the purported rev from the EC. We take it at face value.
                if ec_rev < net::MIN_EC_REV {
                    log::warn!("EC firmware is too old to interoperate with the connection manager.");
                    let mut note = String::from(t!("net.ec_rev_old", xous::LANG));
                    note.push_str(&format!("\n\n{}{}", t!("net.ec_current_rev", xous::LANG), ec_rev.to_string()));
                    modals.show_notification(&note, None).unwrap();
                    xous::return_scalar(msg.sender, 0).unwrap();
                } else {
                    xous::return_scalar(msg.sender, 1).unwrap();
                }
            }),
            Some(UpdateOp::UpdateAuto) => msg_blocking_scalar_unpack!(msg, force_arg, _, _, _, {
                const GW_HASH_OFFSET: usize = 0x58;
                const FW_HASH_OFFSET: usize = 0x38;
                const HASH_LEN: usize = 32;
                const SEMVER_OFFSET: usize = 0x18;
                const SEMVER_LEN: usize = 16;
                const EC_FLASH_BASE: u32 = 0x2000_0000;
                const MIN_EC_VER_WITH_HASHES: SemVer = SemVer {
                    maj: 0, min: 9, rev: 8, extra: 8, commit: None,
                };
                const WF200_HASH_LEN: usize = 0x48; // this is actually hash + signature + keyset

                let force = force_arg != 0;
                log::debug!("force update argument: {:?}", force);

                let mut ec_reported_rev = com.get_ec_sw_tag().unwrap(); // fetch the purported rev from the EC. We take it at face value.
                if !is_ec_rev_sane(&ec_reported_rev) && !force {
                    const RETRY_TOTAL_DURATION_MS: usize = 5000; // it can easily take 2-3 seconds for the EC to boot and respond to version requests
                    const RETRY_INTERVAL_MS: usize = 500;
                    let mut retries = 0;
                    loop {
                        ec_reported_rev = com.get_ec_sw_tag().unwrap(); // fetch the purported rev from the EC. We take it at face value.
                        if is_ec_rev_sane(&ec_reported_rev) {
                            break;
                        }
                        ticktimer.sleep_ms(RETRY_INTERVAL_MS).unwrap();
                        retries += RETRY_INTERVAL_MS;
                        if retries > RETRY_TOTAL_DURATION_MS {
                            break;
                        }
                    }
                    if retries > RETRY_TOTAL_DURATION_MS {
                        // this is typically a result of the EC itself being bricked...nothing we can do about it at this point.
                        log::error!("EC rev report seems bogus: {:?}; aborting autoupdate process.", ec_reported_rev);
                        // this will trigger a dialog box "EC update aborted due to error!"
                        xous::return_scalar(msg.sender, UpdateResult::Abort.to_usize().unwrap()).unwrap();
                        continue;
                    }
                }

                let mut did_something = false;
                let package = unsafe{ core::slice::from_raw_parts(ec_package.as_ptr() as *const u8, xous::EC_FW_PKG_LEN as usize)};

                // the semver *could* be bogus at this point, but we'll validate the package (which contains the semver) before we use it.
                // however, this check is much less computationally expensive than the package validation.
                let length = u32::from_le_bytes(package[0x28..0x2c].try_into().unwrap());
                if length > xous::EC_FW_PKG_LEN { // nothing was staged, or it is bogus (blank FLASH is 0xFFFF_FFFF "length")
                    // only show the warning if the update was forced; otherwise we shouldn't show the warning because it'll pop up every time on a new unit
                    if force {
                        modals.show_notification(
                            &format!("{} gateware", t!("ecup.invalid", xous::LANG)), None).unwrap();
                    }
                    xous::return_scalar(msg.sender, UpdateResult::PackageInvalid.to_usize().unwrap()).unwrap();
                    continue;
                }
                let semver_bytes = &package[0x1000 + length as usize - SEMVER_OFFSET..0x1000 + length as usize - SEMVER_OFFSET + SEMVER_LEN];
                let pkg_ver = SemVer::from(&semver_bytes[..16].try_into().unwrap());
                if (pkg_ver > ec_reported_rev) || force {
                    if validate_package(package,PackageType::Ec) {
                        // check to see if we need to do an update
                        // read the length of the package
                        let gw_hash = &package[0x1000 + length as usize - GW_HASH_OFFSET..0x1000 + length as usize - GW_HASH_OFFSET + HASH_LEN];
                        let fw_hash = &package[0x1000 + length as usize - FW_HASH_OFFSET..0x1000 + length as usize - FW_HASH_OFFSET + HASH_LEN];
                        did_something = true;
                        let mut do_gw = false;
                        let mut do_fw = false;
                        if ec_reported_rev >= MIN_EC_VER_WITH_HASHES {
                            // now determine if we have to flash gateware, firmware, or both.
                            let mut ver_ec_raw = [0u8; 256];
                            match com.flash_verify(
                                EC_FLASH_BASE +
                                EC_GATEWARE_BASE +
                                (length - GW_HASH_OFFSET as u32),
                                &mut ver_ec_raw
                            ) {
                                Err(_) => {
                                    xous::return_scalar(msg.sender, UpdateResult::Abort.to_usize().unwrap()).unwrap();
                                    continue;
                                }
                                _ => {}
                            }
                            if gw_hash != &ver_ec_raw[..HASH_LEN] {
                                do_gw = true;
                                do_fw = true; // if the gateware changed, we *always* update the firmware, because fw also contains the GW hash
                            }
                            if fw_hash != &ver_ec_raw[HASH_LEN..HASH_LEN*2] {
                                do_fw = true;
                            }
                        } else {
                            // this is an old, old version of the EC; always overwrite everything
                            do_gw = true;
                            do_fw = true;
                        }
                        log::info!("Auto-version check results: do_gw: {:?}, do_fw: {:?}", do_gw, do_fw);
                        if do_gw || force {
                            log::info!("updating GW");
                            netmgr.connection_manager_stop().ok();
                            llio.com_event_enable(false).ok();
                            susres.set_suspendable(false).unwrap(); // block suspend/resume operations
                            if !do_update(&mut com, &modals, package, CTRL_PAGE_LEN, EC_GATEWARE_BASE,
                            EC_GATEWARE_LEN,
                            "gateware") {
                                xous::return_scalar(msg.sender, UpdateResult::Abort.to_usize().unwrap()).unwrap();
                                continue;
                            }
                            susres.set_suspendable(true).unwrap(); // resume suspend/resume operations
                        }
                        if do_fw || force {
                            log::info!("updating FW");
                            netmgr.connection_manager_stop().unwrap();
                            llio.com_event_enable(false).ok();
                            susres.set_suspendable(false).unwrap(); // block suspend/resume operations
                            if !do_update(&mut com, &modals, package, EC_GATEWARE_LEN + CTRL_PAGE_LEN,
                            EC_FIRMWARE_BASE, length - (EC_GATEWARE_LEN),
                            "firmware") {
                                xous::return_scalar(msg.sender, UpdateResult::Abort.to_usize().unwrap()).unwrap();
                                continue;
                            }
                            susres.set_suspendable(true).unwrap(); // resume suspend/resume operations
                        }
                    } else {
                        log::error!("firmware package did not pass validation");
                        modals.show_notification(
                            &format!("{} gateware", t!("ecup.invalid", xous::LANG)), None).unwrap();
                        xous::return_scalar(msg.sender, UpdateResult::PackageInvalid.to_usize().unwrap()).unwrap();
                        continue;
                    }
                } else {
                    log::info!("EC Autoupdate check found that EC rev {} is newer or same as update rev {}; no update done", ec_reported_rev.to_string(), pkg_ver.to_string());
                }

                let package = unsafe{ core::slice::from_raw_parts(wf_package.as_ptr() as *const u8, xous::EC_WF200_PKG_LEN as usize)};
                let mut run_wf200_update = false;
                // check to see if we need to do an update. For the WF200, we can only say if the hash is *different*, we don't know if it's newer
                // we assume if it's different, we meant to update it.
                if (ec_reported_rev >= MIN_EC_VER_WITH_HASHES) && !force {
                    let mut ver_wf200_raw = [0u8; 256];
                    match com.flash_verify(
                        EC_FLASH_BASE +
                        WF200_FIRMWARE_BASE,
                        &mut ver_wf200_raw
                    ) {
                        Err(_) => {
                            xous::return_scalar(msg.sender, UpdateResult::Abort.to_usize().unwrap()).unwrap();
                            continue;
                        }
                        _ => {}
                    }
                    // do a shallow check to see if the firmware magic word 'KEYS' is in the location. The intention is that we would
                    // not even try to run the update if nothing valid has been staged and the location would read as 0xFFFFFFFF.
                    if package[CTRL_PAGE_LEN as usize..CTRL_PAGE_LEN as usize + 4] == [0x4b, 0x45, 0x59, 0x53] {
                        if package[CTRL_PAGE_LEN as usize..CTRL_PAGE_LEN as usize + WF200_HASH_LEN] != ver_wf200_raw[..WF200_HASH_LEN] {
                            run_wf200_update = true;
                            log::info!("wf200 rev is different");
                            log::info!("package : {:x?}", &package[CTRL_PAGE_LEN as usize..CTRL_PAGE_LEN as usize + WF200_HASH_LEN]);
                            log::info!("readback: {:x?}", &ver_wf200_raw[..WF200_HASH_LEN]);
                        }
                    } else {
                        log::warn!("Staged WF200 magic number is incorrect, refusing to perform any updates");
                    }
                } else {
                    // ancient version of EC, must run all the updates if any update was run before on the GW
                    run_wf200_update = did_something;
                }
                if run_wf200_update || force {
                    if validate_package(package,PackageType::Wf200) {
                        log::info!("updating Wf200");
                        did_something = true;
                        netmgr.connection_manager_stop().unwrap();
                        llio.com_event_enable(false).ok();
                        let length = u32::from_le_bytes(package[0x28..0x2c].try_into().unwrap());
                        susres.set_suspendable(false).unwrap(); // block suspend/resume operations
                        if !do_update(&mut com, &modals, package, CTRL_PAGE_LEN,
                        WF200_FIRMWARE_BASE, length,
                        "WF200") {
                            xous::return_scalar(msg.sender, UpdateResult::Abort.to_usize().unwrap()).unwrap();
                            continue;
                        }
                        susres.set_suspendable(true).unwrap(); // resume suspend/resume operations
                    } else {
                        log::error!("wf200 package did not pass validation");
                        modals.show_notification(
                            &format!("{} WF200", t!("ecup.invalid", xous::LANG)), None).unwrap();
                        xous::return_scalar(msg.sender, UpdateResult::PackageInvalid.to_usize().unwrap()).unwrap();
                        continue;
                    }
                }

                if did_something {
                    modals.dynamic_notification(Some(t!("ecup.resetting", xous::LANG)), None).unwrap();
                    log::info!("EC firmware had an update");
                    ticktimer.sleep_ms(500).unwrap(); // paranoia wait
                    llio.ec_reset().unwrap(); // firmware should reload
                    ticktimer.sleep_ms(4000).unwrap();
                    com.link_reset().unwrap();
                    com.reseed_ec_trng().unwrap();
                    modals.dynamic_notification_close().unwrap();
                } else {
                    log::info!("Nothing to update on the EC");
                }

                if did_something {
                    xous::return_scalar(msg.sender, UpdateResult::AutoDone.to_usize().unwrap()).unwrap();
                } else {
                    xous::return_scalar(msg.sender, UpdateResult::NothingToDo.to_usize().unwrap()).unwrap();
                }
            }),
            Some(UpdateOp::Quit) => {
                log::info!("quitting updater thread");
                xous::return_scalar(msg.sender, 1).unwrap();
                break;
            },
            None => {
                log::error!("received unknown opcode");
            }
        }
    }
    xous::destroy_server(sid).unwrap();
}


/// copies an image stored in a `package` slice, starting from `pkg_offset` in the `package` with length `len`
/// and writing to FLASH starting at a hardware offset of `flash_start`
fn do_update(com: &mut com::Com, modals: &Modals, package: &[u8], pkg_offset: u32, flash_start: u32, image_len: u32, name: &str) -> bool {
    let tt = ticktimer_server::Ticktimer::new().unwrap();
    // grab an uptime measurement from the EC
    let ut = com.get_ec_uptime().unwrap();
    // pop up a dialog box to warn users, in case they are in the process of resetting the device
    modals.dynamic_notification(Some(
        &format!("{}", t!("ecup.preparing", xous::LANG))
        ), None).unwrap();
    tt.sleep_ms(3000).ok();
    // check the uptime again as a very basic link-up check
    // (usually the link is either stuck at 0, 0xffff, or 0xdddd if the EC is misbehaving)
    let ut_after = com.get_ec_uptime().unwrap();
    if ut_after <= ut || (ut_after - ut) > 5000 {
        log::error!("EC link is not stable, aborting update");
        return false;
    }

    if (pkg_offset + image_len) > package.len() as u32 {
        log::error!("Requested image is larger than the package length");
        return false;
    }
    // erase
    modals.dynamic_notification_update(Some(
        &format!("{}\n({})", t!("ecup.erasing", xous::LANG), name)
        ), None).unwrap();
    log::info!("{}, erasing from 0x{:08x}, 0x{:x} bytes", name, flash_start, image_len);
    if com.flash_erase(flash_start, image_len).unwrap() {
        modals.dynamic_notification_close().unwrap();
    } else {
        modals.dynamic_notification_close().unwrap();
        modals.show_notification(
            &format!("{}\n({})", t!("ecup.abort", xous::LANG), name), None
        ).unwrap();
        return false;
    }
    xous::yield_slice();

    // program
    log::info!("init progress: {:x}->{:x}", pkg_offset, pkg_offset + image_len);
    modals.start_progress(
        &format!("{} {}...", t!("ecup.writing", xous::LANG), name),
        flash_start, flash_start + image_len, flash_start).unwrap();
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
        log::debug!("{} prog 0x{:08x} 4 pages", name, prog_addr);
        if com.flash_program(prog_addr, pages).unwrap() == false {
            modals.finish_progress().unwrap();
            modals.show_notification(
                &format!("{} {}...", t!("ecup.abort", xous::LANG), name), None
            ).unwrap();
            return false;
        }
        prog_addr += 1024;
        progress_ctr += 1;
        if (progress_ctr % 4) == 0 {
            log::info!("{} prog update 0x{:08x} 4*8 pages", name, prog_addr);
            modals.update_progress(prog_addr).unwrap();
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
        log::info!("{} prog 0x{:08x} {} pages (last op)", name, prog_addr, dbgcnt);
        if com.flash_program(prog_addr, pages).unwrap() == false {
            modals.finish_progress().unwrap();
            modals.show_notification(
                &format!("{}\n({})", t!("ecup.abort", xous::LANG), name), None
            ).unwrap();
            return false
        }
    }
    modals.update_progress(pkg_offset + image_len).unwrap(); // my little pet peeve about progress bars always hitting 100%
    xous::yield_slice();
    modals.finish_progress().unwrap();
    true
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

/// returns `false` if the EC rev does not appear sane
fn is_ec_rev_sane(ec_reported_rev: &SemVer) -> bool {
    if ec_reported_rev.maj == 0 && ec_reported_rev.min == 0 && ec_reported_rev.rev == 0 {
        false
    } else {
        if ec_reported_rev.maj > 64 {
            // the purpose of this is to check reports that are 0xdddd, 0xeeee, 0xffff....which are link error
            // codes. We're unlikely to ever get to 64 major releases of this code, so if we see a major release
            // number bigger than 64, we flag it. otoh, if we get to the point of the 65th major release...hopefully
            // this code won't still be around anymore.
            false
        } else {
            true
        }
    }
}