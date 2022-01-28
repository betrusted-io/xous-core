use std::convert::TryInto;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};
use std::unimplemented;
use std::io;
use std::io::{Error, ErrorKind, Result};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;

use smoltcp::wire::{Ipv4Address, Ipv6Address, IpAddress};
use smoltcp::{
    wire::IpEndpoint,
    time::Duration,
};

use xous::{CID, Message, SID, msg_blocking_scalar_unpack, send_message};
use xous_ipc::Buffer;
use crate::NetConn;
use crate::api::*;
//use crate::api::udp::*;
use num_traits::*;

use std::io::{IoSliceMut, IoSlice};
use std::net::Shutdown;
use std::fmt;

// used by both clients and servers. implement this first.
pub struct TcpStream {
    net: NetConn,
    cb_sid: SID,
    socket_addr: SocketAddr,
    nonblocking: bool,
}

impl TcpStream {
    pub fn connect(maybe_socket: io::Result<&SocketAddr>) -> io::Result<TcpStream> {
        TcpStream::connect_inner(maybe_socket)
    }

    pub fn connect_xous<A: ToSocketAddrs>(addr: A) -> Result<TcpStream> {
        match addr.to_socket_addrs() {
            Ok(socks) => {
                match socks.into_iter().next() {
                    Some(socket_addr) => {
                        TcpStream::connect_inner(
                            Ok(&socket_addr),
                        )
                    }
                    _ => Err(Error::new(ErrorKind::InvalidInput, "IP address invalid"))
                }
            }
            _ => Err(Error::new(ErrorKind::InvalidInput, "IP address invalid"))
        }
    }

    pub fn connect_inner(maybe_socket: io::Result<&SocketAddr>) -> io::Result<TcpStream> {
        let socket = maybe_socket?;
        let xns = xous_names::XousNames::new().unwrap();
        let net = NetConn::new(&xns).unwrap();
        let cb_sid = xous::create_server().unwrap();

        unimplemented!()
    }



    pub fn connect_timeout(_: &SocketAddr, _: Duration) -> io::Result<TcpStream> {
        unimplemented!()
    }

    pub fn set_read_timeout(&self, _: Option<Duration>) -> io::Result<()> {
        unimplemented!()
    }

    pub fn set_write_timeout(&self, _: Option<Duration>) -> io::Result<()> {
        unimplemented!()
    }

    pub fn read_timeout(&self) -> io::Result<Option<Duration>> {
        unimplemented!()
    }

    pub fn write_timeout(&self) -> io::Result<Option<Duration>> {
        unimplemented!()
    }

    pub fn peek(&self, _: &mut [u8]) -> io::Result<usize> {
        unimplemented!()
    }

    pub fn read(&self, _: &mut [u8]) -> io::Result<usize> {
        unimplemented!()
    }

    pub fn read_vectored(&self, _: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        unimplemented!()
    }

    pub fn is_read_vectored(&self) -> bool {
        unimplemented!()
    }

    pub fn write(&self, _: &[u8]) -> io::Result<usize> {
        unimplemented!()
    }

    pub fn write_vectored(&self, _: &[IoSlice<'_>]) -> io::Result<usize> {
        unimplemented!()
    }

    pub fn is_write_vectored(&self) -> bool {
        unimplemented!()
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        unimplemented!()
    }

    pub fn socket_addr(&self) -> io::Result<SocketAddr> {
        unimplemented!()
    }

    pub fn shutdown(&self, _: Shutdown) -> io::Result<()> {
        unimplemented!()
    }

    pub fn duplicate(&self) -> io::Result<TcpStream> {
        unimplemented!()
    }

    pub fn set_linger(&self, _: Option<Duration>) -> io::Result<()> {
        unimplemented!()
    }

    pub fn linger(&self) -> io::Result<Option<Duration>> {
        unimplemented!()
    }

    pub fn set_nodelay(&self, _: bool) -> io::Result<()> {
        unimplemented!()
    }

    pub fn nodelay(&self) -> io::Result<bool> {
        unimplemented!()
    }

    pub fn set_ttl(&self, _: u32) -> io::Result<()> {
        unimplemented!()
    }

    pub fn ttl(&self) -> io::Result<u32> {
        unimplemented!()
    }

    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        unimplemented!()
    }

    pub fn set_nonblocking(&self, _: bool) -> io::Result<()> {
        unimplemented!()
    }
}

impl fmt::Debug for TcpStream {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        unimplemented!()
    }
}
/*
impl Read for TcpStream {
    
}

impl Write for TcpStream {


}
*/