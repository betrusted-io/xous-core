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
    local_port: u16,
    ticktimer: ticktimer_server::Ticktimer,

    rx_buf: Arc::<Mutex::<Vec::<u8>>>,
    rx_handle: Option<JoinHandle::<()>>,
    /// xous-specific feature that allows for more efficent Rx than blocking. Caller must set_scalar_notification() to use it.
    rx_notify: Arc<Mutex<XousScalarEndpoint>>,

    nonblocking: bool,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    rx_shutdown: bool,
    tx_shutdown: bool,
}

impl TcpStream {
    pub fn connect(maybe_socket: io::Result<&SocketAddr>) -> io::Result<TcpStream> {
        TcpStream::connect_inner(maybe_socket, None, None)
    }
    pub fn connect_timeout(maybe_socket: &SocketAddr, duration: Duration) -> io::Result<TcpStream> {
        TcpStream::connect_inner(maybe_socket, duration, None)
    }

    /// This API call uses the stdlib-internal calling convention to assist with the process of migrating this into a true libstd
    pub fn connect_xous<A: ToSocketAddrs>(addr: A, timeout: Option<Duration>, keepalive: Option<Duration>) -> Result<TcpStream> {
        match addr.to_socket_addrs() {
            Ok(socks) => {
                match socks.into_iter().next() {
                    Some(socket_addr) => {
                        TcpStream::connect_inner(
                            Ok(&socket_addr),
                            timeout,
                            keepalive
                        )
                    }
                    _ => Err(Error::new(ErrorKind::InvalidInput, "IP address invalid"))
                }
            }
            _ => Err(Error::new(ErrorKind::InvalidInput, "IP address invalid"))
        }
    }
    /// note that there is no provision in the rust stdlib to set a keepalive, so it's sort of wedged in here
    pub fn connect_inner(
        maybe_socket: io::Result<&SocketAddr>,
        timeout: Option<Duration>,
        keepalive: Option<Duration>) -> io::Result<TcpStream> {
        let socket = maybe_socket?;
        let xns = xous_names::XousNames::new().unwrap();
        let net = NetConn::new(&xns).unwrap();
        let cb_sid = xous::create_server().unwrap();
        let ticktimer = ticktimer_server::Ticktimer::new().unwrap();

        let rx_buf = Arc::new(Mutex::new(Vec::new()));
        let notify = Arc::new(Mutex::new(XousScalarEndpoint::new()));

        let handle = thread::spawn({
            let cb_sid_clone = cb_sid.clone();
            let rx_buf = Arc::clone(&rx_buf);
            let notify = Arc::clone(&notify);
            move || {
                loop {
                    let msg = xous::receive_message(cb_sid_clone).unwrap();
                    match FromPrimitive::from_usize(msg.body.id()) {
                        Some(NetTcpCallback::RxData) => {
                            let buffer = unsafe {Buffer::from_memory_message(msg.body.memory_message().unwrap())};
                            let incoming = buffer.as_flat::<NetTcpResponse, _>().unwrap();
                            { // grab the lock once for better efficiency
                                let mut rx_locked = rx_buf.lock().unwrap();
                                for &d in incoming.data[..incoming.len as unsize].iter() {
                                    rx_locked.push(d);
                                }
                            }
                            notify.lock().unwrap().notify(); // this will only notify if a destination has been set
                        }
                        Some(NetTcpCallback::Drop) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                            log::debug!("Drop received, exiting Tcp handler");
                            xous::return_scalar(msg.sender, 1).unwrap();
                            break;
                        }),
                        None => {
                            log::error!("got unknown message type on Tcp callback: {:?}", msg);
                        }
                    }
                }
            }
        });

        let request = NetTcpManage {
            cb_sid,
            ip_addr: NetPiAddr::from(socket),
            remote_port: socket.port(),
            local_port: None,
            timeout_ms: if let Some(d) = timeout { Some(d.total_millis()) } else { None },
            keepalive_ms: if let Some(d) = keepalive { Some(d.total_millis()) } else { None },
            result: None,
            rx_shutdown: None,
        };
        let mut buf = Buffer::into_buf(request)
            .or(Err(Error::new(ErrorKind::Other, "can't register with Net server")))?;
        buf.lend_mut(net.conn(), Opcode::TcpConnect.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "can't register with Net server")))?;
        let ret = buf.to_original::<NetTcpManage, _>().unwrap();
        match ret.result {
            Some(NetMemResponse::Ok) => {
                if let Some(local_port) = ret.local_port {
                    Ok(TcpSocket {
                        net,
                        cb_sid,
                        socket_addr: *socket,
                        local_port,
                        rx_buf,
                        handle: Some(handle),
                        rx_notify: notify,
                        read_timeout: None,
                        write_timeout: None,
                        ticktimer,
                        dest_socket: None,
                        max_payload,
                        nonblocking: false,
                        rx_shutdown: false,
                        tx_shutdown: false,
                    })
                } else {
                    Err(Error::new(ErrorKind::Other, "Net server failed to assign us a local port"))
                }
            },
            _ => {
                Err(Error::new(ErrorKind::Other, "can't register with Net server"))
            }
        }
    }
    pub fn set_scalar_notification(&mut self, cid: CID, op: usize, args: [Option<usize>; 4]) {
        self.rx_notify.lock().unwrap().set(cid, op, args);
    }
    pub fn clear_scalar_notification(&mut self) {
        self.rx_notify.lock().unwrap().clear();
    }

    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        if let Some(duration) = timeout {
            if duration.total_millis() == 0 {
                Err(Error::new(ErrorKind::InvalidInput, "zero duration is not valid"))
            } else {
                self.read_timeout = Some(duration);
                Ok(())
            }
        } else {
            self.read_timeout = None;
            Ok(())
        }
    }
    pub fn set_write_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        if let Some(duration) = timeout {
            if duration.total_millis() == 0 {
                Err(Error::new(ErrorKind::InvalidInput, "zero duration is not valid"))
            } else {
                self.write_timeout = Some(duration);
                Ok(())
            }
        } else {
            self.write_timeout = None;
            Ok(())
        }
    }
    pub fn read_timeout(&self) -> io::Result<Option<Duration>> {
        Ok(self.read_timeout)
    }
    pub fn write_timeout(&self) -> io::Result<Option<Duration>> {
        Ok(self.write_timeout)
    }
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.socket_addr)
    }
    pub fn socket_addr(&self) -> io::Result<SocketAddr> {
        Ok(SocketAddr::new(
            IpAddr::V4(127, 0, 0, 1),
            self.local_port,
        ))
    }

    pub fn shutdown(&self, request: Shutdown) -> io::Result<()> {
        match request {
            Shutdown::Read | Shutdown::Both => {
                let request = NetTcpManage {
                    cb_sid,
                    ip_addr: NetPiAddr::from(socket),
                    remote_port: socket.port(),
                    local_port: Some(self.local_port),
                    timeout_ms: if let Some(d) = timeout { Some(d.total_millis()) } else { None },
                    keepalive_ms: if let Some(d) = keepalive { Some(d.total_millis()) } else { None },
                    result: None,
                    rx_shutdown: Some(true),
                };
                let mut buf = Buffer::into_buf(request)
                    .or(Err(Error::new(ErrorKind::Other, "can't register with Net server")))?;
                buf.lend_mut(net.conn(), Opcode::TcpRxShutdown.to_u32().unwrap())
                    .or(Err(Error::new(ErrorKind::Other, "can't register with Net server")))?;
                let ret = buf.to_original::<NetTcpManage, _>().unwrap();

                if request == Shutdown::Both {
                    self.tx_shutdown = true;
                }
                match ret.result {
                    Ok(_) => {
                        self.rx_shutdown = true;
                        Ok(())
                    }
                    Err(_) => {
                        Err(Error::new(ErrorKind::Other, "Internal error"))
                    }
                }
            }
            Shutdown::Write => {
                self.tx_shutdown = true;
                Ok(())
            }
        }
    }

    pub fn duplicate(&self) -> io::Result<TcpStream> {
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

    pub fn peek(&self, _: &mut [u8]) -> io::Result<usize> {
        unimplemented!()
    }
}

impl fmt::Debug for TcpStream {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        unimplemented!()
    }
}

impl Read for TcpStream {
    fn read(&self, _: &mut [u8]) -> io::Result<usize> {
        unimplemented!()
    }
}

impl Write for TcpStream {
    fn write(&self, _: &[u8]) -> io::Result<usize> {
        unimplemented!()
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        let request = NetTcpManage {
            ip_addr: NetIpAddr::from(self.socket_addr),
            remote_port: self.socket_addr.port(),
            local_port: self.local_port,
            cb_sid: self.cb_sid.to_array(),
            timeout_ms: None,
            keepalive_ms: None,
            result: None,
            rx_shutdown: None, // this is ignored by the close because it is redundant
        };
        let mut buf = Buffer::into_buf(request).expect("can't unregister with Net server");
        buf.lend_mut(self.net.conn(), Opcode::TcpClose.to_u32().unwrap()).expect("can't unregister with Net server");
        match buf.to_original().unwrap() {
            NetMemResponse::Ok => {
                let drop_cid = xous::connect(self.cb_sid).unwrap();
                xous::send_message(
                    drop_cid,
                    Message::new_blocking_scalar(NetTcpCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0)
                ).expect("couldn't send Drop to our repsonding server");
                unsafe{xous::disconnect(drop_cid).ok()}; // should be safe because we're the only connection and the previous was a blocking scalar
            },
            _ => {
                panic!("Couldn't unregister with net server");
            }
        }
        // this will block until the responder thread exits, which it should because it received the Drop message
        if let Some(handle) = self.rx_handle.take() {
            handle.join().unwrap();
        }
        // now we can detroy the server id of the responder thread
        xous::destroy_server(self.cb_sid).ok();
    }
}