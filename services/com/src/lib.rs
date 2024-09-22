#![cfg_attr(target_os = "none", no_std)]

/// This is the API that other servers use to call the COM. Read this code as if you
/// are calling these functions inside a different process.
pub mod api;

use std::cell::RefCell;
use std::collections::VecDeque;

pub use api::*;
pub use com_rs::serdes::Ipv4Conf;
use com_rs::{DhcpState, LinkState};
use num_traits::{FromPrimitive, ToPrimitive};
use xous::{CID, Error, Message, msg_scalar_unpack, send_message};
use xous_ipc::{Buffer, String};
use xous_semver::SemVer;

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
    ec_lock_id: Option<[u32; 4]>,
    ec_acquired: bool,
    /// this is a hack to make loopbacks work on smoltcp. Work-around taken from Redox, but tracking this
    /// issue as well: <https://github.com/smoltcp-rs/smoltcp/issues/50> and <https://github.com/smoltcp-rs/smoltcp/issues/55>
    loopback_buf: RefCell<VecDeque<Vec<u8>>>,
}
impl Com {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn =
            xns.request_connection_blocking(api::SERVER_NAME_COM).expect("Can't connect to COM server");
        Ok(Com {
            conn,
            battstats_sid: None,
            ec_lock_id: None,
            ec_acquired: false,
            loopback_buf: RefCell::new(VecDeque::new()),
        })
    }

    pub fn conn(&self) -> CID { self.conn }

    pub fn getop_backlight(&self) -> u32 { Opcode::SetBackLight.to_u32().unwrap() }

    #[deprecated(
        note = "Uses susres.immediate_poweroff() instead, as power sequencing requirements have changed."
    )]
    pub fn power_off_soc(&self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::PowerOffSoc.to_usize().unwrap(), 0, 0, 0, 0))
            .map(|_| ())
    }

    /// ship mode is synchronous, so that we can schedule order-of-operation dependent tasks around it
    pub fn ship_mode(&self) -> Result<(), xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::ShipMode.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar1(_) = response {
            Ok(())
        } else {
            log::error!("ship_mode failed to execute");
            Err(xous::Error::InternalError)
        }
    }

    pub fn link_reset(&self) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::LinkReset.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .map(|_| ())
    }

    pub fn ping(&self, value: usize) -> Result<usize, xous::Error> {
        if let xous::Result::Scalar1(pong) = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::Ping.to_usize().unwrap(), value, 0, 0, 0),
        )? {
            Ok(pong)
        } else {
            Err(xous::Error::InternalError)
        }
    }

    pub fn reseed_ec_trng(&self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::ReseedTrng.to_usize().unwrap(), 0, 0, 0, 0))
            .map(|_| ())
    }

    pub fn get_ec_uptime(&self) -> Result<u64, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::GetUptime.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar2(lsb, msb) = response {
            Ok(lsb as u64 | (msb as u64) << 32)
        } else {
            Err(xous::Error::InternalError)
        }
    }

    pub fn get_wf200_fw_rev(&self) -> Result<SemVer, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::Wf200Rev.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar1(rev) = response {
            Ok(SemVer {
                maj: ((rev >> 16) & 0xFF) as u16,
                min: ((rev >> 8) & 0xFF) as u16,
                rev: (rev & 0xFF) as u16,
                extra: 0,
                commit: None,
            })
        } else {
            panic!("unexpected return value: {:#?}", response);
        }
    }

    /// this is the Git rev of the *soc*, not the firmware.
    pub fn get_ec_git_rev(&self) -> Result<(u32, bool), Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::EcGitRev.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
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

    /// this is the rev of the firmware
    pub fn get_ec_sw_tag(&self) -> Result<SemVer, Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::EcSwTag.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar1(rev) = response {
            Ok(SemVer {
                maj: ((rev >> 24) & 0xff) as u16,
                min: ((rev >> 16) & 0xff) as u16,
                rev: ((rev >> 8) & 0xff) as u16,
                extra: ((rev >> 0) & 0xff) as u16,
                commit: None,
            })
        } else {
            panic!("unexpected return value: {:#?}", response);
        }
    }

    pub fn send_pds_line(&self, s: &String<512>) -> Result<(), Error> {
        use core::fmt::Write;
        let mut clone_s: String<512> = String::new();
        write!(clone_s, "{}", s.as_str().unwrap()).map_err(|_| xous::Error::AccessDenied)?;

        let buf = Buffer::into_buf(clone_s).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::Wf200PdsLine.to_u32().unwrap()).map(|_| ())
    }

    /// this kicks off an async callback for battery status at some later time
    pub fn req_batt_stats(&self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::BattStatsNb.to_usize().unwrap(), 0, 0, 0, 0))
            .map(|_| ())
    }

    /// this allows the caller to provide a hook to handle the callback
    pub fn hook_batt_stats(&mut self, cb: fn(BattStats)) -> Result<(), xous::Error> {
        if unsafe { BATTSTATS_CB }.is_some() {
            return Err(xous::Error::MemoryInUse);
        }
        unsafe { BATTSTATS_CB = Some(cb) };
        if self.battstats_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.battstats_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(
                battstats_server,
                sid_tuple.0 as usize,
                sid_tuple.1 as usize,
                sid_tuple.2 as usize,
                sid_tuple.3 as usize,
            )
            .unwrap();
            xous::send_message(
                self.conn,
                Message::new_scalar(
                    Opcode::RegisterBattStatsListener.to_usize().unwrap(),
                    sid_tuple.0 as usize,
                    sid_tuple.1 as usize,
                    sid_tuple.2 as usize,
                    sid_tuple.3 as usize,
                ),
            )
            .unwrap();
        }
        Ok(())
    }

    pub fn get_batt_stats_blocking(&mut self) -> Result<BattStats, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::BattStats.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
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
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::PollUsbCc.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar2(val1, val2) = response {
            let event = if ((val1 >> 16) & 0xff) == 0 { false } else { true };
            let regs: [u16; 3] =
                [(val1 & 0xFFFF) as u16, (val2 & 0xFFFF) as u16, ((val2 >> 16) & 0xFF) as u16];
            let rev: u8 = ((val1 >> 24) & 0xff) as u8;
            Ok((event, regs, rev))
        } else {
            Err(xous::Error::InternalError)
        }
    }

    pub fn wifi_disable(&self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::Wf200Disable.to_usize().unwrap(), 0, 0, 0, 0))
            .map(|_| ())
    }

    // as wifi_reset() re-initializes the wifi chip, call this after wifi_disable() to re-enable wifi
    pub fn wifi_reset(&self) -> Result<usize, xous::Error> {
        let ret = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::Wf200Reset.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't send reset opcode");
        if let xous::Result::Scalar1(time) = ret {
            log::info!("WF200 reset took {}ms", time);
            Ok(time)
        } else {
            Err(xous::Error::Timeout)
        }
    }

    pub fn set_ssid_scanning(&self, enable: bool) -> Result<(), xous::Error> {
        if enable {
            send_message(self.conn, Message::new_scalar(Opcode::ScanOn.to_usize().unwrap(), 0, 0, 0, 0))
                .map(|_| ())
        } else {
            send_message(self.conn, Message::new_scalar(Opcode::ScanOff.to_usize().unwrap(), 0, 0, 0, 0))
                .map(|_| ())
        }
    }

    // this function no longer works, must rely on the event-based response via ComInt mechanism
    #[deprecated]
    pub fn ssid_scan_updated(&self) -> Result<bool, xous::Error> {
        if let xous::Result::Scalar1(avail) = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::SsidCheckUpdate.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .unwrap()
        {
            if avail != 0 { Ok(true) } else { Ok(false) }
        } else {
            Err(xous::Error::InternalError)
        }
    }

    // superceded by ssid_fetch_as_list in versions later 0.9.5 (non-inclusive)
    #[deprecated]
    pub fn ssid_fetch_as_string(&self) -> Result<xous_ipc::String<256>, xous::Error> {
        let ssid_list = xous_ipc::String::<256>::new();
        let mut buf = Buffer::into_buf(ssid_list).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::SsidFetchAsString.to_u32().unwrap())
            .or(Err(xous::Error::InternalError))?;
        let response = buf.to_original::<xous_ipc::String<256>, _>().unwrap();
        Ok(response)
    }

    /// returns a vector of `(u8, String)` tuples that represent rssi + AP name
    /// Note: this only returns the very most recent incremental scan results from the wifi chip directly.
    /// The aggregated results of multiple scan passes are accessible from the connection manager via the
    /// NetMgr object.
    pub fn ssid_fetch_as_list(&self) -> Result<Vec<(u8, std::string::String)>, xous::Error> {
        let ssid_alloc = SsidReturn::default();
        let mut buf = Buffer::into_buf(ssid_alloc).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::SsidFetchAsStringV2.to_u32().unwrap())
            .or(Err(xous::Error::InternalError))?;
        let response = buf.to_original::<SsidReturn, _>().unwrap();
        let mut ret = Vec::<(u8, std::string::String)>::new();
        for ssid in response.list {
            ret.push((
                ssid.rssi,
                std::string::String::from(ssid.name.as_str().unwrap_or("UTF-8 Parse Error")),
            ));
        }
        Ok(ret)
    }

    pub fn get_standby_current(&self) -> Result<Option<i16>, xous::Error> {
        if let xous::Result::Scalar2(valid, current) = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::StandbyCurrent.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .unwrap()
        {
            if valid != 0 { Ok(Some(current as i16)) } else { Ok(None) }
        } else {
            Err(xous::Error::InternalError)
        }
    }

    pub fn set_boost(&self, on: bool) -> Result<(), xous::Error> {
        if on {
            send_message(self.conn, Message::new_scalar(Opcode::BoostOn.to_usize().unwrap(), 0, 0, 0, 0))
                .map(|_| ())
        } else {
            send_message(self.conn, Message::new_scalar(Opcode::BoostOff.to_usize().unwrap(), 0, 0, 0, 0))
                .map(|_| ())
        }
    }

    // numbers from 0-255 represent backlight brightness. Note that only the top 5 bits are used.
    pub fn set_backlight(&self, main: u8, secondary: u8) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(
                Opcode::SetBackLight.to_usize().unwrap(),
                (main >> 3) as usize,
                (secondary >> 3) as usize,
                0,
                0,
            ),
        )
        .map(|_| ())
    }

    pub fn is_charging(&self) -> Result<bool, xous::Error> {
        if let xous::Result::Scalar1(state) = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::IsCharging.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .unwrap()
        {
            if state != 0 { Ok(true) } else { Ok(false) }
        } else {
            Err(xous::Error::InternalError)
        }
    }

    pub fn request_charging(&self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::RequestCharging.to_usize().unwrap(), 0, 0, 0, 0))
            .map(|_| ())
    }

    pub fn gyro_read_blocking(&self) -> Result<(u16, u16, u16, u16), xous::Error> {
        if let xous::Result::Scalar2(x_y, z_id) = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::ImuAccelReadBlocking.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .unwrap()
        {
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
        if let xous::Result::Scalar1(acquired) = send_message(
            self.conn,
            Message::new_blocking_scalar(
                Opcode::FlashAcquire.to_usize().unwrap(),
                id0 as usize,
                id1 as usize,
                id2 as usize,
                id3 as usize,
            ),
        )
        .unwrap()
        {
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
            return Err(xous::Error::AccessDenied);
        }
        let flashop = api::FlashRecord { id: self.ec_lock_id.unwrap(), op: api::FlashOp::Erase(addr, len) };
        let mut buf = Buffer::into_buf(flashop).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::FlashOp.to_u32().unwrap())
            .expect("couldn't send flash erase command");
        match buf.to_original().unwrap() {
            api::FlashResult::Pass => Ok(true),
            api::FlashResult::Fail => Ok(false),
        }
    }

    pub fn flash_program(&mut self, addr: u32, page: [Option<[u8; 256]>; 4]) -> Result<bool, xous::Error> {
        if !self.ec_acquired {
            return Err(xous::Error::AccessDenied);
        }
        let flashop =
            api::FlashRecord { id: self.ec_lock_id.unwrap(), op: api::FlashOp::Program(addr, page) };
        let mut buf = Buffer::into_buf(flashop).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::FlashOp.to_u32().unwrap())
            .expect("couldn't send flash program command");
        match buf.to_original().unwrap() {
            api::FlashResult::Pass => Ok(true),
            api::FlashResult::Fail => Ok(false),
        }
    }

    /// Reads a page of data out of the EC, starting at address `addr`. Always reads 256 bytes.
    /// The address is in absolute addressing in the EC space, which means this routine, rather
    /// deliberately, could be used to also read RAM and CSRs in the EC...
    pub fn flash_verify(&mut self, addr: u32, page: &mut [u8; 256]) -> Result<(), xous::Error> {
        if !self.ec_acquired {
            return Err(xous::Error::AccessDenied);
        }
        let flashop =
            api::FlashRecord { id: self.ec_lock_id.unwrap(), op: api::FlashOp::Verify(addr, [0u8; 256]) };
        let mut buf = Buffer::into_buf(flashop).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::FlashOp.to_u32().unwrap())
            .expect("couldn't send flash program command");
        let ret = buf.to_original::<api::FlashRecord, _>().unwrap();
        match ret.op {
            FlashOp::Verify(_a, d) => {
                page.copy_from_slice(&d);
                Ok(())
            }
            _ => Err(xous::Error::InternalError),
        }
    }

    /// This blocks until the COM responds from initializing the Wifi chip
    pub fn wlan_set_on(&mut self) -> Result<xous::Result, xous::Error> {
        send_message(self.conn, Message::new_blocking_scalar(Opcode::WlanOn.to_usize().unwrap(), 0, 0, 0, 0))
    }

    pub fn wlan_set_off(&mut self) -> Result<xous::Result, xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::WlanOff.to_usize().unwrap(), 0, 0, 0, 0))
    }

    pub fn wlan_set_ssid(&mut self, s: &str) -> Result<xous::Result, xous::Error> {
        use core::fmt::Write;
        // Enforce WF200 driver API length limit
        if s.len() > api::WF200_SSID_MAX_LEN {
            return Err(xous::Error::InvalidString);
        }
        let mut copy: String<{ api::WF200_SSID_MAX_LEN }> = String::new();
        let _ = write!(copy, "{}", s);
        let buf = Buffer::into_buf(copy).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::WlanSetSSID.to_u32().unwrap())
    }

    pub fn wlan_set_pass(&mut self, s: &str) -> Result<xous::Result, xous::Error> {
        use core::fmt::Write;
        // Enforce WF200 driver API length limit
        if s.len() > api::WF200_PASS_MAX_LEN {
            return Err(xous::Error::InvalidString);
        }
        let mut copy: String<{ api::WF200_PASS_MAX_LEN }> = String::new();
        let _ = write!(copy, "{}", s);
        let buf = Buffer::into_buf(copy).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::WlanSetPass.to_u32().unwrap())
    }

    pub fn wlan_join(&mut self) -> Result<xous::Result, xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::WlanJoin.to_usize().unwrap(), 0, 0, 0, 0))
    }

    pub fn wlan_leave(&mut self) -> Result<xous::Result, xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::WlanLeave.to_usize().unwrap(), 0, 0, 0, 0))
    }

    /// Note: applications should poll the `NetManager::read_wifi_state()` call for wifi status information,
    /// not the COM directly. This is because the `wlan_status` call is fairly heavy weight, and the
    /// `NetManager::read_wifi_state()` will cache this information making the status check lighter-weight
    /// overall.
    pub fn wlan_status(&self) -> Result<WlanStatus, xous::Error> {
        let status = WlanStatusIpc::default();
        let mut buf = Buffer::into_buf(status).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::WlanStatus.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        let response = buf.to_original::<WlanStatusIpc, _>().unwrap();
        Ok(WlanStatus::from_ipc(response))
    }

    pub fn wlan_get_config(&self) -> Result<Ipv4Conf, xous::Error> {
        let prealloc = Ipv4Conf::default().encode_u16();
        let mut buf = Buffer::into_buf(prealloc).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::WlanGetConfig.to_u32().expect("WlanGetConfig failed"))
            .or(Err(xous::Error::InternalError))?;
        let response = buf.to_original().expect("Couldn't convert WlanGetConfig buffer");
        let config = Ipv4Conf::decode_u16(&response);
        if (config.mac[0] & 0xFE) == 0x01 || (config.addr[0] & 0xF0) == 0xE0 {
            // something is wrong with the COM; probably the link is being reset or not in a proper state.
            Err(xous::Error::BadAddress)
        } else {
            Ok(config)
        }
    }

    pub fn wlan_debug(&self) -> Result<WlanDebug, xous::Error> {
        let prealloc = WlanDebug::default();
        let mut buf = Buffer::into_buf(prealloc).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::WlanDebug.to_u32().expect("WlanDebug failed"))
            .or(Err(xous::Error::InternalError))?;
        let response = buf.to_original().expect("Couldn't convert WlanDebug buffer");
        Ok(response)
    }

    pub fn wlan_fetch_packet(&self, pkt: &mut [u8]) -> Result<(), xous::Error> {
        if pkt.len() > NET_MTU {
            return Err(xous::Error::OutOfMemory);
        }
        let mut prealloc: [u8; NET_MTU] = [0; NET_MTU];
        let len_bytes = (pkt.len() as u16).to_be_bytes();
        prealloc[0] = len_bytes[0];
        prealloc[1] = len_bytes[1];
        let mut buf = Buffer::into_buf(prealloc).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::WlanFetchPacket.to_u32().expect("WlanFetchPacket failed"))
            .or(Err(xous::Error::InternalError))?;
        let response = buf.as_flat::<[u8; NET_MTU], _>().expect("couldn't convert WlanFetchPacket buffer");
        for (&src, dst) in response.iter().zip(pkt.iter_mut()) {
            *dst = src;
        }
        Ok(())
    }

    pub fn wlan_fetch_loopback_packet(&self, pkt: &mut [u8]) -> Result<(), xous::Error> {
        // this is a hack to make loopbacks work on smoltcp. Work-around taken from Redox, but tracking this
        // issue as well: https://github.com/smoltcp-rs/smoltcp/issues/50 and https://github.com/smoltcp-rs/smoltcp/issues/55
        // Inject the loopback packets. loopback packets always take priority, for now.
        if let Some(loop_packet) = self.loopback_buf.borrow_mut().pop_front() {
            // pkt.len() is almost always not loop_packet.len(), just copy the first bit that can fit and the
            // rest is garbarge...
            for (&s, d) in loop_packet.iter().zip(pkt.iter_mut()) {
                *d = s;
            }
        }
        Ok(())
    }

    // this is a hack to make loopbacks work on smoltcp. Work-around taken from Redox, but tracking this issue
    // as well: https://github.com/smoltcp-rs/smoltcp/issues/50 and https://github.com/smoltcp-rs/smoltcp/issues/55
    // this function handles enqueuing packets for local injection
    pub fn wlan_queue_loopback(&self, pk: &[u8]) { self.loopback_buf.borrow_mut().push_back(pk.to_vec()); }

    pub fn wlan_send_packet(&self, pkt: &[u8]) -> Result<(), xous::Error> {
        if pkt.len() > NET_MTU {
            return Err(xous::Error::OutOfMemory);
        }
        let mut prealloc: [u8; NET_MTU + 2] = [0; NET_MTU + 2];
        let len_bytes = (pkt.len() as u16).to_be_bytes();
        prealloc[0] = len_bytes[0];
        prealloc[1] = len_bytes[1];
        for (&src, dst) in pkt.iter().zip(prealloc[2..].iter_mut()) {
            *dst = src;
        }
        let buf = Buffer::into_buf(prealloc).or(Err(xous::Error::InternalError))?;
        buf.send(self.conn, Opcode::WlanSendPacket.to_u32().expect("WlanSendPacket failed"))
            .or(Err(xous::Error::InternalError))?;
        Ok(())
    }

    /// signal strength in -dBm (pre-negated, for "proper" reporting, add a - sign)
    pub fn wlan_get_rssi(&self) -> Result<u8, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::WlanRssi.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't send WlanRssi message");
        if let xous::Result::Scalar1(rssi_usize) = response {
            if rssi_usize & 0xFF_00 != 0 {
                log::error!("got an error code in fetching the RSSI data: 0x{:x}", rssi_usize);
                Err(xous::Error::UnknownError)
            } else {
                // must convert raw code to signal strength here
                let rssi = 110u8 - (rssi_usize & 0xFF) as u8;
                log::debug!("RSSI (lib): -{}dBm", rssi);
                Ok(rssi)
            }
        } else {
            Err(xous::Error::InternalError)
        }
    }

    pub fn wlan_sync_state(&self) -> Result<(LinkState, DhcpState), xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::WlanSyncState.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't send WlanRssi message");
        if let xous::Result::Scalar2(link, dhcp) = response {
            Ok((LinkState::decode_u16(link as u16), DhcpState::decode_u16(dhcp as u16)))
        } else {
            Err(xous::Error::InternalError)
        }
    }

    pub fn wlan_is_reset_hold(&self) -> Result<bool, xous::Error> {
        let status = self.wlan_status()?;
        if status.link_state == LinkState::ResetHold { Ok(true) } else { Ok(false) }
    }

    pub fn ints_enable(&self, int_list: &[ComIntSources]) {
        let mut mask_val: u16 = 0;
        for &item in int_list.iter() {
            let item_as_u16: u16 = item.into();
            mask_val |= item_as_u16;
        }
        let _ = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::IntSetMask.to_usize().unwrap(), mask_val as usize, 0, 0, 0),
        )
        .expect("couldn't send IntSetMask message");
    }

    pub fn ints_get_enabled(&self, int_list: &mut Vec<ComIntSources>) {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::IntGetMask.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("Couldn't get IntGetMask");
        if let xous::Result::Scalar1(raw_mask) = response {
            let mut mask_bit: u16 = 1;
            for _ in 0..16 {
                let int_src = ComIntSources::from(mask_bit & raw_mask as u16);
                if int_src != ComIntSources::Invalid {
                    int_list.push(int_src);
                }
                mask_bit <<= 1;
            }
        } else {
            panic!("failed to send IntGetmask message");
        }
    }

    pub fn ints_ack(&self, int_list: &[ComIntSources]) {
        let mut ack_val: u16 = 0;
        for &item in int_list.iter() {
            let item_as_u16: u16 = item.into();
            ack_val |= item_as_u16;
        }
        let _ = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::IntAck.to_usize().unwrap(), ack_val as usize, 0, 0, 0),
        )
        .expect("couldn't send IntSetMask message");
    }

    pub fn ints_get_active(
        &self,
        int_list: &mut Vec<ComIntSources>,
    ) -> Result<(Option<u16>, usize, usize), xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::IntFetchVector.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't get IntFetchVector");
        let mut rxlen: Option<u16> = None;
        if let xous::Result::Scalar2(ints, maybe_rxlen) = response {
            if ints == 0xDDDD {
                log::warn!("IntFetchVector: 0xDDDD sentinel returned");
                return Err(xous::Error::Timeout);
            }
            let mut mask_bit: u16 = 1;
            for _ in 0..16 {
                let int_src = ComIntSources::from(mask_bit & ints as u16);
                if int_src != ComIntSources::Invalid {
                    int_list.push(int_src);
                    if maybe_rxlen > NET_MTU {
                        log::error!(
                            "got an RX_LEN bigger than NET_MTU: {}, squashing packet; ints vector: 0x{:x?}",
                            maybe_rxlen,
                            ints
                        );
                        rxlen = None;
                    } else {
                        if int_src == ComIntSources::WlanRxReady {
                            rxlen = Some(maybe_rxlen as u16);
                        } else if int_src == ComIntSources::Connect {
                            rxlen = Some(maybe_rxlen as u16);
                        }
                    }
                }
                mask_bit <<= 1;
            }
            Ok((rxlen, ints, maybe_rxlen))
        } else {
            panic!("failed to send IntGetmask message");
        }
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
            xous::send_message(cid, Message::new_scalar(api::Callback::Drop.to_usize().unwrap(), 0, 0, 0, 0))
                .unwrap();
            unsafe {
                xous::disconnect(cid).unwrap();
            }
        }
        // now de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using
        // the connection.
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}
