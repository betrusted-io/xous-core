use bao1x_api::keyboard::*;
use bao1x_hal::board::KeyPress;
use bao1x_hal::kpc_aoint::AoIntStatus;
use num_traits::*;
use utralib::*;
use xous::{CID, MessageSender, msg_blocking_scalar_unpack, msg_scalar_unpack};
use xous_ipc::Buffer;

const KEYUP_DELAY_MS: u64 = 50;

struct KeypressTimestamp {
    pub kp: KeyPress,
    pub next_repeat_time: u64,
}
struct KeyTracker {
    pub rate_ms: usize,
    pub delay_ms: usize,
    pub keys: Vec<KeypressTimestamp>,
}
impl KeyTracker {
    pub fn new() -> Self { KeyTracker { rate_ms: KEYUP_DELAY_MS as usize, delay_ms: 500, keys: Vec::new() } }

    pub fn register_key_down(&mut self, key: KeyPress, ts: u64) {
        if let Some(entry) = self.keys.iter_mut().find(|e| e.kp == key) {
            // update the existing entry if it exists
            entry.next_repeat_time = ts + self.delay_ms as u64;
        } else {
            self.keys.push(KeypressTimestamp { kp: key, next_repeat_time: ts + self.delay_ms as u64 });
        }
    }

    /// Processes the current keys pressed, at the current time stamp
    pub fn update_key_down(&mut self, keys: &[KeyPress], now: u64) {
        // Handle the case that key A is held down, then key B is held down simultaneously,
        // then key A is released while B continues to hold down
        // then key A is pressed again all while key B is held down
        //
        // The behavior in this case is that A should emit a keydown immediately and will also
        // immediately repeat without going through the repeat delay. This is consistent with a
        // "video gaming" mode of operation.
        //
        // Search for keys that are pressed, but not currently in the pressed array
        // trigger an effective key report by setting its timestamp to now minus the repeat delay
        for key in keys {
            // search for key not in keys vector; create a new entry with next repeat equal
            // to now, so it gets emitted immediately
            if !self.keys.iter().any(|e| e.kp == *key) {
                self.keys.push(KeypressTimestamp { kp: *key, next_repeat_time: now });
            }
        }

        // Handle the case that key A is held down, then key B is held down simultaneously.
        // Then, key A is released.
        //
        // This is done after all current keys are registered above.
        //
        // Each key currently pressed should be present in `self.keys` vector.
        // If a currently pressed key is not in the `keys` input slice,
        // delete it from the `self.keys` vector. This will prevent it from continuing to generate
        // key presses on get_repeats()
        self.keys.retain(|entry| keys.iter().any(|key| key == &entry.kp));
    }

    /// Returns any keys that should have repeat key-press events generated
    pub fn get_repeats(&mut self, now: u64) -> Vec<char> {
        let mut kps = Vec::new();
        for key in self.keys.iter_mut() {
            if now >= key.next_repeat_time {
                kps.push(map_keypress(key.kp));
                // rebasing off of now prevents keys from "lagging on" in case of UI delay
                key.next_repeat_time = now + self.rate_ms as u64;
            }
        }
        kps
    }

    pub fn keys_pressed(&self) -> usize { self.keys.len() }

    pub fn clear_keys(&mut self) { self.keys.clear(); }
}

fn map_keypress(kp: KeyPress) -> char {
    match kp {
        KeyPress::Down => '↓',
        KeyPress::Up => '↑',
        KeyPress::Left => '←',
        KeyPress::Right => '→',
        KeyPress::Select => '∴',
        KeyPress::Center => '\r',
        _ => '\u{0000}',
    }
}

pub fn start_keyboard_service(dkpc_ptr: usize, irq_ptr: usize, ao_ptr: usize) {
    std::thread::spawn(move || {
        keyboard_service(dkpc_ptr, irq_ptr, ao_ptr);
    });
    std::thread::spawn(move || {
        keyboard_bouncer();
    });
}

fn keyboard_bouncer() {
    // private server that has no dependencies but a "well-known-name" for the log server
    // to forward keystrokes into.
    let sid = xous::create_server_with_address(b"keyboard_bouncer")
        .expect("couldn't create keyboard log bounce server");
    let xns = xous_names::XousNames::new().unwrap();
    let kbd = bao1x_api::keyboard::Keyboard::new(&xns).unwrap();
    let mut msg_opt = None;
    loop {
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        let msg = msg_opt.as_mut().unwrap();
        // only one type of message is expected
        match msg.body.scalar_message() {
            Some(m) => {
                if let Some(c) = char::from_u32(m.arg1 as u32) {
                    kbd.inject_key(c);
                }
            }
            _ => {}
        }
    }
}

fn keyboard_service(dkpc_ptr: usize, irq_ptr: usize, ao_ptr: usize) {
    let dkpc = CSR::new(dkpc_ptr as *mut u32);
    let mut ao = CSR::new(ao_ptr as *mut u32);
    let _irq = CSR::new(irq_ptr as *mut u32);

    let xns = xous_names::XousNames::new().unwrap();
    let kbd_sid = xns.register_name(bao1x_api::SERVER_NAME_KBD, None).expect("can't register server");
    let tt = ticktimer::Ticktimer::new().unwrap();

    let mut listeners: Vec<(CID, usize)> = Vec::new();
    let mut observer_conn: Option<CID> = None;
    let mut observer_op: Option<usize> = None;

    let mut esc_index: Option<usize> = None;
    let mut esc_chars = [0u8; 16];
    // storage for any blocking listeners
    let mut blocking_listener = Vec::<MessageSender>::new();

    let iox = crate::iox::IoxHal::new();
    let (_row, _col) = bao1x_hal::board::setup_kpc_pins(&iox);

    #[cfg(feature = "keyboard-testing")]
    // this routine is useful for mapping out raw keys on new hardware builds
    std::thread::spawn(move || {
        let dkpc = CSR::new(dkpc_ptr as *mut u32);
        let mut irq = CSR::new(irq_ptr as *mut u32);
        let mut ao = CSR::new(ao_ptr as *mut u32);
        let tt = ticktimer::Ticktimer::new().unwrap();
        loop {
            tt.sleep_ms(1000);
            for i in (0..6).chain(12..13).chain(8..9) {
                log::info!("{:x}: {:x} ", i * 4, unsafe { dkpc.base().add(i).read_volatile() });
            }
            let fr = AoIntStatus::new_with_raw_value(ao.r(utra::ao_sysctrl::SFR_AOFR));
            let pending = irq.r(utralib::utra::irqarray2::EV_PENDING);
            log::info!(
                "int: {:x}/{:x}/{:x}/{:x?}",
                irq.r(utralib::utra::irqarray2::EV_ENABLE),
                pending,
                irq.r(utralib::utra::irqarray2::EV_STATUS),
                fr,
            );
            ao.wo(utra::ao_sysctrl::SFR_AOFR, fr.raw_value());
            irq.wo(utra::irqarray2::EV_PENDING, pending);
        }
    });

    let mut key_tracker = KeyTracker::new();
    let mut last_key_event = 0u64;
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
                        listeners.push((cid, <u32 as From<u32>>::from(kr.listener_op_id.into()) as usize));
                    }
                    Err(e) => {
                        log::error!("couldn't connect to listener: {:?}", e);
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
                            observer_op = Some(<u32 as From<u32>>::from(kr.listener_op_id.into()) as usize);
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
                // only one key map for the input keyboard. Key mapping for translation of
                // key presess to USB key codes should be set in the USB stack, not here.
                unimplemented!()
            }
            Some(KeyboardOpcode::GetKeyMap) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                log::warn!("Defaulting to DVORAK map");
                xous::return_scalar(msg.sender, KeyMap::Dvorak.into()).expect("can't retrieve keymap");
            }),
            Some(KeyboardOpcode::SetRepeat) => msg_scalar_unpack!(msg, rate, delay, _, _, {
                key_tracker.rate_ms = rate;
                key_tracker.delay_ms = delay;
            }),
            Some(KeyboardOpcode::SetChordInterval) => msg_scalar_unpack!(msg, _delay, _, _, _, {
                // chording allows us to interpret multiple key hits as a whole new separate key.
                // for now we don't implement this feature.
                unimplemented!()
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

                for &(conn, listener_op) in listeners.iter() {
                    if key != '\u{0000}' {
                        if key >= '\u{f700}' && key <= '\u{f8ff}' {
                            log::info!("ignoring key '{}'({:x})", key, key as u32); // ignore Apple PUA characters
                        } else {
                            log::debug!("injecting key '{}'({:x}) to {}", key, key as u32, conn); // always be noisy about this, it's an exploit path
                            xous::try_send_message(
                                conn,
                                xous::Message::new_scalar(
                                    listener_op,
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
            Some(KeyboardOpcode::HandlerTrigger) => msg_scalar_unpack!(msg, pending, _, _, _, {
                if pending & (1 << bao1x_hal::kpc_aoint::IrqMapping::AoInt as usize) != 0 {
                    let mut kc: Vec<char> = Vec::new();
                    let now = tt.elapsed_ms();
                    if now - last_key_event > KEYUP_DELAY_MS {
                        key_tracker.clear_keys();
                    }
                    last_key_event = now;

                    // key downs come from this register
                    if dkpc.r(utra::dkpc::SFR_SR1) != 0 {
                        let sr1 = unsafe { dkpc.base().add(8).read_volatile() };
                        let key_down = bao1x_hal::board::kpc_sr1_to_key(sr1);
                        key_tracker.register_key_down(key_down, now);
                        if key_down != KeyPress::Invalid {
                            kc.push(map_keypress(key_down))
                        }
                    }
                    // the keys_pressed() check is necessary because the interrupt will fire *before* a key
                    // press is effectively registered. This is because the interrupt fires as soon as any
                    // noise is detected on the keyboard, *before* the hardware debounce
                    // happens!
                    if key_tracker.keys_pressed() > 0 {
                        let sr0 = dkpc.r(utra::dkpc::SFR_SR0);
                        let keys_down = bao1x_hal::board::kpc_sr0_to_key(sr0);
                        key_tracker.update_key_down(&keys_down, now);
                    }

                    // clear the FR bits
                    let fr = AoIntStatus::new_with_raw_value(ao.r(utra::ao_sysctrl::SFR_AOFR));
                    ao.wo(utra::ao_sysctrl::SFR_AOFR, fr.raw_value());

                    // add any repeat keys to the key response array
                    kc.extend_from_slice(&key_tracker.get_repeats(now));

                    // strip out any null entries that were generated
                    kc.retain(|&c| c != '\u{0000}');

                    // send keys, if any
                    // handle the blocking listeners
                    if kc.len() > 0 {
                        for listener in blocking_listener.drain(..) {
                            xous::return_scalar2(
                                listener,
                                if kc.len() >= 1 { kc[0] as u32 as usize } else { 0 },
                                if kc.len() >= 2 { kc[1] as u32 as usize } else { 0 },
                            )
                            .unwrap();
                            if kc.len() > 2 {
                                log::warn!(
                                    "Extra keys in multi-hit event went unreported: only 2 of {} total keys reported out of {:?}",
                                    kc.len(),
                                    &kc,
                                );
                            }
                        }
                    }
                    // handle the true async listeners
                    for kv in kc.chunks(4) {
                        let mut keys: [char; 4] = ['\u{0000}', '\u{0000}', '\u{0000}', '\u{0000}'];
                        for i in 0..kv.len() {
                            keys[i] = kv[i];
                        }
                        log::trace!("sending keys {:?}", keys);
                        for &(listener_conn, listener_op) in listeners.iter() {
                            xous::try_send_message(
                                listener_conn,
                                xous::Message::new_scalar(
                                    listener_op,
                                    keys[0] as u32 as usize,
                                    keys[1] as u32 as usize,
                                    keys[2] as u32 as usize,
                                    keys[3] as u32 as usize,
                                ),
                            )
                            .ok();
                        }
                    }
                } else {
                    log::warn!("Unhandled interrupt: {:x}", pending);
                }
            }),
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
                match c {
                    0x41 => return Ok(Some('↑')),
                    0x42 => return Ok(Some('↓')),
                    0x43 => return Ok(Some('→')),
                    0x44 => return Ok(Some('←')),
                    0x7e => return Err(()), // premature end
                    _ => {
                        if c != 0x5b {
                            return Err(());
                        }
                    }
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
