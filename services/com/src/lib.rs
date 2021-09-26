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
    ec_lock_id: Option<[u32; 4]>,
    ec_acquired: bool,
}
impl Com {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_COM).expect("Can't connect to COM server");
        Ok(Com {
            conn,
            battstats_sid: None,
            ticktimer: ticktimer_server::Ticktimer::new().expect("Can't connect to ticktimer"),
            ec_lock_id: None,
            ec_acquired: false,
        })
    }
    pub fn conn(&self) -> CID {self.conn}
    pub fn getop_backlight(&self) -> u32 {Opcode::SetBackLight.to_u32().unwrap()}

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

    pub fn link_reset(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_blocking_scalar(Opcode::LinkReset.to_usize().unwrap(), 0, 0, 0, 0)
        ).map(|_| ())
    }
    pub fn reseed_ec_trng(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::ReseedTrng.to_usize().unwrap(), 0, 0, 0, 0)
        ).map(|_| ())
    }
    pub fn get_ec_uptime(&self) -> Result<u64, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::GetUptime.to_usize().unwrap(), 0, 0, 0, 0)
        )?;
        if let xous::Result::Scalar2(lsb, msb) = response {
            Ok( lsb as u64 | (msb as u64) << 32)
        } else {
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

    pub fn get_batt_stats_blocking(&mut self) -> Result<BattStats, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::BattStats.to_usize().unwrap(), 0, 0, 0, 0))?;
        if let xous::Result::Scalar2(rs0, rs1) = response {
            let bs: BattStats = [rs0, rs1].into();
            Ok(bs)
        } else {
            Err(xous::Error::InternalError)
        }
    }

    pub fn get_more_stats(&mut self) -> Result<[u16; 15], xous::Error> {
        let alloc_stats: [u16; 15] = [0; 15];
        let mut buf = Buffer::into_buf(alloc_stats).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::MoreStats.to_u32().unwrap())?;

        let stats: [u16; 15] = buf.to_original::<[u16; 15], _>().unwrap();
        Ok(stats)
    }

    pub fn poll_usb_cc(&mut self) -> Result<(bool, [u16; 3], u8), xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::PollUsbCc.to_usize().unwrap(), 0, 0, 0, 0))?;
        if let xous::Result::Scalar2(val1, val2) = response {
            let event = if ((val1 >> 16) & 0xff) == 0 {
                false
            } else {
                true
            };
            let regs: [u16; 3] = [
                (val1 & 0xFFFF) as u16,
                (val2 & 0xFFFF) as u16,
                ((val2 >> 16) & 0xFF) as u16
            ];
            let rev: u8 = ((val1 >> 24) & 0xff) as u8;
            Ok((event, regs, rev))
        } else {
            Err(xous::Error::InternalError)
        }
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

    pub fn set_boost(&self, on: bool) -> Result<(), xous::Error> {
        if on {
            send_message(self.conn, Message::new_scalar(Opcode::BoostOn.to_usize().unwrap(), 0, 0, 0, 0,)).map(|_| ())
        } else {
            send_message(self.conn, Message::new_scalar(Opcode::BoostOff.to_usize().unwrap(), 0, 0, 0, 0,)).map(|_| ())
        }
    }

    // numbers from 0-255 represent backlight brightness. Note that only the top 5 bits are used.
    pub fn set_backlight(&self, main: u8, secondary: u8) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::SetBackLight.to_usize().unwrap(),
                (main >> 3) as usize,
                (secondary >> 3) as usize,
                0, 0
            )
        ).map(|_| ())
    }

    pub fn is_charging(&self) -> Result<bool, xous::Error> {
        if let xous::Result::Scalar1(state) =
            send_message(self.conn,
                Message::new_blocking_scalar(Opcode::IsCharging.to_usize().unwrap(), 0, 0, 0, 0)).unwrap() {
            if state != 0 {
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Err(xous::Error::InternalError)
        }
    }

    pub fn request_charging(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::RequestCharging.to_usize().unwrap(), 0, 0, 0, 0
        )).map(|_| ())
    }

    pub fn gyro_read_blocking(&self) -> Result<(u16, u16, u16, u16), xous::Error> {
        if let xous::Result::Scalar2(x_y, z_id) =
            send_message(self.conn,
                Message::new_blocking_scalar(Opcode::ImuAccelReadBlocking.to_usize().unwrap(), 0, 0, 0, 0)).unwrap() {

            let x = (x_y >> 16) as u16;
            let y = (x_y & 0xffff) as u16;
            let z = (z_id >> 16) as u16;
            let id = (z_id & 0xffff) as u16;
            Ok((x, y, z, id))
        } else {
            Err(xous::Error::InternalError)
        }
    }

    pub fn flash_acquire(&mut self) -> Result<bool, xous::Error> {
        let (id0, id1, id2, id3) = xous::create_server_id()?.to_u32();
        self.ec_lock_id = Some([id0, id1, id2, id3]);
        if let xous::Result::Scalar1(acquired) =
            send_message(self.conn,
                Message::new_blocking_scalar(Opcode::FlashAcquire.to_usize().unwrap(), id0 as usize, id1 as usize, id2 as usize, id3 as usize)).unwrap() {
            if acquired != 0 {
                self.ec_acquired = true;
                Ok(true)
            } else {
                self.ec_acquired = false;
                Ok(false)
            }
        } else {
            self.ec_acquired = false;
            Err(xous::Error::InternalError)
        }
    }

    pub fn flash_erase(&mut self, addr: u32, len: u32) -> Result<bool, xous::Error> {
        if !self.ec_acquired {
            return Err(xous::Error::AccessDenied)
        }
        let flashop = api::FlashRecord {
            id: self.ec_lock_id.unwrap(),
            op: api::FlashOp::Erase(addr, len),
        };
        let mut buf = Buffer::into_buf(flashop).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::FlashOp.to_u32().unwrap()).expect("couldn't send flash erase command");
        match buf.to_original().unwrap() {
            api::FlashResult::Pass => {
                Ok(true)
            },
            api::FlashResult::Fail => {
                Ok(false)
            }
        }
    }

    pub fn flash_program(&mut self, addr: u32, page: [Option<[u8; 256]>; 4]) -> Result<bool, xous::Error> {
        if !self.ec_acquired {
            return Err(xous::Error::AccessDenied)
        }
        let flashop = api::FlashRecord {
            id: self.ec_lock_id.unwrap(),
            op: api::FlashOp::Program(addr, page)
        };
        let mut buf = Buffer::into_buf(flashop).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::FlashOp.to_u32().unwrap()).expect("couldn't send flash program command");
        match buf.to_original().unwrap() {
            api::FlashResult::Pass => {
                Ok(true)
            },
            api::FlashResult::Fail => {
                Ok(false)
            }
        }
    }

    pub fn wlan_set_on(&mut self) -> Result<xous::Result, xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(Opcode::WlanOn.to_usize().unwrap(), 0, 0, 0, 0),
        )
    }

    pub fn wlan_set_off(&mut self) -> Result<xous::Result, xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(Opcode::WlanOff.to_usize().unwrap(), 0, 0, 0, 0),
        )
    }

    pub fn wlan_set_ssid(&mut self, s: &String<1024>) -> Result<xous::Result, xous::Error> {
        use core::fmt::Write;
        // Enforce WF200 driver API length limit
        const WF200_SSID_MAX_LEN: usize = 32;
        if s.len() > WF200_SSID_MAX_LEN {
            return Err(xous::Error::InvalidString);
        }
        let mut copy: String::<WF200_SSID_MAX_LEN> = String::new();
        let _ = write!(copy, "{}", s);
        let buf = Buffer::into_buf(copy).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::WlanSetSSID.to_u32().unwrap())
    }

    pub fn wlan_set_pass(&mut self, s: &String<1024>) -> Result<xous::Result, xous::Error> {
        use core::fmt::Write;
        // Enforce WF200 driver API length limit
        const WF200_PASS_MAX_LEN: usize = 64;
        if s.len() > WF200_PASS_MAX_LEN {
            return Err(xous::Error::InvalidString);
        }
        let mut copy: String::<WF200_PASS_MAX_LEN> = String::new();
        let _ = write!(copy, "{}", s);
        let buf = Buffer::into_buf(copy).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::WlanSetPass.to_u32().unwrap())
    }

    pub fn wlan_join(&mut self) -> Result<xous::Result, xous::Error> {
        // TODO: how to make this return success/fail status from WF200?
        send_message(
            self.conn,
            Message::new_scalar(Opcode::WlanJoin.to_usize().unwrap(), 0, 0, 0, 0),
        )
    }

    pub fn wlan_leave(&mut self) -> Result<xous::Result, xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(Opcode::WlanLeave.to_usize().unwrap(), 0, 0, 0, 0),
        )
    }

    pub fn wlan_status(&mut self) -> Result<String<160>, xous::Error> {
        // TODO: how to make this return IP, netmask, gateway, DNS server, and STA MAC?
        const STATUS_MAX_LEN: usize = 160;
        let status = xous_ipc::String::<STATUS_MAX_LEN>::new();
        let mut buf = Buffer::into_buf(status).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::WlanStatus.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        let response = buf.to_original::<xous_ipc::String::<STATUS_MAX_LEN>, _>().unwrap();
        Ok(response)
    }

}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
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
        if REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
    }
}
