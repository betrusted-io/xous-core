#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use core::sync::atomic::{AtomicBool, Ordering};
use std::collections::HashSet;

use api::*;
use num_traits::*;
use xous::{msg_blocking_scalar_unpack, msg_scalar_unpack};
use xous_ipc::Buffer;

mod implementation {
    pub struct Spinor {
        id: u32,
        handler_conn: xous::CID,
        csr: utralib::CSR<u32>,
        susres: RegManager<{ utra::spinor::SPINOR_NUMREGS }>,
        softirq: utralib::CSR<u32>,
        cur_op: Option<FlashOp>,
        ticktimer: ticktimer_server::Ticktimer,
        // TODO: refactor ecup command to use spinor to operate the reads
        #[cfg(feature = "extra_flush")]
        flusher: MemoryRange,
    }
}

pub fn thread() {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    /*
        Very important to track who has access to the SPINOR server, and limit access. Access to this server is essential for persistent rootkits.
        Here is the list of servers allowed to access, and why:
          - shellchat (for testing ONLY, remove once done)
          - suspend/resume (for suspend locking/unlocking calls)
          - keystore
          - PDDB
          - keyboard (for updating the key map setting, which needs to be loaded upstream of the PDDB)
    */
    #[cfg(any(feature = "precursor", feature = "renode"))]
    let spinor_sid = xns.register_name(api::SERVER_NAME_SPINOR, Some(5)).expect("can't register server");
    #[cfg(not(target_os = "xous"))]
    let spinor_sid = xns.register_name(api::SERVER_NAME_SPINOR, None).expect("can't register server"); // hosted mode we don't care about security of the spinor server
    log::trace!("registered with NS -- {:?}", spinor_sid);

    let handler_conn =
        xous::connect(spinor_sid).expect("couldn't create interrupt handler callback connection");
    let mut spinor = Box::new(Spinor::new(handler_conn));
    spinor.init();

    log::trace!("ready to accept requests");

    // handle suspend/resume with a separate thread, which monitors our in-progress state
    // we can't interrupt an erase or program operation, so the op MUST finish before we can suspend.
    let susres_mgr_sid = xous::create_server().unwrap();
    let (sid0, sid1, sid2, sid3) = susres_mgr_sid.to_u32();
    xous::create_thread_4(susres_thread, sid0 as usize, sid1 as usize, sid2 as usize, sid3 as usize)
        .expect("couldn't start susres handler thread");

    let llio = llio::Llio::new(&xns);

    let mut client_id: Option<[u32; 4]> = None;
    let mut soc_token: Option<[u32; 4]> = None;
    const MAX_ERRLOG_LEN: usize = 512; // this will span a couple erase blocks if my math is right
    let mut ecc_errors: HashSet<(u32, u32, u32, u32)> = HashSet::new();
    let mut staging_write_protect: bool = false;

    loop {
        let mut msg = xous::receive_message(spinor_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::SuspendInner) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                spinor.suspend();
                xous::return_scalar(msg.sender, 1).unwrap();
            }),
            Some(Opcode::ResumeInner) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                spinor.resume();
                xous::return_scalar(msg.sender, 1).unwrap();
            }),
            Some(Opcode::RegisterSocToken) => msg_scalar_unpack!(msg, id0, id1, id2, id3, {
                // only the first process to claim it can have it!
                // make sure to do it correctly at boot: this must be done extremely early in the
                // boot process; any attempt to access this unit for functional ops before this is registered
                // shall panic this is to mitigate a DoS attack on the legitimate registrar
                // that gives way for the incorrect process to grab this token
                if soc_token.is_none() {
                    soc_token = Some([id0 as u32, id1 as u32, id2 as u32, id3 as u32]);
                }
            }),
            Some(Opcode::SetStagingWriteProtect) => msg_scalar_unpack!(msg, id0, id1, id2, id3, {
                if let Some(token) = soc_token {
                    if token[0] == id0 as u32
                        && token[1] == id1 as u32
                        && token[2] == id2 as u32
                        && token[3] == id3 as u32
                    {
                        staging_write_protect = true;
                    }
                }
            }),
            Some(Opcode::ClearStagingWriteProtect) => msg_scalar_unpack!(msg, id0, id1, id2, id3, {
                if let Some(token) = soc_token {
                    if token[0] == id0 as u32
                        && token[1] == id1 as u32
                        && token[2] == id2 as u32
                        && token[3] == id3 as u32
                    {
                        staging_write_protect = false;
                    }
                }
            }),
            Some(Opcode::AcquireExclusive) => msg_blocking_scalar_unpack!(msg, id0, id1, id2, id3, {
                if soc_token.is_none() {
                    // reject any ops until a soc token is registered
                    xous::return_scalar(msg.sender, 0).unwrap();
                }
                if client_id.is_none() && !SUSPEND_PENDING.load(Ordering::Relaxed) {
                    OP_IN_PROGRESS.store(true, Ordering::Relaxed); // lock out suspends when the exclusive lock is acquired
                    llio.wfi_override(true).expect("couldn't shut off WFI");
                    client_id = Some([id0 as u32, id1 as u32, id2 as u32, id3 as u32]);
                    log::trace!("giving {:x?} an exclusive lock", client_id);
                    SUSPEND_FAILURE.store(false, Ordering::Relaxed);
                    xous::return_scalar(msg.sender, 1).unwrap();
                } else {
                    xous::return_scalar(msg.sender, 0).unwrap();
                }
            }),
            Some(Opcode::ReleaseExclusive) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                client_id = None;
                OP_IN_PROGRESS.store(false, Ordering::Relaxed);
                llio.wfi_override(false).expect("couldn't restore WFI");
                xous::return_scalar(msg.sender, 1).unwrap();
            }),
            Some(Opcode::AcquireSuspendLock) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if client_id.is_none() && !OP_IN_PROGRESS.load(Ordering::Relaxed) {
                    SUSPEND_PENDING.store(true, Ordering::Relaxed);
                    xous::return_scalar(msg.sender, 1).expect("couldn't ack AcquireSuspendLock");
                } else {
                    xous::return_scalar(msg.sender, 0).expect("couldn't ack AcquireSuspendLock");
                }
            }),
            Some(Opcode::ReleaseSuspendLock) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                SUSPEND_PENDING.store(false, Ordering::Relaxed);
                xous::return_scalar(msg.sender, 1).expect("couldn't ack ReleaseSuspendLock");
            }),
            Some(Opcode::WriteRegion) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut wr = buffer.to_original::<WriteRegion, _>().unwrap();
                let mut authorized = true;
                if let Some(st) = soc_token {
                    if staging_write_protect
                        && ((wr.start >= xous::SOC_REGION_LOC) && (wr.start < xous::LOADER_LOC))
                        || !staging_write_protect
                            && ((wr.start >= xous::SOC_REGION_LOC) && (wr.start < xous::SOC_STAGING_GW_LOC))
                    {
                        // if only the holder of the ID that matches the SoC token can write to the SOC flash
                        // area other areas are not as strictly controlled because
                        // signature checks ostensibly should catch attempts to modify
                        // them. However, access to the gateware definition would allow one to rewrite
                        // the boot ROM, which would then change the trust root. Therefore, we check this
                        // region specifically.
                        if st != wr.id {
                            wr.result = Some(SpinorError::AccessDenied);
                            authorized = false;
                        }
                    }
                } else {
                    // the soc token MUST be initialized early on, if not, something bad has happened.
                    wr.result = Some(SpinorError::AccessDenied);
                    authorized = false;
                }
                if authorized {
                    match client_id {
                        Some(id) => {
                            if wr.id == id {
                                wr.result = Some(spinor.write_region(&mut wr)); // note: this must reject out-of-bound length requests for security reasons
                            } else {
                                wr.result = Some(SpinorError::IdMismatch);
                            }
                        }
                        _ => {
                            wr.result = Some(SpinorError::NoId);
                        }
                    }
                }
                buffer.replace(wr).expect("couldn't return response code to WriteRegion");
            }
            Some(Opcode::BulkErase) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut wr = buffer.to_original::<BulkErase, _>().unwrap();
                // bounds check to within the PDDB region for bulk erases. Please use standard patching for
                // other regions.
                let authorized = if (wr.start >= xous::PDDB_LOC)
                    && ((wr.start + wr.len) <= (xous::PDDB_LOC + xous::PDDB_LEN))
                {
                    true
                } else {
                    false
                };
                if authorized {
                    match client_id {
                        Some(id) => {
                            if wr.id == id {
                                wr.result = Some(spinor.bulk_erase(&mut wr)); // note: this must reject out-of-bound length requests for security reasons
                            } else {
                                wr.result = Some(SpinorError::IdMismatch);
                            }
                        }
                        _ => {
                            wr.result = Some(SpinorError::NoId);
                        }
                    }
                } else {
                    wr.result = Some(SpinorError::AccessDenied);
                }
                buffer.replace(wr).expect("couldn't return response code to WriteRegion");
            }
            Some(Opcode::EccError) => msg_scalar_unpack!(msg, hw_rep, status, lower_addr, upper_addr, {
                /*
                 Historical notes:
                   - First ECC failure noted June 27, 2022 on CI unit (24/7 re-write testing, few times/day, 2-3 yrs continuous)
                     Location 0x7305030, code 0xb3.
                       Meaning: ecc error; 2 bits flipped (detect but uncorrectable); failure chunk 3.
                       Address is 0x7305030. Chunk 3 means bits 48-63 offset from the address on the left.
                          The lower 4 bits are 0, so the chunk is the bit-offset of the failure into the 16 bytes encoded by the 4 lowest bits.
                          So more precisely, somewhere around 0x7305036-7 range there is a bit flip.
                          This is fairly deep within the PDDB array...suspect possibly a bad power-down event during the CI process?
                   This is the raw log:
                   ERR :spinor: ECC error reported: 0xfffffffc 0xb3b30000 0x3305080 0x7305030 (services\spinor\src\main.rs:830)
                   Archived here: https://ci.betrusted.io/view/Enabled/job/ctap2-tests/64/console
                   - Second ECC failure noted July 13, 2022 on the high-cycle dev unit. The failure actually may
                     be linked to an aborted write during backup generation; it was reported at an address that
                     up until now was never used. This error was different from the previous one in that after
                     tripping the ROM would only return 0xFF, and it would not clear. The error address
                     was 0x01D7_F0A0 - just inside the backup block. The backup code has been fixed to not
                     use two disjoint patch operations to merge its data, and to instead merge the write data
                     before patching. Error was cleared by erasing the block, and has not since been observed again.
                */
                if !ecc_errors.contains(&(hw_rep as u32, status as u32, lower_addr as u32, upper_addr as u32))
                {
                    if ecc_errors.len() < MAX_ERRLOG_LEN {
                        ecc_errors.insert((
                            hw_rep as u32,
                            status as u32,
                            lower_addr as u32,
                            upper_addr as u32,
                        ));
                    } else {
                        log::warn!("ECC log overflow, error not stored");
                    }
                    log::error!(
                        "ECC error reported: 0x{:x} 0x{:x} 0x{:x} 0x{:x}",
                        hw_rep,
                        status,
                        lower_addr,
                        upper_addr
                    );
                    // how to read:
                    // first word is what address the HW PHY was set to when the interrupt flipped. This
                    // doesn't seem to be useful. second word is the status. Top 16 bits
                    // -> top 512Mbits; lower 16 bits is lower 512Mbits    the 16-bit word
                    // will be double-byte repeated because DDR. third word is the lower
                    // 512Mbit address fourth word is the upper 512Mbit address
                    // There is only an error if the second word is non-zero for a given ECC address. That is,
                    // it seems   the address word is always updated, so you'll read
                    // something out akin to the last thing touched   by the ECC engine,
                    // but there's only an error if the status word indicates that.
                }
            }),
            Some(Opcode::EccLog) => {
                for (index, entry) in ecc_errors.iter().enumerate() {
                    log::info!("{}: {:x?}", index, entry);
                }
                ecc_errors.clear();
            }
            None => {
                log::error!("couldn't convert opcode");
                break;
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    let quitconn = xous::connect(susres_mgr_sid).unwrap();
    xous::send_message(
        quitconn,
        xous::Message::new_scalar(api::SusResOps::Quit.to_usize().unwrap(), 0, 0, 0, 0),
    )
    .unwrap();
    unsafe {
        xous::disconnect(quitconn).unwrap();
    }

    xns.unregister_server(spinor_sid).unwrap();
    xous::destroy_server(spinor_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
