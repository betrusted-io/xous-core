use crate::*;
use xous::{CID, send_message, Message};
use xous_ipc::Buffer;

use num_traits::*;
use std::io::{Result, Error, ErrorKind};
use std::path::{Path, Component};
use std::format;
use std::io::{Read, Write};

use std::string::String;

pub struct PddbKey<'a> {
    conn: CID,
    dict: String,
    key: String,
    token: ApiToken,
    /// position in the key's data "stream"
    pos: u64,
    buf: Buffer<'a>,
}
impl<'a> PddbKey<'a> {
    pub fn get<P: AsRef<Path>>(path: P) -> Result<PddbKey<'a>> {
        let xns = xous_names::XousNames::new().unwrap();
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_PDDB).expect("Can't connect to Pddb server");

        if !path.as_ref().is_absolute() {
            return Err(Error::new(ErrorKind::InvalidInput, "All PDDB keys must be fully specified relative to a dictionary"));
        }
        let mut dict = String::new();
        let mut key = String::new();
        let mut components = path.as_ref().components();
        match components.next().unwrap() {
            Component::Prefix(prefix_component) => {
                if let Some(dictstr) = prefix_component.as_os_str().to_str() {
                    if dictstr.len() <= DICT_NAME_LEN {
                        dict.push_str(dictstr);
                    } else {
                        return Err(Error::new(ErrorKind::InvalidInput, format!("PDDB dictionary names must be shorter than {} bytes", DICT_NAME_LEN)));
                    }
                } else {
                    return Err(Error::new(ErrorKind::InvalidInput, "PDDB dictionary names must valid UTF-8"));
                }
            }
            _ => {
                return Err(Error::new(ErrorKind::InvalidInput, "All PDDB entries must be of the format `dict:key`, where `dict` is treated as a Prefix"));
            }
        }
        // collect the remaining components into the key
        for comps in components {
            if let Some(keystr) = comps.as_os_str().to_str() {
                key.push_str(keystr);
            } else {
                return Err(Error::new(ErrorKind::InvalidInput, "PDDB dictionary names must valid UTF-8"));
            }
        }

        if key.len() > KEY_NAME_LEN {
            return Err(Error::new(ErrorKind::InvalidInput, format!("PDDB key names must be shorter than {} bytes", DICT_NAME_LEN)));
        }

        let request = PddbKeyRequest {
            dict: xous_ipc::String::<DICT_NAME_LEN>::from_str(dict.as_str()),
            key: xous_ipc::String::<KEY_NAME_LEN>::from_str(key.as_str()),
            token: None,
        };
        let mut buf = Buffer::into_buf(request)
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
        buf.lend_mut(conn, Opcode::KeyRequest.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;

        let response = buf.to_original::<PddbKeyRequest, _>().unwrap();

        // we probably should never remove this check -- the code may compile correctly and
        // "work" without this being an even page size, but it's pretty easy to get this wrong,
        // and if it's wrong we can lose a lot in terms of efficiency of execution.
        assert!(core::mem::size_of::<PddbBuf>() == 4096, "PddBuf record has the wrong size");
        if let Some(token) = response.token {
            Ok(PddbKey {
                conn,
                dict,
                key,
                token,
                pos: 0,
                buf: Buffer::new(core::mem::size_of::<PddbBuf>()),
            })
        } else {
            Err(Error::new(ErrorKind::PermissionDenied, "Dict/Key access denied"))
        }
    }
    /// this will clear all residual values in the buffer. Should be called whenever the Basis set changes.
    pub fn volatile_clear(&mut self) {
        self.buf.volatile_clear();
    }

    pub(crate) fn conn(&self) -> CID {
        self.conn
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
            // leaking that data back to a server where the data had either originated from or was disclosed at
            // one point.
            let readlen = {
                let pbuf = PddbBuf::from_slice_mut(self.buf.as_mut());
                // sure, we could make it a loop, but...unrolled seems better
                pbuf.token[0] = self.token[0];
                pbuf.token[1] = self.token[1];
                pbuf.token[2] = self.token[2];
                let readlen = if buf.len() <= pbuf.data.len() {
                    buf.len() as u16
                } else {
                    pbuf.data.len() as u16
                };
                pbuf.len = readlen;
                pbuf.retcode = PddbRetcode::Uninit;
                pbuf.position = self.pos;
                readlen
            };
            // this takes the buffer and remaps it to the server, and on return the data is mapped back
            self.buf.lend_mut(self.conn, Opcode::ReadKey.to_u32().unwrap())
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
                        self.pos += buf.len() as u64;
                        Ok(pbuf.len as usize)
                    }
                    PddbRetcode::BasisLost => Err(Error::new(ErrorKind::BrokenPipe, "Basis lost")),
                    PddbRetcode::AccessDenied => Err(Error::new(ErrorKind::PermissionDenied, "Access denied")),
                    _ => Err(Error::new(ErrorKind::Other, "Unhandled error code in PddbKey Read")),
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
                let writelen = if buf.len() <= pbuf.data.len() {
                    buf.len() as u16
                } else {
                    pbuf.data.len() as u16
                };
                pbuf.len = writelen;
                pbuf.retcode = PddbRetcode::Uninit;
                for (&src, dst) in buf.iter().zip(pbuf.data.iter_mut()) {
                    *dst = src;
                }
                pbuf.position = self.pos;
                writelen
            };
            // this takes the buffer and remaps it to the server, and on return the data is mapped back
            self.buf.lend_mut(self.conn, Opcode::WriteKey.to_u32().unwrap())
                .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
            {
                // at this point, pbuf has been mutated by the server with a return code and the return data.
                let pbuf = PddbBuf::from_slice_mut(self.buf.as_mut());
                match pbuf.retcode {
                    PddbRetcode::Ok => {
                        assert!(pbuf.len <= writelen, "More data written than we requested");
                        self.pos += writelen as u64;
                        Ok(pbuf.len as usize)
                    }
                    PddbRetcode::BasisLost => Err(Error::new(ErrorKind::BrokenPipe, "Basis lost")),
                    PddbRetcode::AccessDenied => Err(Error::new(ErrorKind::PermissionDenied, "Access denied")),
                    _ => Err(Error::new(ErrorKind::Other, "Unhandled error code in PddbKey Read")),
                }
            }
        }
    }
    fn flush(&mut self) -> Result<()> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::WriteKeyFlush.to_usize().unwrap(), 0, 0, 0, 0)
        ).or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
        if let xous::Result::Scalar1(rcode) = response {
            match FromPrimitive::from_u8(rcode as u8) {
                Some(PddbRetcode::Ok) => Ok(()),
                Some(PddbRetcode::BasisLost) => Err(Error::new(ErrorKind::BrokenPipe, "Basis lost")),
                _ => Err(Error::new(ErrorKind::Interrupted, "Flush failed for unspecified reasons")),
            }
        } else {
            Err(Error::new(ErrorKind::Other, "Xous internal error"))
        }
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl<'a> Drop for PddbKey<'a> {
    fn drop(&mut self) {
        self.buf.volatile_clear(); // clears any confidential data in our memory buffer

        // the connection to the server side must be reference counted, so that multiple instances of this object within
        // a single process do not end up de-allocating the CID on other threads before they go out of scope.
        // Note to future me: you want this. Don't get rid of it because you think, "nah, nobody will ever make more than one copy of this object".
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) - 1, Ordering::Relaxed);
        if REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
        // if there was object-specific state (such as a one-time use server for async callbacks, specific to the object instance),
        // de-allocate those items here. They don't need a reference count because they are object-specific
    }
}

