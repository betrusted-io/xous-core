use num_traits::*;
use xous::{msg_blocking_scalar_unpack, msg_scalar_unpack, MessageSender, CID};
use xous_ipc::Buffer;

use crate::api;
use crate::api::keyboard::*;

pub fn start_keyboard_service() {
    std::thread::spawn(move || {
        keyboard_service();
    });
}

fn keyboard_service() {
    let xns = xous_names::XousNames::new().unwrap();
    let kbd_sid = xns.register_name(api::SERVER_NAME_KBD, None).expect("can't register server");

    let mut listener_conn: Option<CID> = None;
    let mut listener_op: Option<usize> = None;
    let mut observer_conn: Option<CID> = None;
    let mut observer_op: Option<usize> = None;

    let mut esc_index: Option<usize> = None;
    let mut esc_chars = [0u8; 16];
    // storage for any blocking listeners
    let mut blocking_listener = Vec::<MessageSender>::new();

    loop {
        let msg = xous::receive_message(kbd_sid).unwrap(); // this blocks until we get a message
        let op = FromPrimitive::from_usize(msg.body.id());
        log::debug!("{:?}", op);
        match op {
            Some(KeyboardOpcode::BlockingKeyListener) => {
                blocking_listener.push(msg.sender);
            }
            Some(KeyboardOpcode::RegisterListener) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let kr = buffer.as_flat::<KeyboardRegistration, _>().unwrap();
                match xns.request_connection_blocking(kr.server_name.as_str()) {
                    Ok(cid) => {
                        listener_conn = Some(cid);
                        listener_op = Some(kr.listener_op_id as usize);
                    }
                    Err(e) => {
                        log::error!("couldn't connect to listener: {:?}", e);
                        listener_conn = None;
                        listener_op = None;
                    }
                }
            }
            Some(KeyboardOpcode::RegisterKeyObserver) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let kr = buffer.as_flat::<KeyboardRegistration, _>().unwrap();
                if observer_conn.is_none() {
                    match xns.request_connection_blocking(kr.server_name.as_str()) {
                        Ok(cid) => {
                            observer_conn = Some(cid);
                            observer_op = Some(kr.listener_op_id as usize);
                        }
                        Err(e) => {
                            log::error!("couldn't connect to observer: {:?}", e);
                            observer_conn = None;
                            observer_op = None;
                        }
                    }
                }
            }
            Some(KeyboardOpcode::SelectKeyMap) => {
                todo!();
            }
            Some(KeyboardOpcode::GetKeyMap) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                todo!();
            }),
            Some(KeyboardOpcode::SetRepeat) => msg_scalar_unpack!(msg, _rate, _delay, _, _, {
                todo!();
            }),
            Some(KeyboardOpcode::SetChordInterval) => msg_scalar_unpack!(msg, _delay, _, _, _, {
                todo!();
            }),
            Some(KeyboardOpcode::InjectKey) => msg_scalar_unpack!(msg, k, _, _, _, {
                // key substitutions to help things work better
                // 1b5b317e = home
                // 1b5b44 = left
                // 1b5b43 = right
                // 1b5b41 = up
                // 1b5b42 = down
                let key = match esc_index {
                    Some(i) => {
                        esc_chars[i] = (k & 0xff) as u8;
                        match esc_match(&esc_chars[..i + 1]) {
                            Ok(m) => {
                                if let Some(code) = m {
                                    // Ok(Some(code)) is a character found
                                    esc_chars = [0u8; 16];
                                    esc_index = None;
                                    code
                                } else {
                                    // Ok(None) means we're still accumulating characters
                                    if i + 1 < esc_chars.len() {
                                        esc_index = Some(i + 1);
                                    } else {
                                        esc_index = None;
                                        esc_chars = [0u8; 16];
                                    }
                                    '\u{0000}'
                                }
                            }
                            // invalid sequence encountered, abort
                            Err(_) => {
                                log::warn!("Unhandled escape sequence: {:x?}", &esc_chars[..i + 1]);
                                esc_chars = [0u8; 16];
                                esc_index = None;
                                '\u{0000}'
                            }
                        }
                    }
                    _ => {
                        if k == 0x1b {
                            esc_index = Some(1);
                            esc_chars = [0u8; 16]; // clear the full search array with every escape sequence init
                            esc_chars[0] = 0x1b;
                            '\u{0000}'
                        } else {
                            let bs_del_fix = if k == 0x7f { 0x08 } else { k };
                            core::char::from_u32(bs_del_fix as u32).unwrap_or('\u{0000}')
                        }
                    }
                };

                if let Some(conn) = listener_conn {
                    if key != '\u{0000}' {
                        if key >= '\u{f700}' && key <= '\u{f8ff}' {
                            log::info!("ignoring key '{}'({:x})", key, key as u32); // ignore Apple PUA characters
                        } else {
                            log::info!("injecting key '{}'({:x})", key, key as u32); // always be noisy about this, it's an exploit path
                            xous::try_send_message(
                                conn,
                                xous::Message::new_scalar(
                                    listener_op.unwrap(),
                                    key as u32 as usize,
                                    '\u{0000}' as u32 as usize,
                                    '\u{0000}' as u32 as usize,
                                    '\u{0000}' as u32 as usize,
                                ),
                            )
                            .unwrap_or_else(|_| {
                                log::info!("Input overflow, dropping keys!");
                                xous::Result::Ok
                            });
                        }
                    }
                }

                if observer_conn.is_some() && observer_op.is_some() {
                    log::trace!("sending observer key");
                    xous::try_send_message(
                        observer_conn.unwrap(),
                        xous::Message::new_scalar(observer_op.unwrap(), 0, 0, 0, 0),
                    )
                    .ok();
                }

                for listener in blocking_listener.drain(..) {
                    // we must unblock anyways once the key is hit; even if the key is invalid,
                    // send the invalid key. The receiving library function will clean this up into a
                    // nil-response vector.
                    xous::return_scalar2(listener, key as u32 as usize, 0).unwrap();
                }
            }),
            Some(KeyboardOpcode::HandlerTrigger) => {
                todo!("Write this once we have an IRQ handler for keyboard interrupts");
            }
            None => {
                log::error!("couldn't convert KeyboardOpcode");
                break;
            }
        }
    }
    xns.unregister_server(kbd_sid).unwrap();
    xous::destroy_server(kbd_sid).unwrap();
    xous::terminate_process(0)
}

#[cfg(not(feature = "rawserial"))]
fn esc_match(esc_chars: &[u8]) -> Result<Option<char>, ()> {
    let mut extended = Vec::<u8>::new();
    for (i, &c) in esc_chars.iter().enumerate() {
        match i {
            0 => {
                if c != 0x1b {
                    return Err(());
                }
            }
            1 => {
                if c != 0x5b {
                    return Err(());
                }
            }
            2 => match c {
                0x41 => return Ok(Some('↑')),
                0x42 => return Ok(Some('↓')),
                0x43 => return Ok(Some('→')),
                0x44 => return Ok(Some('←')),
                0x7e => return Err(()), // premature end
                _ => extended.push(c),
            },
            _ => {
                if c == 0x7e {
                    if extended.len() == 1 {
                        if extended[0] == 0x31 {
                            return Ok(Some('∴'));
                        }
                    } else {
                        return Err(()); // code unrecognized
                    }
                } else {
                    extended.push(c)
                }
            }
        }
    }
    Ok(None)
}
