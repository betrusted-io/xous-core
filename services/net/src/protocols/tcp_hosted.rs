use std::net::{SocketAddr, ToSocketAddrs};
use std::io;
use std::io::{Error, ErrorKind, Result};
use std::io::{Read, Write};
use std::net::TcpStream as TcpStreamHosted;
use smoltcp::time::Duration;
use std::net::Shutdown;
use std::unimplemented;
use std::net::TcpListener as TcpListenerHosted;

pub struct TcpStream {
    stream: TcpStreamHosted,
}

impl TcpStream {
    pub fn connect(maybe_socket: io::Result<&SocketAddr>) -> io::Result<TcpStream> {
        if let Ok(socket) = maybe_socket {
            match TcpStreamHosted::connect(socket) {
                Ok(stream) => {
                    Ok(TcpStream {
                        stream,
                    })
                },
                Err(e) => Err(e),
            }
        } else {
            Err(Error::new(ErrorKind::InvalidInput, "IP address invalid"))
        }
    }
    pub fn connect_timeout(maybe_socket: io::Result<&SocketAddr>, duration: Duration) -> io::Result<TcpStream> {
        if let Ok(socket) = maybe_socket {
            let d = std::time::Duration::from_millis(duration.total_millis());
            match TcpStreamHosted::connect_timeout(socket, d) {
                Ok(stream) => {
                    Ok(TcpStream {
                        stream,
                    })
                },
                Err(e) => Err(e),
            }
        } else {
            Err(Error::new(ErrorKind::InvalidInput, "IP address invalid"))
        }
    }

    /// This API call uses the stdlib-internal calling convention to assist with the process of migrating this into a true libstd
    pub fn connect_xous<A: ToSocketAddrs>(addr: A, timeout: Option<Duration>, _keepalive: Option<Duration>) -> Result<TcpStream> {
        if let Some (t) = timeout {
            let d = std::time::Duration::from_millis(t.total_millis());
            match addr.to_socket_addrs() {
                Ok(socks) => {
                    match socks.into_iter().next() {
                        Some(socket_addr) => {
                            match TcpStreamHosted::connect_timeout(&socket_addr, d) {
                                Ok(stream) => {
                                    Ok(TcpStream {
                                        stream,
                                    })
                                },
                                Err(e) => Err(e),
                            }
                        }
                        _ => Err(Error::new(ErrorKind::InvalidInput, "IP address invalid"))
                    }
                }
                _ => Err(Error::new(ErrorKind::InvalidInput, "IP address invalid"))
            }
        } else {
            match TcpStreamHosted::connect(addr) {
                Ok(stream) => {
                    Ok(TcpStream {
                        stream,
                    })
                },
                Err(e) => Err(e),
            }
        }
    }
    /// dev note: if you need this function in hosted mode, contact bunnie.
    pub fn set_scalar_notification(&mut self, _cid: xous::CID, _op: usize, _args: [Option<usize>; 4]) {
        unimplemented!()
    }
    /// dev note: if you need this function in hosted mode, contact bunnie.
    pub fn clear_scalar_notification(&mut self) {
        unimplemented!()
    }

    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        let t = match timeout {
            Some(d) => {
                Some(std::time::Duration::from_millis(d.total_millis()))
            }
            None => None,
        };
        self.stream.set_read_timeout(t)
    }
    pub fn set_write_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        let t = match timeout {
            Some(d) => {
                Some(std::time::Duration::from_millis(d.total_millis()))
            }
            None => None,
        };
        self.stream.set_write_timeout(t)
    }
    pub fn read_timeout(&self) -> io::Result<Option<Duration>> {
        match self.stream.read_timeout() {
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
        match self.stream.write_timeout() {
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
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.stream.peer_addr()
    }
    pub fn socket_addr(&self) -> io::Result<SocketAddr> {
        self.stream.local_addr()
    }

    pub fn shutdown(&mut self, request: Shutdown) -> io::Result<()> {
        self.stream.shutdown(request)
    }

    pub fn set_nodelay(&self, setting: bool) -> io::Result<()> {
        self.stream.set_nodelay(setting)
    }

    pub fn nodelay(&self) -> io::Result<bool> {
        self.stream.nodelay()
    }

    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        self.stream.set_ttl(ttl)
    }

    pub fn ttl(&self) -> io::Result<u32> {
        self.stream.ttl()
    }

    /// I'm not quite sure this has any meaning on Xous. Right now this just checks if there is an
    /// Rx state error in progress.
    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.stream.take_error()
    }

    pub fn set_nonblocking(&mut self, setting: bool) -> io::Result<()> {
        self.stream.set_nonblocking(setting)
    }

    pub fn duplicate(&self) -> io::Result<TcpStream> {
        match self.stream.try_clone() {
            Ok(stream) => {
                Ok(TcpStream {
                    stream
                })
            },
            Err(e) => Err(e)
        }
    }

    pub fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream.peek(buf)
    }
}

impl Read for TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream.read(buf)
    }
}

impl Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream.write(buf)
    }

    /// In the case that Nagle's algo is enabled, packets can get stuck in the outgoing buffer.
    /// This basically waits until Nagle's algo times out, and the data is sent.
    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}



pub struct TcpListener {
    listener: TcpListenerHosted,
}

impl TcpListener {
    pub fn bind(maybe_socket: io::Result<&SocketAddr>) -> io::Result<TcpListener> {
        match maybe_socket {
            Ok(socket) => {
                match TcpListenerHosted::bind(socket) {
                    Ok(listener) => {
                        Ok(
                            TcpListener {
                                listener,
                            }
                        )
                    }
                    Err(e) => Err(e)
                }
            }
            Err(e) => Err(e)
        }
    }

    pub fn bind_xous<A: ToSocketAddrs>(addr: A) -> Result<TcpListener> {
        match TcpListenerHosted::bind(addr) {
            Ok(listener) => {
                Ok(
                    TcpListener {
                        listener,
                    }
                )
            }
            Err(e) => Err(e)
        }
    }

    pub fn set_scalar_notification(&mut self, _cid: xous::CID, _op: usize, _args: [Option<usize>; 4]) {
        unimplemented!()
    }
    pub fn clear_scalar_notification(&mut self) {
        unimplemented!()
    }

    pub fn accept(&mut self) -> io::Result<(TcpStream, SocketAddr)> {
        match self.listener.accept() {
            Ok((stream, addr)) => {
                Ok((
                    TcpStream {
                        stream,
                    },
                    addr
                ))
            }
            Err(e) => Err(e)
        }
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        self.listener.set_ttl(ttl)
    }

    pub fn ttl(&self) -> io::Result<u32> {
        self.listener.ttl()
    }

    /// I'm not really sure what this does on Xous, as we don't have a SO_ERROR value in our OS.
    /// Just returning Ok(None) for now.
    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.listener.take_error()
    }

    pub fn set_nonblocking(&mut self, setting: bool) -> io::Result<()> {
        self.listener.set_nonblocking(setting)
    }

    pub fn duplicate(&self) -> io::Result<TcpListener> {
        Err(Error::new(ErrorKind::Other, "Xous does not support cloned listeners"))
    }
}
