use smoltcp::socket::tcp;
use smoltcp::wire::{IpAddress, IpEndpoint};

use crate::*;

pub(crate) fn std_tcp_listen(
    mut msg: xous::MessageEnvelope,
    _iface: &mut Interface,
    sockets: &mut SocketSet,
    our_sockets: &mut Vec<Option<SocketHandle>>,
    trng: &trng::Trng,
) {
    // Ignore nonblocking and scalar messages
    let body = match msg.body.memory_message_mut() {
        Some(b) => b,
        None => {
            log::trace!("invalid message type");
            std_failure(msg, NetError::LibraryError);
            return;
        }
    };

    let bytes = unsafe { body.buf.as_slice::<u8>() };
    let mut local_port = u16::from_le_bytes([bytes[0], bytes[1]]);
    let mut retry_local_port = false;
    if local_port == 0 {
        local_port = (trng.get_u32().unwrap() % 16384 + 49152) as u16;
        retry_local_port = true;
    }
    let address = match parse_address(&bytes[2..]) {
        Some(addr) => addr,
        None => {
            log::trace!("couldn't parse address");
            std_failure(msg, NetError::LibraryError);
            return;
        }
    };
    if address.as_bytes() != [0, 0, 0, 0]
        && address.as_bytes() != [127, 0, 0, 1]
        && address.as_bytes() != IPV4_ADDRESS.load(Ordering::SeqCst).to_be_bytes()
    {
        std_failure(msg, NetError::Invalid);
        return;
    }

    let tcp_rx_buffer = tcp::SocketBuffer::new(vec![0; TCP_BUFFER_SIZE]);
    let tcp_tx_buffer = tcp::SocketBuffer::new(vec![0; TCP_BUFFER_SIZE]);
    let tcp_socket = tcp::Socket::new(tcp_rx_buffer, tcp_tx_buffer);

    let handle = sockets.add(tcp_socket);
    let tcp_socket = sockets.get_mut::<tcp::Socket>(handle);

    loop {
        if let Err(e) = tcp_socket.listen(local_port).map_err(|e| match e {
            smoltcp::socket::tcp::ListenError::InvalidState => NetError::SocketInUse,
            smoltcp::socket::tcp::ListenError::Unaddressable => NetError::Unaddressable,
        }) {
            match e {
                NetError::SocketInUse => {
                    // catch the case that someone gave us port 0, we picked a random port, and it didn't work
                    // out. basically, try it again...
                    if retry_local_port {
                        local_port = (trng.get_u32().unwrap() % 16384 + 49152) as u16;
                        continue;
                    } else {
                        log::debug!("couldn't listen: {:?}", e);
                        std_failure(msg, e);
                        return;
                    }
                }
                _ => {
                    log::debug!("couldn't listen: {:?}", e);
                    std_failure(msg, e);
                    return;
                }
            }
        } else {
            break;
        }
    }

    // Add the socket into our process' list of sockets, and pass the index back as the `fd` parameter for
    // future reference.
    let fd = insert_or_append(our_sockets, handle) as u8;

    let body = msg.body.memory_message_mut().unwrap();
    let bfr = unsafe { body.buf.as_slice_mut::<u8>() };
    log::debug!("successfully connected: {} -> {:?}:{}", fd, address, local_port);
    bfr[0] = 0;
    bfr[1] = fd;
    let local_port_u8 = local_port.to_le_bytes();
    bfr[2] = local_port_u8[0];
    bfr[3] = local_port_u8[1];
}

pub(crate) fn std_tcp_accept(
    mut msg: xous::MessageEnvelope,
    _iface: &mut Interface,
    sockets: &mut SocketSet,
    tcp_accept_waiting: &mut Vec<Option<AcceptingSocket>>,
    tcp_server_remote_close_poll: &mut Vec<SocketHandle>,
    our_sockets: &Vec<Option<SocketHandle>>,
) {
    let fd = (msg.body.id() >> 16) & 0xffff;
    let body = match msg.body.memory_message_mut() {
        Some(body) => body,
        None => {
            std_failure(msg, NetError::LibraryError);
            return;
        }
    };

    // this is not used
    body.valid = None;

    let handle = match our_sockets.get(fd) {
        Some(Some(val)) => val,
        _ => {
            std_failure(msg, NetError::Invalid);
            return;
        }
    };

    let args = unsafe { body.buf.as_slice::<u8>() };
    let nonblocking = args[0] == 0;

    let socket = sockets.get::<tcp::Socket>(*handle);

    if socket.is_active() {
        log::debug!("accept did not block; immediately returning TcpSocket");
        let buf = unsafe { body.buf.as_slice_mut::<u8>() };
        tcp_server_remote_close_poll.push(*handle);
        tcp_accept_success(
            buf,
            fd as u16,
            socket.remote_endpoint().expect("TCP accept missing remote endpoint"),
        );
        return;
    }

    if nonblocking {
        std_failure(msg, NetError::WouldBlock);
        return;
    }
    log::debug!("TCP listener added to accept queue");

    // Adding the message to the udp_rx_waiting list prevents it from going out of scope and
    // thus prevents the .drop() method from being called. Since messages are returned to the sender
    // in the .drop() method, this keeps the caller blocked for the lifetime of the message.
    insert_or_append(
        tcp_accept_waiting,
        AcceptingSocket {
            env: msg, /* <-- msg is inserted into the tcp_accept_waiting vector, thus preventing the
                       * lend_mut from returning. */
            handle: *handle,
            fd,
        },
    );
}

pub(crate) fn tcp_accept_success(buf: &mut [u8], fd: u16, ep: IpEndpoint) {
    log::debug!("tcp accept: remote {:?}", ep);
    buf[0] = 0;
    let fd_arr = fd.to_le_bytes();
    buf[1] = fd_arr[0];
    buf[2] = fd_arr[1];
    match ep.addr {
        IpAddress::Ipv4(a) => {
            buf[3] = 4;
            for (&s, d) in a.0.iter().zip(buf[4..8].iter_mut()) {
                *d = s;
            }
        }
        IpAddress::Ipv6(a) => {
            buf[3] = 6;
            for (&s, d) in a.0.iter().zip(buf[4..20].iter_mut()) {
                *d = s;
            }
        }
    }
    let port = ep.port.to_le_bytes();
    buf[20] = port[0];
    buf[21] = port[1];
}
