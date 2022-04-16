use ticktimer_server::Ticktimer;

use std::convert::TryInto;

use smoltcp::socket::SocketSet;

use core::num::NonZeroU64;
use smoltcp::socket::{
    SocketHandle, TcpSocket, TcpSocketBuffer,
};
use crate::*;

pub(crate) fn std_tcp_connect(
    mut msg: xous::MessageEnvelope,
    local_port: u16,
    sockets: &mut SocketSet,
    tcp_connect_waiting: &mut Vec<Option<(xous::MessageEnvelope, SocketHandle, u16, u16, u16)>>,
    our_sockets: &mut Vec<Option<SocketHandle>>,
) {
    // Ignore nonblocking and scalar messages
    let body = match msg.body.memory_message_mut() {
        Some(b) => b,
        None => {
            log::trace!("invalid message type");
            respond_with_error(msg, NetError::LibraryError);
            return;
        }
    };

    let bytes = body.buf.as_slice::<u8>();
    let remote_port = u16::from_le_bytes([bytes[0], bytes[1]]);
    let timeout_ms = NonZeroU64::new(u64::from_le_bytes(bytes[2..10].try_into().unwrap()));
    let address = match parse_address(&bytes[10..]) {
        Some(addr) => addr,
        None => {
            log::trace!("couldn't parse address");
            respond_with_error(msg, NetError::LibraryError);
            return;
        }
    };

    // initiates a new connection to a remote server consisting of an (Address:Port) tuple.
    // multiple connections can exist to a server, and they are further differentiated by the return port
    let mut tcp_socket = TcpSocket::new(
        TcpSocketBuffer::new(vec![0; TCP_BUFFER_SIZE]),
        TcpSocketBuffer::new(vec![0; TCP_BUFFER_SIZE]),
    );

    // Attempt to connect, returning the error if there is one
    if let Err(e) = tcp_socket
        .connect((address, remote_port), local_port)
        .map_err(|e| match e {
            smoltcp::Error::Illegal => NetError::SocketInUse,
            smoltcp::Error::Unaddressable => NetError::Unaddressable,
            _ => NetError::LibraryError,
        })
    {
        log::trace!("couldn't connect: {:?}", e);
        respond_with_error(msg, e);
        return;
    }

    tcp_socket.set_timeout(timeout_ms.map(|t| Duration::from_millis(t.get())));

    let handle = sockets.add(tcp_socket);

    // Add the socket onto the list of sockets waiting to connect, since the connection will
    // take time.
    let idx = insert_or_append(our_sockets, handle) as u16;
    insert_or_append(
        tcp_connect_waiting,
        (msg, handle, idx, local_port, remote_port),
    );
}

pub(crate) fn std_tcp_tx(
    mut msg: xous::MessageEnvelope,
    timer: &Ticktimer,
    sockets: &mut SocketSet,
    tcp_tx_waiting: &mut Vec<Option<WaitingSocket>>,
    our_sockets: &Vec<Option<SocketHandle>>,
) {
    let connection_handle_index = (msg.body.id() >> 16) & 0xffff;
    let body = match msg.body.memory_message_mut() {
        Some(body) => body,
        None => {
            respond_with_error(msg, NetError::LibraryError);
            return;
        }
    };

    let handle = match our_sockets.get(connection_handle_index) {
        Some(Some(val)) => val,
        _ => {
            respond_with_error(msg, NetError::Invalid);
            return;
        }
    };

    let mut socket = sockets.get::<TcpSocket>(*handle);
    if !socket.can_send() {
        log::trace!("tx can't send, will retry");
        let expiry = body
            .offset
            .map(|x| unsafe { NonZeroU64::new_unchecked(x.get() as u64 + timer.elapsed_ms()) });
        // Add the message to the TcpRxWaiting list, which will prevent it from getting
        // responded to right away.
        insert_or_append(
            tcp_tx_waiting,
            WaitingSocket {
                env: msg,
                handle: *handle,
                expiry,
            },
        );
        return;
    }

    // Perform the transfer
    let sent_octets = {
        let data = body.buf.as_slice::<u8>();
        let length = body
            .valid
            .map(|v| {
                if v.get() > data.len() {
                    data.len()
                } else {
                    v.get()
                }
            })
            .unwrap_or_else(|| data.len());

        match socket.send_slice(&data[..length]) {
            Ok(octets) => octets,
            Err(_) => {
                respond_with_error(msg, NetError::LibraryError);
                return;
            }
        }
    };

    log::trace!("sent {}", sent_octets);
    let response_data = body.buf.as_slice_mut::<u32>();
    if sent_octets == 0 {
        // map u32::MAX to 0, because this `valid` is a NonZeroUsize type but 0 is a valid amount of data to send
        body.valid = xous::MemorySize::new(u32::MAX as usize);
    } else {
        body.valid = xous::MemorySize::new(sent_octets);
    }
    response_data[0] = 0;
    response_data[1] = sent_octets as u32;
}

pub(crate) fn std_tcp_rx(
    mut msg: xous::MessageEnvelope,
    timer: &Ticktimer,
    sockets: &mut SocketSet,
    tcp_rx_waiting: &mut Vec<Option<WaitingSocket>>,
    our_sockets: &Vec<Option<SocketHandle>>,
) {
    let connection_handle_index = (msg.body.id() >> 16) & 0xffff;
    let body = match msg.body.memory_message_mut() {
        Some(body) => body,
        None => {
            respond_with_error(msg, NetError::LibraryError);
            return;
        }
    };

    // Default to having no valid data upon return, indicating an error
    body.valid = None;

    let handle = match our_sockets.get(connection_handle_index) {
        Some(Some(val)) => val,
        _ => {
            respond_with_error(msg, NetError::Invalid);
            return;
        }
    };

    let mut socket = sockets.get::<TcpSocket>(*handle);
    if socket.can_recv() {
        log::trace!("receiving data right away");
        match socket.recv_slice(body.buf.as_slice_mut()) {
            Ok(bytes) => {
                if bytes == 0 {
                    // it's actually valid to receive 0 bytes, but the encoding of this field doesn't allow it.
                    // so, we re-use the "MAX" value to indicate 0. I don't like this, but the other option is to
                    // bit-stuff a length field at the front of the body.buf slice, and that's also ugly for other reasons.
                    // u32::MAX is a fairly "safe" number to use, because it's well beyond the maximum length
                    // of a TCP packet (64k)
                    body.valid = xous::MemorySize::new(u32::MAX as usize);
                } else {
                    body.valid = xous::MemorySize::new(bytes);
                }
                log::trace!("set body.valid = {:?}", body.valid);
            }
            Err(e) => {
                log::error!("unable to receive: {:?}", e);
                respond_with_error(msg, NetError::LibraryError);
            }
        }
        return;
    }

    log::trace!("socket was not able to receive, adding it to list of waiting messages");

    // Add the message to the TcpRxWaiting list, which will prevent it from getting
    // responded to right away.
    let expiry = body
        .offset
        .map(|x| unsafe { NonZeroU64::new_unchecked(x.get() as u64 + timer.elapsed_ms()) });
    insert_or_append(
        tcp_rx_waiting,
        WaitingSocket {
            env: msg,
            handle: *handle,
            expiry,
        },
    );
}

pub(crate) fn std_tcp_peek(
    mut msg: xous::MessageEnvelope,
    sockets: &mut SocketSet,
    our_sockets: &Vec<Option<SocketHandle>>,
) {
    let connection_handle_index = (msg.body.id() >> 16) & 0xffff;
    let body = match msg.body.memory_message_mut() {
        Some(body) => body,
        None => {
            respond_with_error(msg, NetError::LibraryError);
            return;
        }
    };

    // Default to having no valid data upon return, indicating an error
    body.valid = None;

    let handle = match our_sockets.get(connection_handle_index) {
        Some(Some(val)) => val,
        _ => {
            respond_with_error(msg, NetError::Invalid);
            return;
        }
    };

    let mut socket = sockets.get::<TcpSocket>(*handle);
    if socket.can_recv() {
        log::trace!("receiving data right away");
        match socket.peek_slice(body.buf.as_slice_mut()) {
            Ok(bytes) => {
                if bytes == 0 {
                    // it's actually valid to receive 0 bytes, but the encoding of this field doesn't allow it.
                    // so, we re-use the "MAX" value to indicate 0. I don't like this, but the other option is to
                    // bit-stuff a length field at the front of the body.buf slice, and that's also ugly for other reasons.
                    // u32::MAX is a fairly "safe" number to use, because it's well beyond the maximum length
                    // of a TCP packet (64k)
                    body.valid = xous::MemorySize::new(u32::MAX as usize);
                } else {
                    body.valid = xous::MemorySize::new(bytes);
                }
                log::trace!("set body.valid = {:?}", body.valid);
            }
            Err(e) => {
                log::error!("unable to receive: {:?}", e);
                respond_with_error(msg, NetError::LibraryError);
            }
        }
    } else {
        // No data available, so indicate `None`
        body.valid = None;
        // Also indicate no error
        body.buf.as_slice_mut::<u32>()[0] = 0;
    }
}