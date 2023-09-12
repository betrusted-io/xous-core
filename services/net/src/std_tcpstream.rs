use ticktimer_server::Ticktimer;

use std::convert::TryInto;

use core::num::NonZeroU64;
use smoltcp::socket::{
    TcpSocket, TcpSocketBuffer,
};
use smoltcp::iface::{Interface, SocketHandle};
use crate::*;
use crate::device::NetPhy;

pub(crate) fn std_tcp_connect(
    mut msg: xous::MessageEnvelope,
    local_port: u16,
    iface: &mut Interface::<NetPhy>,
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

    let bytes = unsafe { body.buf.as_slice::<u8>() };
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
    let tcp_socket = TcpSocket::new(
        TcpSocketBuffer::new(vec![0; TCP_BUFFER_SIZE]),
        TcpSocketBuffer::new(vec![0; TCP_BUFFER_SIZE]),
    );
    let handle = iface.add_socket(tcp_socket);
    let (tcp_socket, cx) = iface.get_socket_and_context::<TcpSocket>(handle);

    // Attempt to connect, returning the error if there is one
    if let Err(e) = tcp_socket
        .connect(cx, (address, remote_port), local_port)
        .map_err(|e| match e {
            smoltcp::Error::Illegal => NetError::SocketInUse,
            smoltcp::Error::Unaddressable => NetError::Unaddressable,
            _ => NetError::LibraryError,
        })
    {
        log::debug!("couldn't connect: {:?}", e);
        respond_with_error(msg, e);
        return;
    }

    tcp_socket.set_timeout(timeout_ms.map(|t| Duration::from_millis(t.get())));

    // Add the socket onto the list of sockets waiting to connect, since the connection will
    // take time.
    let idx = insert_or_append(our_sockets, handle) as u16;
    insert_or_append(
        tcp_connect_waiting,
        (msg, handle, idx, local_port, remote_port),
    );
    log::debug!("connect waiting now: {}, {:?} {:?} {:?}", idx, handle, local_port, remote_port);
}

pub(crate) fn std_tcp_tx(
    mut msg: xous::MessageEnvelope,
    timer: &Ticktimer,
    iface: &mut Interface::<NetPhy>,
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

    let socket = iface.get_socket::<TcpSocket>(*handle);
    // handle the case that the connection closed due to the receiver quitting
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
        let data = unsafe { body.buf.as_slice::<u8>() };
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
    let response_data = unsafe { body.buf.as_slice_mut::<u32>() };
    response_data[0] = 0;
    response_data[1] = sent_octets as u32;
}

pub(crate) fn std_tcp_rx(
    mut msg: xous::MessageEnvelope,
    timer: &Ticktimer,
    iface: &mut Interface::<NetPhy>,
    tcp_rx_waiting: &mut Vec<Option<WaitingSocket>>,
    our_sockets: &Vec<Option<SocketHandle>>,
    nonblocking: bool,
) {
    let connection_handle_index = (msg.body.id() >> 16) & 0xffff;
    let body = match msg.body.memory_message_mut() {
        Some(body) => body,
        None => {
            respond_with_error(msg, NetError::LibraryError);
            return;
        }
    };
    let expiry = body
        .offset
        .map(|x| unsafe { NonZeroU64::new_unchecked(x.get() as u64 + timer.elapsed_ms()) });

    // Offset is used as a flag to indicate an error. `None` means an error occured. `Some` means no error.
    body.offset = None;

    let handle = match our_sockets.get(connection_handle_index) {
        Some(Some(val)) => val,
        _ => {
            respond_with_error(msg, NetError::Invalid);
            return;
        }
    };

    let socket = iface.get_socket::<TcpSocket>(*handle);
    if socket.can_recv() {
        log::debug!("receiving data right away");
        let buflen = if let Some(valid) = body.valid {
            valid.get()
        } else {
            0
        };
        match socket.recv_slice(unsafe { &mut body.buf.as_slice_mut()[..buflen] }) {
            Ok(bytes) => {
                // it's actually valid to receive 0 bytes, but the encoding of this field doesn't allow it.
                // so, `None` is abused to represent the value of "0" bytes, which is what is naturally returned
                // as the "error" when you try to create a NonZeroUsize with 0.
                body.valid = xous::MemorySize::new(bytes);
                body.offset = xous::MemoryAddress::new(1);
                log::debug!("set body.valid = {:?}", body.valid);
            }
            Err(e) => {
                log::error!("unable to receive: {:?}", e);
                respond_with_error(msg, NetError::LibraryError);
            }
        }
        return;
    }
    if nonblocking {
        respond_with_error(msg, NetError::WouldBlock);
        return;
    }

    log::debug!("socket was not able to receive, adding it to list of waiting messages");

    // Add the message to the TcpRxWaiting list, which will prevent it from getting
    // responded to right away.
    if expiry.is_some() {
        log::debug!("read with timeout set: {:?}", expiry);
    }
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
    timer: &Ticktimer,
    iface: &mut Interface::<NetPhy>,
    our_sockets: &Vec<Option<SocketHandle>>,
    tcp_peek_waiting: &mut Vec<Option<WaitingSocket>>,
    nonblocking: bool,
) {
    let connection_handle_index = (msg.body.id() >> 16) & 0xffff;
    let body = match msg.body.memory_message_mut() {
        Some(body) => body,
        None => {
            respond_with_error(msg, NetError::LibraryError);
            return;
        }
    };
    let expiry = body
    .offset
    .map(|x| unsafe { NonZeroU64::new_unchecked(x.get() as u64 + timer.elapsed_ms()) });

    // Offset is used to indicate an error. None=>Error, Some=>no error
    body.offset = None;

    let handle = match our_sockets.get(connection_handle_index) {
        Some(Some(val)) => val,
        _ => {
            respond_with_error(msg, NetError::Invalid);
            return;
        }
    };

    let socket = iface.get_socket::<TcpSocket>(*handle);
    if socket.can_recv() {
        log::debug!("peeking data right away");
        let buflen = if let Some(valid) = body.valid {
            valid.get()
        } else {
            0
        };
        match socket.peek_slice(unsafe { &mut body.buf.as_slice_mut()[..buflen] }) {
            Ok(bytes) => {
                // it's actually valid to receive 0 bytes, but the encoding of this field doesn't allow it.
                // so, `None` is abused to represent the value of "0" bytes, which is what is naturally returned
                // as the "error" when you try to create a NonZeroUsize with 0.
                body.valid = xous::MemorySize::new(bytes);
                body.offset = xous::MemoryAddress::new(1);
                log::trace!("set body.valid = {:?}", body.valid);
            }
            Err(e) => {
                log::error!("unable to receive: {:?}", e);
                respond_with_error(msg, NetError::LibraryError);
            }
        }
    } else {
        if nonblocking {
            respond_with_error(msg, NetError::WouldBlock);
        } else {
            // Add the message to the TcpRxWaiting list, which will prevent it from getting
            // responded to right away.
            insert_or_append(
                tcp_peek_waiting,
                WaitingSocket {
                    env: msg,
                    handle: *handle,
                    expiry,
                },
            );
        }
    }
}

pub(crate) fn std_tcp_can_close(tx_waiting: &Vec<Option<WaitingSocket>>, handle: SocketHandle) -> bool {
    for maybe_socket in tx_waiting.iter() {
        if let Some(socket) = maybe_socket {
            if socket.handle == handle {
                return false
            }
        }
    }
    true
}
