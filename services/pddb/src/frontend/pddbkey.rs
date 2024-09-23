use std::io::{Error, ErrorKind, Result};
use std::io::{Read, Seek, SeekFrom, Write};

use num_traits::*;
use xous::{CID, Message, send_message};
use xous_ipc::Buffer;

use crate::*;

pub struct PddbKey<'a> {
    pub(crate) token: ApiToken,
    /// position in the key's data "stream"
    pub(crate) pos: u64,
    pub(crate) buf: Buffer<'a>,
    pub(crate) conn: CID,
}
/// PddbKeys are created by Pddb
impl<'a> PddbKey<'a> {
    /// this will clear all residual values in the buffer. Should be called whenever the Basis set changes.
    pub fn volatile_clear(&mut self) { self.buf.volatile_clear(); }

    pub fn attributes(&self) -> Result<KeyAttributes> {
        let req = PddbKeyAttrIpc::new(self.token);
        let mut buf = Buffer::into_buf(req).expect("Couldn't convert memory structure");
        buf.lend_mut(self.conn, Opcode::KeyAttributes.to_u32().unwrap())
            .expect("couldn't execute KeyAttributes opcode");
        let ret = buf.to_original::<PddbKeyAttrIpc, _>().expect("couldn't restore req structure");
        match ret.code {
            PddbRequestCode::NoErr => Ok(ret.to_attributes()),
            PddbRequestCode::NotFound => Err(Error::new(ErrorKind::NotFound, "Key not found")),
            _ => Err(Error::new(ErrorKind::Other, "Internal error requesting key attributes")),
        }
    }
}

impl<'a> Seek for PddbKey<'a> {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        match pos {
            SeekFrom::Start(p) => {
                self.pos = p;
                Ok(p)
            }
            SeekFrom::Current(p) => {
                if self.pos as i64 + p >= 0 {
                    self.pos = (self.pos as i64 + p) as u64;
                    Ok(self.pos)
                } else {
                    Err(Error::new(ErrorKind::InvalidInput, "Seek to negative offset"))
                }
            }
            SeekFrom::End(p) => {
                let req = PddbKeyAttrIpc::new(self.token);
                let mut buf = Buffer::into_buf(req).expect("Couldn't convert memory structure");
                buf.lend_mut(self.conn, Opcode::KeyAttributes.to_u32().unwrap())
                    .expect("couldn't execute KeyAttributes opcode");
                let ret = buf.to_original::<PddbKeyAttrIpc, _>().expect("couldn't restore req structure");
                match ret.code {
                    PddbRequestCode::NoErr => {
                        let len = ret.to_attributes().len as i64;
                        if len + p >= 0 {
                            self.pos = (len + p) as u64;
                            Ok(self.pos)
                        } else {
                            Err(Error::new(ErrorKind::InvalidInput, "Seek to negative offset"))
                        }
                    }
                    PddbRequestCode::NotFound => Err(Error::new(ErrorKind::NotFound, "Key not found")),
                    _ => Err(Error::new(ErrorKind::Other, "Internal error requesting key attributes")),
                }
            }
        }
    }
}

impl<'a> Read for PddbKey<'a> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if buf.len() == 0 {
            Ok(0)
        } else {
            // create pbuf from a pre-reserved chunk of memory, to save on allocator thrashing
            // note that it does mean that un-erased data from previous reads and writes are passed back
            // to the server, which is a kind of information leakage, but I think in practice we're
            // leaking that data back to a server where the data had either originated from or was disclosed
            // at one point.
            let readlen = {
                let pbuf = PddbBuf::from_slice_mut(self.buf.as_mut());
                // sure, we could make it a loop, but...unrolled seems better
                pbuf.token[0] = self.token[0];
                pbuf.token[1] = self.token[1];
                pbuf.token[2] = self.token[2];
                let readlen =
                    if buf.len() <= pbuf.data.len() { buf.len() as u16 } else { pbuf.data.len() as u16 };
                pbuf.len = readlen;
                pbuf.retcode = PddbRetcode::Uninit;
                pbuf.position = self.pos;
                readlen
            };
            // this takes the buffer and remaps it to the server, and on return the data is mapped back
            self.buf
                .lend_mut(self.conn, Opcode::ReadKey.to_u32().unwrap())
                .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
            {
                // at this point, pbuf has been mutated by the server with a return code and the return data.
                let pbuf = PddbBuf::from_slice_mut(self.buf.as_mut());
                match pbuf.retcode {
                    PddbRetcode::Ok => {
                        for (&src, dst) in pbuf.data.iter().zip(buf.iter_mut()) {
                            *dst = src;
                        }
                        assert!(pbuf.len <= readlen, "More data returned than we requested");
                        self.pos += pbuf.len as u64;
                        Ok(pbuf.len as usize)
                    }
                    PddbRetcode::BasisLost => Err(Error::new(ErrorKind::BrokenPipe, "Basis lost")),
                    PddbRetcode::AccessDenied => {
                        Err(Error::new(ErrorKind::PermissionDenied, "Access denied"))
                    }
                    PddbRetcode::UnexpectedEof => Ok(0), /* I believe this is the "expected" behavior for */
                    // reads that want to read beyond the current end
                    // of file
                    _ => {
                        log::error!("Unhandled error code: {:?}", pbuf.retcode);
                        Err(Error::new(ErrorKind::Other, "Unhandled error code in PddbKey Read"))
                    }
                }
            }
        }
    }
}

impl<'a> Write for PddbKey<'a> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        if buf.len() == 0 {
            Ok(0)
        } else {
            let writelen = {
                let pbuf = PddbBuf::from_slice_mut(self.buf.as_mut());
                // sure, we could make it a loop, but...unrolled seems better
                pbuf.token[0] = self.token[0];
                pbuf.token[1] = self.token[1];
                pbuf.token[2] = self.token[2];
                let writelen =
                    if buf.len() <= pbuf.data.len() { buf.len() as u16 } else { pbuf.data.len() as u16 };
                pbuf.len = writelen;
                pbuf.retcode = PddbRetcode::Uninit;
                for (&src, dst) in buf.iter().zip(pbuf.data.iter_mut()) {
                    *dst = src;
                }
                pbuf.position = self.pos;
                writelen
            };
            // this takes the buffer and remaps it to the server, and on return the data is mapped back
            self.buf
                .lend_mut(self.conn, Opcode::WriteKey.to_u32().unwrap())
                .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
            {
                // at this point, pbuf has been mutated by the server with a return code and the return data.
                let pbuf = PddbBuf::from_slice_mut(self.buf.as_mut());
                match pbuf.retcode {
                    PddbRetcode::Ok => {
                        assert!(pbuf.len <= writelen, "More data written than we requested");
                        self.pos += pbuf.len as u64;
                        Ok(pbuf.len as usize)
                    }
                    PddbRetcode::BasisLost => Err(Error::new(ErrorKind::BrokenPipe, "Basis lost")),
                    PddbRetcode::AccessDenied => {
                        Err(Error::new(ErrorKind::PermissionDenied, "Access denied"))
                    }
                    _ => Err(Error::new(ErrorKind::Other, "Unhandled error code in PddbKey Write")),
                }
            }
        }
    }

    fn flush(&mut self) -> Result<()> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::WriteKeyFlush.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
        if let xous::Result::Scalar1(rcode) = response {
            match FromPrimitive::from_u8(rcode as u8) {
                Some(PddbRetcode::Ok) => Ok(()),
                Some(PddbRetcode::BasisLost) => Err(Error::new(ErrorKind::BrokenPipe, "Basis lost")),
                Some(PddbRetcode::DiskFull) => {
                    Err(Error::new(ErrorKind::OutOfMemory, "Out of disk space, some data lost on sync"))
                }
                _ => Err(Error::new(ErrorKind::Interrupted, "Flush failed for unspecified reasons")),
            }
        } else {
            Err(Error::new(ErrorKind::Other, "Xous internal error"))
        }
    }
}

use core::sync::atomic::Ordering;
impl<'a> Drop for PddbKey<'a> {
    fn drop(&mut self) {
        self.buf.volatile_clear(); // clears any confidential data in our memory buffer

        // notify the server that we can drop the connection state when our object goes out of scope
        send_message(
            self.conn,
            Message::new_blocking_scalar(
                Opcode::KeyDrop.to_usize().unwrap(),
                self.token[0] as usize,
                self.token[1] as usize,
                self.token[2] as usize,
                0,
            ),
        )
        .expect("couldn't send KeyDrop message");

        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}
