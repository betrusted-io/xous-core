use crate::*;
use crate::device::NetPhy;
use smoltcp::wire::{IpEndpoint, IpAddress};

pub(crate) fn std_tcp_listen(
    mut msg: xous::MessageEnvelope,
    iface: &mut Interface::<NetPhy>,
    our_sockets: &mut Vec<Option<SocketHandle>>,
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

    let bytes = body.buf.as_slice::<u8>();
    let local_port = u16::from_le_bytes([bytes[0], bytes[1]]);
    let address = match parse_address(&bytes[2..]) {
        Some(addr) => addr,
        None => {
            log::trace!("couldn't parse address");
            std_failure(msg, NetError::LibraryError);
            return;
        }
    };

    let tcp_rx_buffer = TcpSocketBuffer::new(vec![0; TCP_BUFFER_SIZE]);
    let tcp_tx_buffer = TcpSocketBuffer::new(vec![0; TCP_BUFFER_SIZE]);
    let tcp_socket = TcpSocket::new(tcp_rx_buffer, tcp_tx_buffer);
    let handle = iface.add_socket(tcp_socket);
    let tcp_socket = iface.get_socket::<TcpSocket>(handle);

    if let Err(e) = tcp_socket
        .listen(local_port)
        .map_err(|e| match e {
            smoltcp::Error::Illegal => NetError::SocketInUse,
            smoltcp::Error::Unaddressable => NetError::Unaddressable,
            _ => NetError::LibraryError,
        })
    {
        log::trace!("couldn't listen: {:?}", e);
        std_failure(msg, e);
        return;
    }

    // Add the socket into our process' list of sockets, and pass the index back as the `fd` parameter for future reference.
    let fd = insert_or_append(our_sockets, handle) as u8;

    let body = msg.body.memory_message_mut().unwrap();
    let bfr = body.buf.as_slice_mut::<u8>();
    log::trace!("successfully connected: {} -> {:?}:{}", fd, address, local_port);
    bfr[0] = 0;
    bfr[1] = fd;
}

pub(crate) fn std_tcp_accept(
    mut msg: xous::MessageEnvelope,
    iface: &mut Interface::<NetPhy>,
    tcp_accept_waiting: &mut Vec<Option<AcceptingSocket>>,
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

    let args = body.buf.as_slice::<u8>();
    let nonblocking = args[0] == 0;

    let socket = iface.get_socket::<TcpSocket>(*handle);

    if socket.is_active() {
        log::trace!("accept did not block; immediately returning TcpSocket");
        let buf = body.buf.as_slice_mut::<u8>();
        tcp_accept_success(buf, fd as u16, socket.remote_endpoint());
        return;
    }

    if nonblocking {
        std_failure(msg, NetError::WouldBlock);
        return;
    }
    log::trace!("TCP listener added to accept queue");

    // Adding the message to the udp_rx_waiting list prevents it from going out of scope and
    // thus prevents the .drop() method from being called. Since messages are returned to the sender
    // in the .drop() method, this keeps the caller blocked for the lifetime of the message.
    insert_or_append(
        tcp_accept_waiting,
        AcceptingSocket {
            env: msg, // <-- msg is inserted into the tcp_accept_waiting vector, thus preventing the lend_mut from returning.
            handle: *handle,
            fd,
        },
    );
}

pub(crate) fn tcp_accept_success(buf: &mut [u8], fd: u16, ep: IpEndpoint) {
    log::debug!("tcp accept: {:?}", ep);
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
        _ => {
            buf[3] = 0; // this is the invalid/error type
        }
    }
    let port = ep.port.to_le_bytes();
    buf[20] = port[0];
    buf[21] = port[1];
}
