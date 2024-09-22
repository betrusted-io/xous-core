#![cfg_attr(target_os = "none", no_std)]

pub mod api;
pub use api::*;
use num_traits::*;
use xous::{CID, Message, send_message};
use xous_ipc::Buffer;

#[derive(Debug)]
pub struct Jtag {
    conn: CID,
}
impl Jtag {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn =
            xns.request_connection_blocking(api::SERVER_NAME_JTAG).expect("Can't connect to JTAG server");
        Ok(Jtag { conn })
    }

    pub fn get_id(&self) -> Result<u32, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::GetId.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("can't issue get_id message");
        if let xous::Result::Scalar1(id) = response { Ok(id as u32) } else { Err(xous::Error::InternalError) }
    }

    pub fn get_dna(&self) -> Result<u64, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::GetDna.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("can't issue get_id message");
        if let xous::Result::Scalar2(v1, v2) = response {
            Ok(((v1 as u64) << 32) | v2 as u64)
        } else {
            Err(xous::Error::InternalError)
        }
    }

    pub fn efuse_fetch(&self) -> Result<EfuseRecord, xous::Error> {
        let xmit = EfuseRecord { key: [0; 32], user: 0, cntl: 0 };
        let mut buf = Buffer::into_buf(xmit).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::EfuseFetch.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;

        buf.to_original::<EfuseRecord, _>().or(Err(xous::Error::InternalError))
    }

    pub fn write_ir(&self, ir: u8) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(Opcode::WriteIr.to_usize().unwrap(), ir as usize, 0, 0, 0),
        )
        .map(|_| ())
    }

    pub fn efuse_key_burn(&self, key: [u8; 32]) -> Result<bool, xous::Error> {
        let mut buf = Buffer::into_buf(key).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::EfuseKeyBurn.to_u32().unwrap())
            .or(Err(xous::Error::InternalError))?;
        match buf.to_original().unwrap() {
            EfuseResult::Success => Ok(true),
            EfuseResult::Failure => Ok(false),
        }
    }

    pub fn efuse_user_burn(&self, user: u32) -> Result<bool, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::EfuseUserBurn.to_usize().unwrap(), user as usize, 0, 0, 0),
        )
        .expect("can't fetch user value");
        if let xous::Result::Scalar1(res) = response {
            if res == 0 { Ok(false) } else { Ok(true) }
        } else {
            Err(xous::Error::InternalError)
        }
    }

    pub fn is_efuse_only_boot(&self) -> Result<bool, xous::Error> {
        let erec = self.efuse_fetch().expect("couldn't read efuse record");
        if (erec.cntl & 0x1) != 0 { Ok(true) } else { Ok(false) }
    }

    pub fn is_disable_jtag(&self) -> Result<bool, xous::Error> {
        let erec = self.efuse_fetch().expect("couldn't read efuse record");
        if (erec.cntl & 0x2) != 0 { Ok(true) } else { Ok(false) }
    }

    pub fn is_efuse_writeable(&self) -> Result<bool, xous::Error> {
        let erec = self.efuse_fetch().expect("couldn't read efuse record");
        if (erec.cntl & 0x4) != 0 { Ok(true) } else { Ok(false) }
    }

    pub fn is_key_access_protected(&self) -> Result<bool, xous::Error> {
        let erec = self.efuse_fetch().expect("couldn't read efuse record");
        if (erec.cntl & 0x8) != 0 { Ok(true) } else { Ok(false) }
    }

    pub fn is_user_and_key_access_protected(&self) -> Result<bool, xous::Error> {
        let erec = self.efuse_fetch().expect("couldn't read efuse record");
        if (erec.cntl & 0x10) != 0 { Ok(true) } else { Ok(false) }
    }

    pub fn is_control_write_protected(&self) -> Result<bool, xous::Error> {
        let erec = self.efuse_fetch().expect("couldn't read efuse record");
        if (erec.cntl & 0x20) != 0 { Ok(true) } else { Ok(false) }
    }

    pub fn get_raw_control_bits(&self) -> Result<u8, xous::Error> {
        let erec = self.efuse_fetch().expect("couldn't read efuse record");
        Ok(erec.cntl)
    }

    fn mod_ctl_efuse(&self, bits_to_set: u8) -> Result<bool, xous::Error> {
        let erec = self.efuse_fetch().expect("couldn't read efuse record");
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(
                Opcode::EfuseCtlBurn.to_usize().unwrap(),
                (erec.cntl | bits_to_set) as usize,
                0,
                0,
                0,
            ),
        )
        .expect("can't issue config burn message");
        if let xous::Result::Scalar1(r) = response {
            if r != 0 { Ok(true) } else { Ok(false) }
        } else {
            Err(xous::Error::InternalError)
        }
    }

    pub fn burn_efuse_only_boot(&self) -> Result<bool, xous::Error> { self.mod_ctl_efuse(0x1) }

    pub fn burn_disable_jtag(&self) -> Result<bool, xous::Error> { self.mod_ctl_efuse(0x2) }

    pub fn burn_efuse_write_protect(&self) -> Result<bool, xous::Error> { self.mod_ctl_efuse(0x4) }

    pub fn burn_key_access_protect(&self) -> Result<bool, xous::Error> { self.mod_ctl_efuse(0x8) }

    pub fn burn_user_and_key_access_protect(&self) -> Result<bool, xous::Error> { self.mod_ctl_efuse(0x10) }

    pub fn burn_control_write_protect(&self) -> Result<bool, xous::Error> { self.mod_ctl_efuse(0x20) }

    /// Sets all the fuses, except `disable_jtag` and `control_write_protect`.
    /// This forces the device to:
    ///   - only boot from an encrypted image (so unbricking via JTAG requires you to provide an encryption
    ///     key)
    ///   - key update disallowed
    ///   - key readback disallowed
    ///   - user bits can be read out (but they can't be updated)
    ///   - control bits can still be blown (so you can still set `disable_jtag` later on; however, it is
    ///     physically impossible to unset bits, so it is not possible to enable key readback despite being
    ///     able to write bits)
    pub fn seal_device(&self) -> Result<bool, xous::Error> { self.mod_ctl_efuse(0b00_1101) }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Jtag {
    fn drop(&mut self) {
        // the connection to the server side must be reference counted, so that multiple instances of this
        // object within a single process do not end up de-allocating the CID on other threads before
        // they go out of scope. Note to future me: you want this. Don't get rid of it because you
        // think, "nah, nobody will ever make more than one copy of this object".
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
        // if there was object-specific state (such as a one-time use server for async callbacks, specific to
        // the object instance), de-allocate those items here. They don't need a reference count
        // because they are object-specific
    }
}
