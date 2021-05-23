use crate::{ShellCmdApi, CommonEnv};
use xous_ipc::String;

use core::sync::atomic::{AtomicU32, Ordering};
static CB_ID: AtomicU32 = AtomicU32::new(0);
use num_traits::*;

use engine_sha512::*;
use digest::Digest;

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum UpdateOp {
    //UpdateGateware,
    UpdateFirmware,
    //UpdateWf200,
    //ResetEc,
    Quit,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum UpdateResult {
    PackageValid,
    EraseProgress,
    ProgramProgress,
    EraseDone,
    ProgramDone,
    Abort,
}

fn validate_package(pkg: &[u8]) -> bool {
    let hasher = engine_sha512::Sha512Trunc256::new(Some(FallbackStrategy::HardwareThenSoftware));
    let mut temp: [u8; 4] = Default::default();
    temp.copy_from_slice(&pkg[0x20..0x24]);
    if temp != [0x70, 0x72, 0x65, 0x63] {
        log::error!("update package does not have the correct signature");
        return false;
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
    for(&stored, &computed) in pkg[..0x20].iter().zip(digest.iter()) {
        if stored != computed {
            log::error!("update package hash mismatch");
            return false;
        }
    }
    true
}

fn ecupdate_thread(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    let xns = xous_names::XousNames::new().unwrap();
    let com = com::Com::new(&xns).unwrap();
    let callback_conn = xns.request_connection_blocking(crate::SERVER_NAME_SHELLCHAT).unwrap();
    let susres = susres::Susres::new_without_hook(&xns).unwrap();

    let ec_package = xous::syscall::map_memory(
        xous::MemoryAddress::new(xous::EC_FW_PKG_LOC as usize),
        None,
        xous::EC_FW_PKG_LEN as usize,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    ).expect("couldn't map EC firmware package memory range");
    let wf_package = xous::syscall::map_memory(
        xous::MemoryAddress::new(xous::EC_WF200_PKG_LOC as usize),
        None,
        xous::EC_WF200_PKG_LEN as usize,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    ).expect("couldn't map EC wf200 package memory range");
    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("EC updater got msg {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(UpdateOp::UpdateFirmware) => {
                let package = unsafe{ ec_package.as_ptr() as &[u8]};
                if !validate_package(package) {
                    log::error!("firmware package did not pass validation");
                    xous::send_message(callback_conn,
                        xous::Message::new_scalar(CB_ID.load(Ordering::Relaxed) as usize,
                        UpdateResult::Abort.to_usize().unwrap(),
                        0, 0, 0)).unwrap();
                } else {
                    // do the update
                    let mut temp: [u8; 4] = Default::default();
                    temp.copy_from_slice(&package[0x28..0x2c]);
                    let length = u32::from_le_bytes(temp);

                    // erase

                    // program

                    // completed successfully, report the result
                    xous::send_message(callback_conn,
                        xous::Message::new_scalar(CB_ID.load(Ordering::Relaxed) as usize,
                        UpdateResult::ProgramDone.to_usize().unwrap(),
                        length as usize, 0, 0)).unwrap();
                }
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
        use core::fmt::Write;
        let mut ret = String::<1024>::new();
        let helpstring = "ecup [all] [gw] [fw] [wf200]";

        let mut tokens = args.as_str().unwrap().split(' ');

        if self.in_progress {
            write!(ret, "Programming already in progress, can't double-initiate!").unwrap();
        } else {
            if let Some(sub_cmd) = tokens.next() {
                match sub_cmd {
                    "fw" => {
                        self.in_progress = true;
                        let start = env.ticktimer.elapsed_ms();
                        self.start_time = Some(start);
                        xous::send_message(self.update_cid,
                            xous::Message::new_scalar(UpdateOp::UpdateFirmware.to_usize().unwrap(), 0, 0, 0, 0)
                        ).unwrap();
                        write!(ret, "Starting EC firmware update").unwrap();
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
        use core::fmt::Write;

        log::debug!("update callback");
        let mut ret = String::<1024>::new();

        xous::msg_scalar_unpack!(msg, result_code, progress, total, _, {
            let end = env.ticktimer.elapsed_ms();
            let elapsed: f64 = (end - self.start_time.unwrap()) as f64;
            match FromPrimitive::from_usize(result_code) {
                UpdateResult::PackageValid => {
                    write!(ret, "Firmware package validated in {}ms", elapsed).unwrap();
                },
                UpdateResult::EraseDone => {
                    write!(ret, "Erase of {} bytes done", progress).unwrap();
                },
                UpdateResult::EraseProgress => {
                    write!(ret, "Erasing {:.1}% complete", (progress as f64 / total as f64) * 100.0).unwrap();
                },
                UpdateResult::ProgramDone => {
                    write!(ret, "Programming of {} bytes done in {:.1}s", progress, elapsed / 1000.0).unwrap();
                    self.in_progress = false;
                },
                UpdateResult::ProgramProgress => {
                    write!(ret, "Programming {:.1}% complete", (progress as f64 / total as f64) * 100.0).unwrap();
                },
                UpdateResult::Abort => {
                    write!(ret, "Programming aborted in {:.1}s due to an internal error!", elapsed / 1000.0).unwrap();
                    self.in_progress = false;
                }
                _ => write!(ret, "Got unknown update callback: {:?}", result_code).unwrap()
            }
        });
        Ok(Some(ret))
    }
}
