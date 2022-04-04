use std::convert::TryInto;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};
use std::unimplemented;
use std::io;
use std::io::{Error, ErrorKind, Result};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;

use smoltcp::wire::{Ipv4Address, Ipv6Address, IpAddress};
use smoltcp::{
    wire::IpEndpoint,
    time::Duration,
};

use xous::{CID, Message, SID, msg_blocking_scalar_unpack};
use xous_ipc::Buffer;
use crate::NetConn;
use crate::api::*;
//use crate::api::udp::*;
use num_traits::*;

//////// Public structures
pub struct UdpRx {
    pub endpoint: IpEndpoint,
    pub data: Vec<u8>,
}


///////// UdpSocket implementation
pub struct UdpSocket{
    net: NetConn,
    cb_sid: SID,
    socket_addr: SocketAddr,
    rx_buf: Arc<Mutex<Vec<UdpRx>>>,
    handle: Option<JoinHandle::<()>>,
    notify: Arc<Mutex<XousScalarEndpoint>>,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    ticktimer: ticktimer_server::Ticktimer,
    dest_socket: Option<SocketAddr>,
    max_payload: Option<u16>,
    nonblocking: bool,
}

// next steps: build this stub, and figure out how to clean up the error handling code.
impl UdpSocket {
    pub fn bind(maybe_socket: io::Result<&SocketAddr>) -> Result<UdpSocket> {
        UdpSocket::bind_inner(maybe_socket, None)
    }
    pub fn bind_xous<A: ToSocketAddrs>(socket: A, max_payload: Option<u16>) -> Result<UdpSocket> {
        match socket.to_socket_addrs() {
            Ok(socks) => {
                match socks.into_iter().next() {
                    Some(socket_addr) => {
                        UdpSocket::bind_inner(
                            Ok(&socket_addr),
                            max_payload)
                    }
                    _ => Err(Error::new(ErrorKind::InvalidInput, "IP address invalid"))
                }
            }
            _ => Err(Error::new(ErrorKind::InvalidInput, "IP address invalid"))
        }
    }

    fn bind_inner(maybe_socket: io::Result<&SocketAddr>, max_payload: Option<u16>) -> Result<UdpSocket> {
        let socket = maybe_socket?;
        let xns = xous_names::XousNames::new().unwrap();
        let net = NetConn::new(&xns).unwrap();
        let cb_sid = xous::create_server().unwrap();

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
                        Some(NetUdpCallback::RxData) => {
                            let buffer = unsafe {Buffer::from_memory_message(msg.body.memory_message().unwrap())};
                            let incoming = buffer.as_flat::<NetUdpResponse, _>().unwrap();
                            let endpoint = match incoming.endpoint_ip_addr {
                                ArchivedNetIpAddr::Ipv4(v4_octets) => {
                                    IpEndpoint::new(
                                        IpAddress::Ipv4(Ipv4Address::from_bytes(&v4_octets)),
                                        incoming.endpoint_port as u16,
                                    )
                                },
                                ArchivedNetIpAddr::Ipv6(v6_octets) => {
                                    IpEndpoint::new(
                                        IpAddress::Ipv6(Ipv6Address::from_bytes(&v6_octets)),
                                        incoming.endpoint_port as u16,
                                    )
                                },
                            };
                            let mut rx = UdpRx {
                                endpoint,
                                data: Vec::new(),
                            };
                            for &d in incoming.data[..incoming.len as usize].iter() {
                                rx.data.push(d);
                            }
                            rx_buf.lock().unwrap().push(rx);
                            notify.lock().unwrap().notify(); // this will only notify if a destination has been set
                        },
                        Some(NetUdpCallback::Drop) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                            log::debug!("Drop received, exiting Udp handler");
                            xous::return_scalar(msg.sender, 1).unwrap(); // actual return value doesn't matter -- it's that there is a return value
                            break;
                        }),
                        None => {
                            log::error!("got unknown message type on Udp callback: {:?}", msg);
                        }
                    }
                }
            }
        });

        let request = NetUdpBind {
            ip_addr: NetIpAddr::from(*socket),
            port: socket.port(),
            cb_sid: cb_sid.to_array(),
            max_payload,
        };
        let mut buf = Buffer::into_buf(request)
            .or(Err(Error::new(ErrorKind::Other, "can't register with Net server")))?;
        buf.lend_mut(net.conn(), Opcode::UdpBind.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "can't register with Net server")))?;
        match buf.to_original().unwrap() {
            NetMemResponse::Ok => {
                Ok(UdpSocket {
                    net,
                    cb_sid,
                    socket_addr: *socket,
                    rx_buf,
                    handle: Some(handle),
                    notify,
                    read_timeout: None,
                    write_timeout: None,
                    ticktimer: ticktimer_server::Ticktimer::new().unwrap(),
                    dest_socket: None,
                    max_payload,
                    nonblocking: false,
                })
            },
            _ => {
                Err(Error::new(ErrorKind::Other, "can't register with Net server"))
            }
        }
    }
    pub fn get_nonblocking(&self) -> bool { self.nonblocking }

    pub fn set_scalar_notification(&mut self, cid: CID, op: usize, args: [Option<usize>; 4]) {
        self.notify.lock().unwrap().set(cid, op, args);
    }
    pub fn clear_scalar_notification(&mut self) {
        self.notify.lock().unwrap().clear();
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

    pub fn recv(&self, pkt: &mut [u8]) -> io::Result<usize> {
        self.recv_from_inner(pkt, false).map(|(len, _addr)| len)
    }
    pub fn peek(&self, pkt: &mut [u8]) -> io::Result<usize> {
        self.recv_from_inner(pkt, true).map(|(len, _addr)| len)
    }
    pub fn recv_from(&self, pkt: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.recv_from_inner(pkt, false)
    }
    pub fn peek_from(&self, pkt: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.recv_from_inner(pkt, true)
    }

    fn recv_from_inner(&self, pkt: &mut [u8], do_peek: bool) -> io::Result<(usize, SocketAddr)> {
        let timeout = if let Some(to) = self.read_timeout {
            to.total_millis() + self.ticktimer.elapsed_ms()
        } else {
            u64::MAX
        };
        loop {
            if self.rx_buf.lock().unwrap().len() > 0 {
                let rx_pkt = self.rx_buf.lock().unwrap().remove(0); // safe b/c len > 1, checked above
                for (&src, dst) in rx_pkt.data.iter().zip(pkt.iter_mut()) {
                    *dst = src;
                }
                let addr = match rx_pkt.endpoint.addr {
                    IpAddress::Ipv4(ipv4) => {
                        IpAddr::V4(Ipv4Addr::new(ipv4.0[0], ipv4.0[1], ipv4.0[2], ipv4.0[3]))
                    },
                    IpAddress::Ipv6(ipv6) => {
                        IpAddr::V6(Ipv6Addr::new(
                            u16::from_be_bytes(ipv6.0[0..1].try_into().unwrap()),
                            u16::from_be_bytes(ipv6.0[2..3].try_into().unwrap()),
                            u16::from_be_bytes(ipv6.0[4..5].try_into().unwrap()),
                            u16::from_be_bytes(ipv6.0[6..7].try_into().unwrap()),
                            u16::from_be_bytes(ipv6.0[8..9].try_into().unwrap()),
                            u16::from_be_bytes(ipv6.0[10..11].try_into().unwrap()),
                            u16::from_be_bytes(ipv6.0[12..13].try_into().unwrap()),
                            u16::from_be_bytes(ipv6.0[14..15].try_into().unwrap()),
                        ))
                    },
                    _ => {
                        panic!("malformed endpoint record");
                    }
                };
                let len = rx_pkt.data.len();
                let socket_addr = SocketAddr::new(
                    addr,
                    rx_pkt.endpoint.port,
                );
                if do_peek {
                    // re-insert the element after taking it out. We can't mux it above with the if/else because
                    // a peek is a borrow, but a remove is a move, and that's not easy to coerce in Rust
                    self.rx_buf.lock().unwrap().insert(0, rx_pkt);
                }
                return Ok((
                    len,
                    socket_addr
                ));
            }
            if timeout < self.ticktimer.elapsed_ms() {
                return Err(Error::new(ErrorKind::WouldBlock, "UDP Rx timeout reached"));
            }
            xous::yield_slice();
        }
    }

    pub fn connect(&mut self, maybe_socket_addr: io::Result<&SocketAddr>) -> io::Result<()> {
        let socket = *maybe_socket_addr?;
        self.dest_socket = Some(socket);
        // the socket is just locally stored until the next send call, so there's nothing to send to the Net server
        Ok(())
    }
    pub fn connect_xous<A: ToSocketAddrs>(&mut self, addr: A) -> io::Result<()> {
        match addr.to_socket_addrs() {
            Ok(socks) => {
                match socks.into_iter().next() {
                    Some(socket_addr) => self.connect(Ok(&socket_addr)),
                    _ => Err(Error::new(ErrorKind::InvalidInput, "Destination socket invalid"))
                }
            }
            _ => Err(Error::new(ErrorKind::InvalidInput, "Destination socket invalid"))
        }
    }

    pub fn send(&mut self, pkt: &[u8]) -> io::Result<usize> {
        self.send_inner(pkt, None)
    }

    pub fn send_to(&mut self, pkt: &[u8], addr: &SocketAddr) -> io::Result<usize> {
        self.send_inner(pkt, Some(addr))
    }
    pub fn send_to_xous<A: ToSocketAddrs>(&mut self, pkt: &[u8], addr: A) -> io::Result<usize> {
        match addr.to_socket_addrs() {
            Ok(socks) => {
                match socks.into_iter().next() {
                    Some(socket_addr) => self.send_inner(pkt, Some(&socket_addr)),
                    _ => Err(Error::new(ErrorKind::InvalidInput, "Destination socket invalid"))
                }
            }
            _ => Err(Error::new(ErrorKind::InvalidInput, "Destination socket invalid"))
        }
    }

    fn send_inner(&mut self, pkt: &[u8], maybe_dest: Option<&SocketAddr>) -> io::Result<usize> {
        let dest = if let Some(&d) = maybe_dest {
            Some(NetSocketAddr::from(d))
        } else {
            // if not specified, grab from the local destination copy stored here
            if let Some(dest) = self.dest_socket {
                Some(NetSocketAddr::from(dest))
            } else {
                return Err(Error::new(ErrorKind::AddrNotAvailable, "Destination address was not specified with connect"));
            }
        };
        let mut udp_tx = NetUdpTransmit {
            dest_socket: dest,
            local_port: self.socket_addr.port(),
            len: pkt.len() as u16,
            data: [0; UDP_RESPONSE_MAX_LEN],
        };
        for (&src, dst) in pkt.iter().zip(udp_tx.data.iter_mut()) {
            *dst = src;
        }

        let mut buf = Buffer::into_buf(udp_tx)
            .or(Err(Error::new(ErrorKind::Other, "can't send to Net server")))?;
        buf.lend_mut(self.net.conn(), Opcode::UdpTx.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "can't send to Net server")))?;
        match buf.to_original().unwrap() {
            NetMemResponse::Sent(len) => Ok(len as usize),
            _ => Err(Error::new(ErrorKind::Other, "send failed")),
        }
    }

    pub fn socket_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.socket_addr)
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        match self.dest_socket {
            Some(dest) => {
                Ok(dest)
            }
            None => {
                Err(Error::new(ErrorKind::NotConnected, "No peer specified"))
            }
        }
    }

    pub fn duplicate(&self) -> io::Result<UdpSocket> {
        // do the basic clone connection
        let mut cloned_socket = UdpSocket::bind_inner(Ok(&self.socket_addr), self.max_payload)?;
        // now copy all the properties over as required by spec
        cloned_socket.set_read_timeout(self.read_timeout)?;
        cloned_socket.set_write_timeout(self.write_timeout)?;
        if let Some(sa) = self.dest_socket {
            cloned_socket.connect(Ok(&sa))?
        }
        cloned_socket.set_nonblocking(self.get_nonblocking())?;
        // TTL doesn't need to be cloned because it is attached to the port itself in the Net server
        Ok(cloned_socket)
    }

    /*
    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        if ttl > 255 {
            return Err(Error::new(ErrorKind::InvalidInput, "TTL must be less than 256"))
        }
        send_message(
            self.net.conn(),
            Message::new_scalar(Opcode::UdpSetTtl.to_usize().unwrap(), ttl as usize, self.socket_addr.port() as usize, 0, 0)
        ).map_or_else(|_| Ok(()), |_| Err(Error::new(ErrorKind::ConnectionRefused, "can't send TTL set message")))
    }

    pub fn ttl(&self) -> io::Result<u32> {
        let result = send_message(
            self.net.conn(),
            Message::new_blocking_scalar(Opcode::UdpGetTtl.to_usize().unwrap(), self.socket_addr.port() as usize, 0, 0, 0)
        ).expect("couldn't retrieve TTL value");
        if let xous::Result::Scalar1(ttl) = result {
            Ok(ttl as u32)
        } else {
            Err(Error::new(ErrorKind::ConnectionRefused, "can't get TTL value from Net server"))
        }
    }
    */
    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        // we don't have stateful errors, yet?...
        Ok(None)
    }

    pub fn set_nonblocking(&mut self, nb: bool) -> io::Result<()> {
        self.nonblocking = nb;
        Ok(())
    }

    //////// the remaining functions are currently unimplemented: we don't have multicast or broadcast support //////////
    pub fn set_broadcast(&self, _: bool) -> io::Result<()> {
        unimplemented!("work in progress")
    }

    pub fn broadcast(&self) -> io::Result<bool> {
        unimplemented!("work in progress")
    }

    pub fn set_multicast_loop_v4(&self, _: bool) -> io::Result<()> {
        unimplemented!("work in progress")
    }

    pub fn multicast_loop_v4(&self) -> io::Result<bool> {
        unimplemented!("work in progress")
    }

    pub fn set_multicast_ttl_v4(&self, _: u32) -> io::Result<()> {
        unimplemented!("work in progress")
    }

    pub fn multicast_ttl_v4(&self) -> io::Result<u32> {
        unimplemented!("work in progress")
    }

    pub fn join_multicast_v4(&self, _: &Ipv4Addr, _: &Ipv4Addr) -> io::Result<()> {
        unimplemented!("work in progress")
    }

    pub fn leave_multicast_v4(&self, _: &Ipv4Addr, _: &Ipv4Addr) -> io::Result<()> {
        unimplemented!("work in progress")
    }

    pub fn set_multicast_loop_v6(&self, _: bool) -> io::Result<()> {
        unimplemented!("ipv6 not implemented")
    }

    pub fn multicast_loop_v6(&self) -> io::Result<bool> {
        unimplemented!("ipv6 not implemented")
    }

    pub fn join_multicast_v6(&self, _: &Ipv6Addr, _: u32) -> io::Result<()> {
        unimplemented!("ipv6 not implemented")
    }

    pub fn leave_multicast_v6(&self, _: &Ipv6Addr, _: u32) -> io::Result<()> {
        unimplemented!("ipv6 not implemented")
    }

}

impl std::fmt::Debug for UdpSocket {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unimplemented!("work in progress")
    }
}

impl Drop for UdpSocket {
    fn drop(&mut self) {
        let request = NetUdpBind {
            ip_addr: NetIpAddr::from(self.socket_addr),
            port: self.socket_addr.port(),
            cb_sid: self.cb_sid.to_array(),
            max_payload: None,
        };
        let mut buf = Buffer::into_buf(request).expect("can't unregister with Net server");
        buf.lend_mut(self.net.conn(), Opcode::UdpClose.to_u32().unwrap()).expect("can't unregister with Net server");
        match buf.to_original().unwrap() {
            NetMemResponse::Ok => {
                let drop_cid = xous::connect(self.cb_sid).unwrap();
                xous::send_message(
                    drop_cid,
                    Message::new_blocking_scalar(NetUdpCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0)
                ).expect("couldn't send Drop to our repsonding server");
                unsafe{xous::disconnect(drop_cid).unwrap()}; // should be safe because we're the only connection and the previous was a blocking scalar
            },
            _ => {
                panic!("Couldn't unregister with net server");
            }
        }
        // this will block until the responder thread exits, which it should because it received the Drop message
        if let Some(handle) = self.handle.take() {
            handle.join().unwrap();
        }
        // now we can detroy the server id of the responder thread
        xous::destroy_server(self.cb_sid).unwrap();
    }
}