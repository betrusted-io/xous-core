use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::unimplemented;
use std::io;
use std::io::{Error, ErrorKind, Result};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU32, Ordering};
use std::fmt;
use std::collections::VecDeque;

use xous::{Message, SID, CID};
use xous_ipc::Buffer;
use crate::TcpStream;
use crate::NetConn;
use crate::api::*;
use num_traits::*;

const LISTENER_POLL_INTERVAL_MS: usize = 250;

pub struct TcpListener {
    net: NetConn,
    cb_sid: SID,
    ticktimer: ticktimer_server::Ticktimer,
    xns: xous_names::XousNames,
    refcount: Arc<AtomicU32>,
    port: u16,
    nonblocking: bool,
    original_notify: XousScalarEndpoint,

    // reserved for when the responder thread is "promoted" to active status
    rx_buf: Arc::<Mutex::<VecDeque::<u8>>>,
    /// xous-specific feature that allows for more efficent Rx than blocking. Caller must set_scalar_notification() to use it.
    rx_notify: Arc<Mutex<XousScalarEndpoint>>,
    listener_info: Arc<Mutex<Option<NetTcpListenCallback>>>,
}

impl TcpListener {
    pub fn bind(maybe_socket: io::Result<&SocketAddr>) -> io::Result<TcpListener> {
        TcpListener::bind_inner(maybe_socket)
    }

    pub fn bind_xous<A: ToSocketAddrs>(addr: A) -> Result<TcpListener> {
        match addr.to_socket_addrs() {
            Ok(socks) => {
                match socks.into_iter().next() {
                    Some(socket_addr) => {
                        TcpListener::bind_inner(
                            Ok(&socket_addr),
                        )
                    }
                    _ => Err(Error::new(ErrorKind::InvalidInput, "IP address invalid"))
                }
            }
            _ => Err(Error::new(ErrorKind::InvalidInput, "IP address invalid"))
        }
    }

    pub fn bind_inner(maybe_socket: io::Result<&SocketAddr>) -> io::Result<TcpListener> {
        let socket = maybe_socket?;
        let xns = xous_names::XousNames::new().unwrap();
        let net = NetConn::new(&xns).unwrap();
        let cb_sid = xous::create_server().unwrap();
        let ticktimer = ticktimer_server::Ticktimer::new().unwrap();

        let rx_buf = Arc::new(Mutex::new(VecDeque::<u8>::new()));
        rx_buf.lock().unwrap().reserve(TCP_BUFFER_SIZE);
        let notify = Arc::new(Mutex::new(XousScalarEndpoint::new()));
        let listener_info = Arc::new(Mutex::new(None::<NetTcpListenCallback>));
        log::info!("TcpListener creating first thread with listener object: {:?}", listener_info);
        let _handle = crate::tcp_rx_thread(
            cb_sid.clone(),
            Arc::clone(&rx_buf),
            Arc::clone(&notify),
            // this is not used by TcpStream
            Arc::clone(&listener_info)
        );
        let request = NetTcpListen {
            cb_sid: cb_sid.to_array(),
            local_port: socket.port(),
            result: None,
        };
        log::debug!("Attempting to bind a listener to port {}", socket.port());
        let mut buf = Buffer::into_buf(request)
            .or(Err(Error::new(ErrorKind::Other, "can't register with Net server")))?;
        buf.lend_mut(net.conn(), Opcode::TcpListen.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "can't register with Net server")))?;
        let ret = buf.to_original::<NetTcpListen, _>().unwrap();
        log::debug!("Got result: {:?}", ret.result);
        match ret.result {
            Some(NetMemResponse::Ok) => {
                Ok(TcpListener {
                    net,
                    cb_sid,
                    ticktimer,
                    xns,
                    refcount: Arc::new(AtomicU32::new(1)), // one instance, that's me!
                    port: socket.port(),
                    rx_buf,
                    rx_notify: notify,
                    listener_info,
                    nonblocking: false,
                    original_notify: XousScalarEndpoint::new(),
                })
            }
            _ => {
                Err(Error::new(ErrorKind::Other, "can't create Listener with Net server"))
            }
        }
    }
    pub fn set_scalar_notification(&mut self, cid: CID, op: usize, args: [Option<usize>; 4]) {
        self.original_notify.set(cid, op, args);
        self.rx_notify.lock().unwrap().set(cid, op, args);
    }
    pub fn clear_scalar_notification(&mut self) {
        self.original_notify.clear();
        self.rx_notify.lock().unwrap().clear();
    }

    pub fn accept(&mut self) -> io::Result<(TcpStream, SocketAddr)> {
        loop {
            log::trace!("listener Arc count: {}", Arc::strong_count(&self.listener_info));
            let accepted = if let Some(info) = self.listener_info.lock().unwrap().take() {
                // 1. Create a new TcpStream object using our existing handles
                let remote = SocketAddr::new(IpAddr::from(info.ip_addr), info.remote_port);
                log::info!("Incoming TCP detected, building stream to {:?}", remote);
                let stream = TcpStream::build_from_listener(
                    NetConn::new(&self.xns).unwrap(),
                    self.cb_sid,
                    remote,
                    info.local_port,
                    std::mem::replace(&mut self.rx_buf, Arc::new(Mutex::new(VecDeque::<u8>::new()))),
                    // we keep our notifier for ourself, create a new one for the passed-on thread
                    Arc::new(Mutex::new(XousScalarEndpoint::new())),
                );
                Some((stream, remote))
            } else {
                log::trace!("Accept: found no incoming TCP, waiting... {:?}, {:?}", self.listener_info, self.cb_sid);
                if self.nonblocking {
                    return Err(Error::new(ErrorKind::WouldBlock, "accept would block"));
                }
                None
            };
            if let Some((stream, remote)) = accepted {
                // 2. Replenish our Listener objects and pass a TcpListen message on to the Net manager to repeat the cycle
                self.listener_info = Arc::new(Mutex::new(None::<NetTcpListenCallback>));
                self.cb_sid = xous::create_server().unwrap();
                // create a new notify arc/mutex, then copy the master notification settings there
                self.rx_notify = Arc::new(Mutex::new(XousScalarEndpoint::new()));
                if self.original_notify.is_set() {
                    let (c, o, a) = self.original_notify.get();
                    self.rx_notify.lock().unwrap().set(c.unwrap(), o.unwrap(), a);
                    log::debug!("Initializing listener with notifier: {:x?}", self.rx_notify.lock().unwrap());
                }
                let _handle = crate::tcp_rx_thread(
                    self.cb_sid.clone(),
                    Arc::clone(&self.rx_buf),
                    Arc::clone(&self.rx_notify),
                    Arc::clone(&self.listener_info)
                );
                log::trace!("Accept created new thread with listener object: {:?}", self.listener_info);
                let request = NetTcpListen {
                    cb_sid: self.cb_sid.to_array(),
                    local_port: stream.socket_addr().unwrap().port(),
                    result: None,
                };
                log::trace!("Accept is registering a new listener: {:?}", request);
                let mut buf = Buffer::into_buf(request)
                    .or(Err(Error::new(ErrorKind::Other, "can't register with Net server")))?;
                buf.lend_mut(self.net.conn(), Opcode::TcpListen.to_u32().unwrap())
                    .or(Err(Error::new(ErrorKind::Other, "can't register with Net server")))?;
                let ret = buf.to_original::<NetTcpListen, _>().unwrap();
                log::debug!("Listener registered with return code {:?}", ret.result);
                return
                    match ret.result {
                        Some(NetMemResponse::Ok) => Ok((stream, remote)),
                        _ => Err(Error::new(ErrorKind::Other, "can't renew Listener with Net server"))
                    }
            }

            self.ticktimer.sleep_ms(LISTENER_POLL_INTERVAL_MS).unwrap();
        }
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            self.port
        ))
    }

    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        self.tcp_manage(TcpMgmtCode::SetTtl(ttl)).map(|_| ())
    }

    pub fn ttl(&self) -> io::Result<u32> {
        match self.tcp_manage(TcpMgmtCode::GetTtl(0)) {
            Ok(TcpMgmtCode::GetTtl(value)) => Ok(value),
            _ => Err(Error::new(ErrorKind::Other, "Internal error")),
        }
    }

    /// I'm not really sure what this does on Xous, as we don't have a SO_ERROR value in our OS.
    /// Just returning Ok(None) for now.
    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        Ok(None)
    }

    pub fn set_nonblocking(&mut self, setting: bool) -> io::Result<()> {
        self.nonblocking = setting;
        Ok(())
    }

    pub fn duplicate(&self) -> io::Result<TcpListener> {
        Err(Error::new(ErrorKind::Other, "Xous does not support cloned listeners"))
    }

    fn tcp_manage(&self, code: TcpMgmtCode) -> io::Result<TcpMgmtCode> {
        // only local_port is needed
        let request = NetTcpManage {
            cb_sid: self.cb_sid.to_array(),
            ip_addr: NetIpAddr::Ipv4([127, 0, 0, 1]),
            remote_port: 0,
            local_port: Some(self.port),
            timeout_ms: None,
            keepalive_ms: None,
            result: None,
            mgmt_code: Some(code),
        };
        let mut buf = Buffer::into_buf(request)
            .or(Err(Error::new(ErrorKind::Other, "can't manage TCP")))?;
        buf.lend_mut(self.net.conn(), Opcode::TcpManageListener.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "can't manage TCP")))?;
        let ret = buf.to_original::<NetTcpManage, _>().unwrap();
        if let Some(code) = ret.mgmt_code {
            Ok(code)
        } else {
            Err(Error::new(ErrorKind::Other, "Internal error"))
        }
    }
}

impl fmt::Debug for TcpListener {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        unimplemented!()
    }
}

impl Drop for TcpListener {
    fn drop(&mut self) {
        if self.refcount.fetch_sub(1, Ordering::SeqCst) == 1 {
            // the only thing we really need from the management structure for this call is the local port...
            let request = NetTcpManage {
                ip_addr: NetIpAddr::from(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
                remote_port: 0, // bogus port
                local_port: Some(self.port),
                cb_sid: self.cb_sid.to_array(),
                timeout_ms: None,
                keepalive_ms: None,
                result: None,
                mgmt_code: Some(TcpMgmtCode::CloseListener),
            };
            let mut buf = Buffer::into_buf(request).expect("can't unregister with Net server");
            buf.lend_mut(self.net.conn(), Opcode::TcpManageListener.to_u32().unwrap()).expect("can't unregister with Net server");
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
            // now we can detroy the server id of the responder thread
            xous::destroy_server(self.cb_sid).ok();
        }
    }
}