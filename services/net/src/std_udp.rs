use crate::*;
use smoltcp::wire::{IpEndpoint, IpAddress};
use ticktimer_server::Ticktimer;

/// Overall architecture for libstd UDP implementation.
///
/// Sockets are stored in the PID/SocketHandle HashMap `process_sockets` (this is shared with TCP)
/// `recv` requests create `UpdStdState` objects, that are stored in a `udp_rx` Vec.

const BUFLEN: usize = NET_MTU as usize;

pub(crate) fn std_udp_bind(
    mut msg: xous::MessageEnvelope,
    sockets: &mut SocketSet,
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

    let udp_rx_buffer =
    UdpSocketBuffer::new(vec![UdpPacketMetadata::EMPTY], vec![0; BUFLEN]);
    let udp_tx_buffer =
        UdpSocketBuffer::new(vec![UdpPacketMetadata::EMPTY], vec![0; BUFLEN]);
    let mut udp_socket = UdpSocket::new(udp_rx_buffer, udp_tx_buffer);

    // Attempt to connect, returning the error if there is one
    if let Err(e) = udp_socket
        .bind(IpEndpoint{addr: address, port: local_port})
        .map_err(|e| match e {
            smoltcp::Error::Illegal => NetError::SocketInUse,
            smoltcp::Error::Unaddressable => NetError::Unaddressable,
            _ => NetError::LibraryError,
        })
    {
        log::trace!("couldn't connect: {:?}", e);
        std_failure(msg, e);
        return;
    }

    let handle = sockets.add(udp_socket);

    // Add the socket into our process' list of sockets, and pass the index back as the `fd` parameter for future reference.
    let idx = insert_or_append(our_sockets, handle) as u8;

    let body = msg.body.memory_message_mut().unwrap();
    let bfr = body.buf.as_slice_mut::<u8>();
    log::trace!("successfully connected: {} -> {:?}:{}", idx, address, local_port);
    bfr[0] = 0;
    bfr[1] = idx;
}

pub(crate) fn std_udp_rx(
    mut msg: xous::MessageEnvelope,
    timer: &Ticktimer,
    sockets: &mut SocketSet,
    udp_rx_waiting: &mut Vec<Option<UdpStdState>>,
    our_sockets: &Vec<Option<SocketHandle>>,
) {
    let connection_handle_index = (msg.body.id() >> 16) & 0xffff;
    let body = match msg.body.memory_message_mut() {
        Some(body) => body,
        None => {
            std_failure(msg, NetError::LibraryError);
            return;
        }
    };

    // this is not used
    body.valid = None;

    let handle = match our_sockets.get(connection_handle_index) {
        Some(Some(val)) => val,
        _ => {
            std_failure(msg, NetError::Invalid);
            return;
        }
    };

    let args = body.buf.as_slice::<u8>();
    let nonblocking = args[0] == 0;
    let expiry = if !nonblocking {
        let to = u64::from_le_bytes(args[1..9].try_into().unwrap());
        if to == 0 {
            None
        } else {
            Some(to + timer.elapsed_ms())
        }
    } else {
        None
    };
    let do_peek = body.offset.is_some();
    log::trace!("udp rx from fd {}", connection_handle_index);
    let mut socket = sockets.get::<UdpSocket>(*handle);
    if socket.can_recv() {
        log::trace!("receiving data right away");
        if do_peek {
            // have to duplicate the code because Endpoint on peek is &, but on recv is not. This
            // difference in types means you can't do a pattern match assign to a common variable.
            match socket.peek() {
                Ok((data, endpoint)) => {
                    udp_rx_success(body.buf.as_slice_mut(), data, *endpoint);
                }
                Err(e) => {
                    log::error!("unable to receive: {:?}", e);
                    std_failure(msg, NetError::LibraryError);
                }
            }
        } else {
            match socket.recv() {
                Ok((data, endpoint)) => {
                    log::debug!("immediate udp rx");
                    udp_rx_success(body.buf.as_slice_mut(), data, endpoint);
                }
                Err(e) => {
                    log::error!("unable to receive: {:?}", e);
                    std_failure(msg, NetError::LibraryError);
                }
            }
        };
        return;
    }
    if nonblocking {
        std_failure(msg, NetError::WouldBlock);
        return;
    }
    log::trace!("UDP socket was not ready to receive, adding it to list of waiting messages");

    // Adding the message to the udp_rx_waiting list prevents it from going out of scope and
    // thus prevents the .drop() method from being called. Since messages are returned to the sender
    // in the .drop() method, this keeps the caller blocked for the lifetime of the message.
    insert_or_append(
        udp_rx_waiting,
        UdpStdState {
            msg, // <-- msg is inserted into the udp_rx_waiting vector, thus preventing the lend_mut from returning.
            handle: *handle,
            expiry,
        },
    );
}

pub(crate) fn std_udp_tx(
    mut msg: xous::MessageEnvelope,
    sockets: &mut SocketSet,
    our_sockets: &Vec<Option<SocketHandle>>,
) {
    // unpack meta
    let connection_handle_index = (msg.body.id() >> 16) & 0xffff;
    let body = match msg.body.memory_message_mut() {
        Some(body) => body,
        None => {
            std_failure(msg, NetError::LibraryError);
            return;
        }
    };
    let handle = match our_sockets.get(connection_handle_index) {
        Some(Some(val)) => val,
        _ => {
            std_failure(msg, NetError::Invalid);
            return;
        }
    };

    // unpack arguments
    let bytes = body.buf.as_slice::<u8>();
    let remote_port = u16::from_le_bytes([bytes[0], bytes[1]]);
    let address = match parse_address(&bytes[2..]) {
        Some(addr) => addr,
        None => {
            log::trace!("couldn't parse address");
            std_failure(msg, NetError::LibraryError);
            return;
        }
    };
    let len = u16::from_le_bytes([bytes[19], bytes[20]]);
    // attempt the tx
    log::debug!("udp tx to fd {} -> {:?}:{} {:?}", connection_handle_index, address, remote_port, &bytes[21..21 + len as usize]);
    let mut socket = sockets.get::<UdpSocket>(*handle);
    match socket.send_slice(&bytes[21..21 + len as usize], IpEndpoint::new(address, remote_port)) {
        Ok(_) => {
            body.buf.as_slice_mut()[0] = 0;
        }
        Err(_e) => {
            // the only type of error returned from smoltcp in this case is if the destination is not addressible.
            std_failure(msg, NetError::Unaddressable);
            return;
        }
    }
}

pub(crate) fn udp_rx_success(buf: &mut [u8], rx: &[u8], ep: IpEndpoint) {
    log::debug!("udp_rx: {:?} -> {:x?}", ep, rx);
    buf[0] = 0;
    let rx_len = (rx.len() as u16).to_le_bytes();
    buf[1] = rx_len[0];
    buf[2] = rx_len[1];
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
    for (&s, d) in rx.iter().zip(buf[22..].iter_mut()) {
        *d = s;
    }
}

pub(crate) fn std_failure(mut env: xous::MessageEnvelope, code: NetError) -> Option<()> {
    log::trace!("std_failure: {:?}", code);
    // If it's not a memory message, don't fill in the return information.
    let body = match env.body.memory_message_mut() {
        None => {
            // But do respond to the scalar message, if it's a BlockingScalar
            if env.body.scalar_message().is_some() && env.body.is_blocking() {
                xous::return_scalar(env.sender, code as usize).ok();
            }
            return None;
        }
        Some(b) => b,
    };

    body.valid = None;
    let s: &mut [u8] = body.buf.as_slice_mut();
    let mut i = s.iter_mut();

    *i.next()? = 1;
    *i.next()? = code as u8;
    None
}