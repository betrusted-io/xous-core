#![cfg_attr(target_os = "none", no_std)]

/// This is the API that other servers use to call the COM. Read this code as if you
/// are calling these functions inside a different process.
pub mod api;

pub use api::BattStats;
use api::{Callback, Opcode};
use xous::{send_message, Error, CID, Message, msg_scalar_unpack};
use xous_ipc::{String, Buffer};
use num_traits::{ToPrimitive, FromPrimitive};

/// mapping of the callback function to the library user
/// this exists in the library user's memory space, so we can have up to one
/// callback per library user.
static mut BATTSTATS_CB: Option<fn(BattStats)> = None;

/// handles callback messages from the COM server, in the library user's process space.
fn battstats_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Callback::BattStats) => msg_scalar_unpack!(msg, lo, hi, _, _, {
                let bs: BattStats = [lo, hi].into();
                unsafe {
                    if let Some(cb) = BATTSTATS_CB {
                        cb(bs)
                    }
                }
            }),
            Some(Callback::Drop) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}
#[derive(Debug)]
pub struct Com {
    conn: CID,
    battstats_sid: Option<xous::SID>,
    ticktimer: ticktimer_server::Ticktimer,
}
impl Com {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        let conn = xns.request_connection_blocking(api::SERVER_NAME_COM).expect("Can't connect to COM server");
        Ok(Com {
            conn,
            battstats_sid: None,
            ticktimer: ticktimer_server::Ticktimer::new().expect("Can't connect to ticktimer"),
        })
    }

    pub fn power_off_soc(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::PowerOffSoc.to_usize().unwrap(), 0, 0, 0, 0)
        ).map(|_| ())
    }

    /// ship mode is synchronous, so that we can schedule order-of-operation dependent tasks around it
    pub fn ship_mode(&self) -> Result<(), xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::ShipMode.to_usize().unwrap(), 0, 0, 0, 0))?;
        if let xous::Result::Scalar1(_) = response {
            Ok(())
        } else {
            log::error!("ship_mode failed to execute");
            Err(xous::Error::InternalError)
        }
    }

    pub fn get_wf200_fw_rev(&self) -> Result<(u8, u8, u8), xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::Wf200Rev.to_usize().unwrap(), 0, 0, 0, 0))?;
        if let xous::Result::Scalar1(rev) = response {
            Ok(((rev >> 16) as u8, (rev >> 8) as u8, rev as u8))
        } else {
            panic!("unexpected return value: {:#?}", response);
        }
    }

    pub fn get_ec_git_rev(&self) -> Result<(u32, bool), Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::EcGitRev.to_usize().unwrap(), 0, 0, 0, 0))?;
        if let xous::Result::Scalar2(rev, dirty) = response {
            let dirtybool: bool;
            if dirty == 0 {
                dirtybool = false;
            } else {
                dirtybool = true;
            }
            Ok((rev as u32, dirtybool))
        } else {
            panic!("unexpected return value: {:#?}", response);
        }
    }

    pub fn send_pds_line(&self, s: &String<512>) -> Result<(), Error> {
        use core::fmt::Write;
        let mut clone_s: String<512> = String::new();
        write!(clone_s, "{}", s.as_str().unwrap()).map_err(|_| xous::Error::AccessDenied)?;

        let buf = Buffer::into_buf(clone_s).or(Err(xous::Error::InternalError))?;
        buf.lend(
            self.conn,
            Opcode::Wf200PdsLine.to_u32().unwrap()
        ).map(|_| ())
    }

    /// this kicks off an async callback for battery status at some later time
    pub fn req_batt_stats(&self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::BattStatsNb.to_usize().unwrap(), 0, 0, 0, 0,)).map(|_| ())
    }

    /// this allows the caller to provide a hook to handle the callback
    pub fn hook_batt_stats(&mut self, cb: fn(BattStats)) -> Result<(), xous::Error> {
        if unsafe{BATTSTATS_CB}.is_some() {
            return Err(xous::Error::MemoryInUse)
        }
        unsafe{BATTSTATS_CB = Some(cb)};
        if self.battstats_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.battstats_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(battstats_server, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
            xous::send_message(self.conn,
                Message::new_scalar(Opcode::RegisterBattStatsListener.to_usize().unwrap(),
                sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize
            )).unwrap();
        }
        Ok(())
    }

    pub fn wifi_disable(&self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::Wf200Disable.to_usize().unwrap(), 0, 0, 0, 0,)).map(|_| ())
    }
    // as wifi_reset() re-initializes the wifi chip, call this after wifi_disable() to re-enable wifi
    pub fn wifi_reset(&self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::Wf200Reset.to_usize().unwrap(), 0, 0, 0, 0,)).expect("couldn't send reset opcode");
        self.ticktimer.sleep_ms(2000).expect("failed in waiting for wifi chip to reset");
        Ok(())
    }
    pub fn set_ssid_scanning(&self, enable: bool) -> Result<(), xous::Error> {
        if enable {
            send_message(self.conn, Message::new_scalar(Opcode::ScanOn.to_usize().unwrap(), 0, 0, 0, 0,)).map(|_| ())
        } else {
            send_message(self.conn, Message::new_scalar(Opcode::ScanOff.to_usize().unwrap(), 0, 0, 0, 0,)).map(|_| ())
        }
    }
    pub fn ssid_scan_updated(&self) -> Result<bool, xous::Error> {
        if let xous::Result::Scalar1(avail) =
            send_message(self.conn, Message::new_blocking_scalar(Opcode::SsidCheckUpdate.to_usize().unwrap(), 0, 0, 0, 0)).unwrap() {
            if avail != 0 {
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Err(xous::Error::InternalError)
        }
    }
    pub fn ssid_fetch_as_string(&self) -> Result<xous_ipc::String::<256>, xous::Error> {
        let ssid_list = xous_ipc::String::<256>::new();
        let mut buf = Buffer::into_buf(ssid_list).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::SsidFetchAsString.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        let response = buf.to_original::<xous_ipc::String::<256>, _>().unwrap();
        Ok(response)
    }

    pub fn get_standby_current(&self) -> Result<Option<i16>, xous::Error> {
        if let xous::Result::Scalar2(valid, current) =
            send_message(self.conn, Message::new_blocking_scalar(Opcode::StandbyCurrent.to_usize().unwrap(), 0, 0, 0, 0)).unwrap() {
            if valid != 0 {
                Ok(Some(current as i16))
            } else {
                Ok(None)
            }
        } else {
            Err(xous::Error::InternalError)
        }
    }
    // note to future self: add other event listener registrations (such as network events) here
}

impl Drop for Com {
    fn drop(&mut self) {
        // if we have callbacks, destroy the battstats callback server
        if let Some(sid) = self.battstats_sid.take() {
            // no need to tell the COM server we're quitting: the next time a callback processes,
            // it will automatically remove my entry as it will receive a ServerNotFound error.

            // tell my handler thread to quit
            let cid = xous::connect(sid).unwrap();
            xous::send_message(cid,
                Message::new_scalar(api::Callback::Drop.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
            unsafe{xous::disconnect(cid).unwrap();}
        }

        // now de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        unsafe{xous::disconnect(self.conn).unwrap();}
    }
}
