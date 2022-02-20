use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};
use std::unimplemented;
use std::io;
use std::io::{Error, ErrorKind, Result};
use std::net::UdpSocket as UdpSocketHosted;

use smoltcp::time::Duration;
use crate::api::XousScalarEndpoint;
use core::sync::atomic::{AtomicBool, Ordering};


use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;

struct UdpRx {
    data: Vec<u8>,
    from: SocketAddr,
}
///////// UdpSocket implementation
pub struct UdpSocket {
    handle: Option<JoinHandle::<()>>,
    nonblocking: bool,
    socket: Arc<Mutex<UdpSocketHosted>>,
    rx_buf: Arc<Mutex<Vec<UdpRx>>>,
    notify: Arc<Mutex<XousScalarEndpoint>>,
    should_drop: Arc<AtomicBool>,
    read_timeout: Option<Duration>,
    ticktimer: ticktimer_server::Ticktimer,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum UdpHostedOpcode {
    Tx,
    Drop,
}

// next steps: build this stub, and figure out how to clean up the error handling code.
impl UdpSocket {
    pub fn bind(maybe_socket: io::Result<&SocketAddr>) -> Result<UdpSocket> {
        if let Ok(socket) = maybe_socket {
            UdpSocket::bind_inner(socket)
        } else {
            Err(Error::new(ErrorKind::InvalidInput, "IP address invalid"))
        }
    }
    pub fn bind_xous<A: ToSocketAddrs>(socket: A, _max_payload: Option<u16>) -> Result<UdpSocket> {
        match socket.to_socket_addrs() {
            Ok(socks) => {
                match socks.into_iter().next() {
                    Some(socket_addr) => {
                        UdpSocket::bind_inner(&socket_addr)
                    }
                    _ => Err(Error::new(ErrorKind::InvalidInput, "IP address invalid"))
                }
            }
            _ => Err(Error::new(ErrorKind::InvalidInput, "IP address invalid"))
        }
    }
    fn bind_inner(socket: &SocketAddr) -> Result<UdpSocket> {
        if let Ok(udpsocket_naked) = UdpSocketHosted::bind(socket) {
            let rx_buf = Arc::new(Mutex::new(Vec::new()));
            let notify = Arc::new(Mutex::new(XousScalarEndpoint::new()));
            let should_drop = Arc::new(AtomicBool::new(false));
            udpsocket_naked.set_nonblocking(true).expect("hosted mode couldn't set socket to nonblocking"); // we emulate blocking behavior outside this thread
            let udpsocket = Arc::new(Mutex::new(udpsocket_naked));
            let handle = thread::spawn({
                let udpsocket = udpsocket.clone();
                let should_drop = should_drop.clone();
                let rx_buf = rx_buf.clone();
                let notify = notify.clone();
                move || {
                    let mut buf = [0u8; 65536];
                    let tt = ticktimer_server::Ticktimer::new().unwrap();
                    loop {
                        if should_drop.load(Ordering::Relaxed) {
                            break;
                        } else {
                            if let Ok((len, addr)) = udpsocket.lock().unwrap().recv_from(&mut buf) {
                                log::info!("received {} bytes from {:?}", len, addr);
                                rx_buf.lock().unwrap().push(UdpRx { data: buf[..len].to_vec(), from: addr });
                                notify.lock().unwrap().notify(); // this will only notify if a destination has been set
                            }
                            // give some time for things to not deadlock
                            tt.sleep_ms(100).unwrap();
                        }
                    }
                }
            });
            Ok(UdpSocket {
                handle: Some(handle),
                nonblocking: false,
                socket: udpsocket,
                rx_buf,
                notify,
                should_drop,
                read_timeout: None,
                ticktimer: ticktimer_server::Ticktimer::new().unwrap(),
            })
        } else {
            Err(Error::new(ErrorKind::InvalidInput, "IP address invalid"))
        }

    }

    pub fn get_nonblocking(&self) -> bool {
        self.nonblocking
    }

    pub fn set_scalar_notification(&mut self, cid: xous::CID, op: usize, args: [Option<usize>; 4]) {
        self.notify.lock().unwrap().set(cid, op, args);
    }
    pub fn clear_scalar_notification(&mut self) {
        self.notify.lock().unwrap().clear();
    }

    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.read_timeout = timeout;
        let t = match timeout {
            Some(d) => {
                Some(std::time::Duration::from_millis(d.total_millis()))
            }
            None => None,
        };
        self.socket.lock().unwrap().set_read_timeout(t)
    }

    pub fn set_write_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        let t = match timeout {
            Some(d) => {
                Some(std::time::Duration::from_millis(d.total_millis()))
            }
            None => None,
        };
        self.socket.lock().unwrap().set_write_timeout(t)
    }

    pub fn read_timeout(&self) -> io::Result<Option<Duration>> {
        match self.socket.lock().unwrap().read_timeout() {
            Ok(maybe_t) => {
                match maybe_t {
                    Some(t) => {
                        Ok(Some(Duration::from_millis(t.as_millis() as u64)))
                    }
                    None => Ok(None)
                }
            },
            Err(e) => Err(e),
        }
    }

    pub fn write_timeout(&self) -> io::Result<Option<Duration>> {
        match self.socket.lock().unwrap().write_timeout() {
            Ok(maybe_t) => {
                match maybe_t {
                    Some(t) => {
                        Ok(Some(Duration::from_millis(t.as_millis() as u64)))
                    }
                    None => Ok(None)
                }
            },
            Err(e) => Err(e),
        }
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

    fn recv_from_inner(&self, pkt: &mut[u8], do_peek: bool) -> io::Result<(usize, SocketAddr)> {
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
                let len = rx_pkt.data.len();
                let socket_addr = rx_pkt.from;
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
        self.socket.lock().unwrap().connect(socket)
    }
    pub fn connect_xous<A: ToSocketAddrs>(&mut self, addr: A) -> io::Result<()> {
        self.socket.lock().unwrap().connect(addr)
    }

    pub fn send(&mut self, pkt: &[u8]) -> io::Result<usize> {
        self.socket.lock().unwrap().send(pkt)
    }

    pub fn send_to(&mut self, pkt: &[u8], addr: &SocketAddr) -> io::Result<usize> {
        self.socket.lock().unwrap().send_to(pkt, addr)
    }
    pub fn send_to_xous<A: ToSocketAddrs>(&mut self, pkt: &[u8], addr: A) -> io::Result<usize> {
        self.socket.lock().unwrap().send_to(pkt, addr)
    }

    pub fn socket_addr(&self) -> io::Result<SocketAddr> {
        self.socket.lock().unwrap().local_addr()
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.socket.lock().unwrap().peer_addr()
    }

    pub fn duplicate(&self) -> io::Result<UdpSocket> {
        // do the basic clone connection
        let sa = self.socket_addr()?;
        let mut cloned_socket = UdpSocket::bind_inner(&sa)?;
        // now copy all the properties over as required by spec
        cloned_socket.set_read_timeout(self.read_timeout)?;
        cloned_socket.set_nonblocking(self.get_nonblocking())?;
        // TTL doesn't need to be cloned because it is attached to the port itself in the Net server
        Ok(cloned_socket)
    }

    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        self.socket.lock().unwrap().set_ttl(ttl)
    }

    pub fn ttl(&self) -> io::Result<u32> {
        self.socket.lock().unwrap().ttl()
    }

    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.socket.lock().unwrap().take_error()
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

impl Drop for UdpSocket {
    fn drop(&mut self) {
        self.should_drop.store(true, Ordering::Relaxed);
        // this will block until the responder thread exits, which it should because it received the Drop message
        if let Some(handle) = self.handle.take() {
            handle.join().unwrap();
        }
    }
}