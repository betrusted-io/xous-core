use smoltcp::wire::IpAddress;
use crate::*;


pub(crate) fn parse_address(data: &[u8]) -> Option<smoltcp::wire::IpAddress> {
    let mut i = data.iter();
    match i.next() {
        Some(&4) => Some(smoltcp::wire::IpAddress::v4(
            *i.next()?,
            *i.next()?,
            *i.next()?,
            *i.next()?,
        )),
        Some(&6) => {
            let mut new_addr = [0u8; 16];
            for octet in new_addr.iter_mut() {
                *octet = *i.next()?;
            }
            let v6: std::net::Ipv6Addr = new_addr.into();
            Some(v6.into())
        }
        _ => None,
    }
}

pub(crate) fn write_address(address: IpAddress, data: &mut [u8]) -> Option<usize> {
    let mut i = data.iter_mut();
    match address {
        IpAddress::Ipv4(a) => {
            *i.next()? = 4;
            for (dest, src) in i.zip(a.as_bytes().iter()) {
                *dest = *src;
            }
            Some(5)
        }
        IpAddress::Ipv6(a) => {
            *i.next()? = 6;
            for (dest, src) in i.zip(a.as_bytes().iter()) {
                *dest = *src;
            }
            Some(16)
        }
    }
}

pub(crate) fn respond_with_error(mut env: xous::MessageEnvelope, code: NetError) -> Option<()> {
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
    let s: &mut [u8] = unsafe { body.buf.as_slice_mut() };
    let mut i = s.iter_mut();

    // Duplicate error to ensure it's seen as an error regardless of byte order/return type
    // This is necessary because errors are encoded as `u8` slices, but "good"
    // responses may be encoded as `u16` or `u32` slices.
    *i.next()? = 1;
    *i.next()? = 1;
    *i.next()? = 1;
    *i.next()? = 1;
    *i.next()? = code as u8;
    *i.next()? = 0;
    *i.next()? = 0;
    *i.next()? = 0;
    None
}

pub(crate) fn respond_with_connected(
    mut env: xous::MessageEnvelope,
    idx: u16,
    local_port: u16,
    remote_port: u16,
) {
    let body = env.body.memory_message_mut().unwrap();
    let bfr = unsafe { body.buf.as_slice_mut::<u16>() };

    log::debug!("successfully connected: {}", idx);
    bfr[0] = 0;
    bfr[1] = idx;
    bfr[2] = local_port;
    bfr[3] = remote_port;
}

/// Insert `Some(value)` into the first slot in the Vec that is `None`,
/// or append it to the end if there is no free slot
pub(crate) fn insert_or_append<T>(arr: &mut Vec<Option<T>>, val: T) -> usize {
    // Look for a free index, or add it onto the end.
    let mut idx = None;
    for (i, elem) in arr.iter_mut().enumerate() {
        if elem.is_none() {
            idx = Some(i);
            break;
        }
    }
    if let Some(idx) = idx {
        arr[idx] = Some(val);
        idx
    } else {
        let idx = arr.len();
        arr.push(Some(val));
        idx
    }
}
