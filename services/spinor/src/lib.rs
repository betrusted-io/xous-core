#![cfg_attr(target_os = "none", no_std)]

pub mod api;
pub use api::*;

use xous::{CID, send_message, Message};
use num_traits::ToPrimitive;

pub struct Spinor {
    conn: CID,
    token: [u32; 4],
}
impl Spinor {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_SPINOR).expect("Can't connect to Spinor server");

        let trng = trng::Trng::new(&xns).expect("Can't connect to TRNG servere");
        Ok(Spinor {
            conn,
            token: [
                trng.get_u32().unwrap(),
                trng.get_u32().unwrap(),
                trng.get_u32().unwrap(),
                trng.get_u32().unwrap(),
            ],
        })
    }

    /// this returns the minimum alignment for an erase block. `write` and `erase` operations
    /// benefit in performance if the requests are aligned to this number.
    pub fn erase_alignment(&self) -> u32 {
        0x1000
    }

    // convenience function to wrap the IPC mess into a single function, but it makes some key assumptions:
    // - caller guarantees that the length of data is less than 4096 bytes
    // - caller also guarantees we have the exclusive lock to do this op
    fn write_page(&mut self, data: &[u8], start_addr: u32) -> Result<(), SpinorError> {
        assert!(data.len() <= 4096, "Assumption of write_page() helper was violated by library code.");

        let mut wr = WriteRegion {
            id: self.token,
            start: start_addr,
            autoerase: true,
            data: [0; 4096],
            len: data.len() as u32,
            result: None,
        };
        for (&src, dst) in data.iter().zip(wr.data.iter_mut()) {
            *dst = src;
        }
        let mut buf = Buffer::into_buf(wr).or(Err(SpinorError::IpcError))?;
        buf.lend_mut(self.conn, Opcode::WriteRegion.to_u32().unwrap()).or(Err(SpinorError::IpcError))?;

        match buf.to_original() {
            Some(wr) => {
                if let Some(res) = wr.result {
                    match res {
                        SpinorError::NoError => Ok(()),
                        _ => Err(res)
                    }
                } else {
                    SpinorError::ImplementationError
                }
            }
            _ => SpinorError::ImplementationError
        }
    }

    /// note: this implementation will write precisely the slice of u8 contained
    /// in data starting from start_addr. If the request is not aligned, the
    /// operation is "expensive" in that it will make a copy of the misaligned edges,
    /// erase the entire sectors underneath, and then re-write the data that was adjacent
    /// to the write data
    pub fn write(&mut self, data: &[u8], start_addr: u32) -> Result<(), SpinorError> {
        // acquire a write lock on the unit
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::AcquireExclusive.to_usize().unwrap(),
                self.token[0] as usize,
                self.token[1] as usize,
                self.token[2] as usize,
                self.token[3] as usize,
            )
        ).expect("couldn't send AcquireExclusive message to Sha2 hardware!");
        if let xous::Result::Scalar1(result) = response {
            if result == 0 {
                return Err(SpinorError::BusyTryAgain)
            }
        }

        let align_mask = self.erase_alignment() - 1;

        let mut ret: Result<(), SpinorError> = Ok(());
        let mut req_addr = start_addr;
        if req_addr & align_mask != 0 {
            let u8_to_alignment = self.erase_alignment() - (req_addr & align_mask);
            if u8_to_alignment < self.erase_alignment() {
                // can't align anything, data is smaller than a page
                ret = write_page(data, req_addr);
            } else {
                // issue one mis-aligned request first; this will trigger a partial erase and re-write
                write_page(data[0..u8_to_alignment as usize], req_addr)?;
                req_addr += u8_to_alignment;
                // now send the rest as aligned; this is much more efficient than streaming a bunch of mis-aligned pages
                for page in &data[u8_to_alignment as usize..].chunks(self.erase_alignment() as usize) {
                    write_data(page, req_addr)?;
                    req_addr += self.erase_alignment();
                }
            }
        } else {
            // the request is aligned, just issue it; the last page, if mis-aligned, might be a little ugly, but nothing we can do about it.
            for page in data.chunks(self.erase_alignment() as usize) {
                write_data(page, req_addr)?;
                req_addr += self.erase_alignment();
            }
        }

        // release the write lock before exiting
        let _ = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::ReleaseExclusive, 0, 0, 0, 0)
        ).expect("couldn't send ReleaseExclusive message");
        ret
    }

    /// note: this implementation will erase precisely the number of u8 starting
    /// at start_addr; if this is not naturally aligned to an erase block, the operation
    /// is "expensive" in that it will make a copy of the misaligned sector, erase the
    /// sector, and then re-write the data that was not meant to be erased!
    pub fn erase(&mut self, start_addr: u32, num_u8: u32) -> SpinorResult {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::Erase.to_usize().unwrap(), start_addr as usize, num_u8 as usize, 0, 0)
            ).expect("Couldn't send erase command");
        if let xous::Result::Scalar1(result) = response {
            match FromPrimitive::from_usize(result) {
                Some(r) => r,
                None => {
                    log::error!("Couldn't transform return enum: {:?}", result);
                    SpinorResult::InternalError
                },
            }
        } else {
            log::error!("unexpected return structure: {:#?}", response);
            SpinorResult::InternalError
        }
    }

    /// these functions are intended for use by the suspend/resume manager. most functions wouldn't have a need to call this.
    pub fn acquire_suspend_lock(&self) -> Result<bool, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::AcquireSuspendLock.to_usize().unwrap(), 0, 0, 0, 0)
        ).expect("Couldn't issue AcquireSuspendLock message");
        if let xous::Result::Scalar1(result) = response {
            if result != 0 {
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Err(xous::Error::InternalError)
        }
    }
    pub fn release_suspend_lock(&self) -> Result<(), xous::Error> {
        // we ignore the result and just turn it into () once we get anything back, as release_suspend "can't fail"
        send_message(self.conn,
            Message::new_blocking_scalar(Opcode::ReleaseSuspendLock.to_usize().unwrap(), 0, 0, 0, 0)
        ).map(|_| ())
    }
}

use core::{sync::atomic::{AtomicU32, Ordering}, u8};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Spinor {
    fn drop(&mut self) {
        // the connection to the server side must be reference counted, so that multiple instances of this object within
        // a single process do not end up de-allocating the CID on other threads before they go out of scope.
        // Note to future me: you want this. Don't get rid of it because you think, "nah, nobody will ever make more than one copy of this object".
        if REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
        // if there was object-specific state (such as a one-time use server for async callbacks, specific to the object instance),
        // de-allocate those items here. They don't need a reference count because they are object-specific
    }
}