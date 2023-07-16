#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;
mod mappings;

use log::info;

use num_traits::*;
use xous_ipc::Buffer;
use xous::{CID, msg_scalar_unpack, msg_blocking_scalar_unpack, MessageSender};
#[cfg(feature="rawserial")]
use std::collections::VecDeque;
#[cfg(feature="rawserial")]
const BLOCKING_QUEUE_LEN: usize = 128;

#[cfg(any(feature="precursor", feature="renode"))]
mod implementation {
    use utralib::generated::*;
    use crate::{RowCol, KeyRawStates, api::*};
    use crate::mappings::*;
    use ticktimer_server::Ticktimer;
    use xous::CID;
    use num_traits::ToPrimitive;
    use susres::{RegManager, RegOrField, SuspendResume};
    use std::collections::HashSet;

    /// note: the code is structured to use at most 16 rows or 16 cols
    const KBD_ROWS: usize = 9;
    const KBD_COLS: usize = 10;

    pub(crate) struct Keyboard {
        conn: CID,
        csr: utralib::CSR<u32>,
        /// where the interrupt handler copies the new state
        new_state: HashSet::<RowCol>,
        /// remember the last key states
        last_state: HashSet::<RowCol>,
        /// connection to the timer for real-time events
        ticktimer: Ticktimer,
        /// mapping for ScanCode translation
        map: KeyMap,
        /// delay in ms before a key is considered to be repeating
        delay: u32,
        /// rate in ms for repeating a key
        rate: u32,
        /// shift key state
        shift_down: bool,
        shift_up: bool,
        /// alt key state
        alt_down: bool,
        alt_up: bool,
        /// timestamp to track repeating key interval
        rate_timestamp: u64,
        /// track the last key held down, which lacks a hold alternate meaning, for repeating
        repeating_key: Option<char>,
        /// timestamp timekeeper for chording / hold key
        chord_timestamp: u64,
        /// chording sample interval
        chord_interval: u32,
        /// chord state array
        chord: [[bool; KBD_COLS]; KBD_ROWS],
        /// memoize number of keys that have been pressed
        chord_active: u32,
        /// indicate if the chord has been captured. Once captured, further presses are ignored, until all keys are let up.
        chord_captured: bool,
        susres: RegManager::<{utra::keyboard::KEYBOARD_NUMREGS}>,
        /// a field used for debugging various keyboard issues, especially with the interrupt handler
        pub debug: usize,
        /// access to settings stored in the first few sectors of FLASH
        early_settings: early_settings::EarlySettings,
    }

    fn handle_kbd(_irq_no: usize, arg: *mut usize) {
        let kbd = unsafe { &mut *(arg as *mut Keyboard) };
        let pending = kbd.csr.r(utra::keyboard::EV_PENDING);
        if kbd.csr.rf(utra::keyboard::EV_PENDING_KEYPRESSED) != 0 {
            // scan the entire key matrix and return the list of keys that are currently
            // pressed as key codes. If we missed an interrupt, then we missed the key...
            kbd.new_state.clear();
            for r in 0..KBD_ROWS {
                let cols: u16 = kbd_getrow(kbd, r as u8);
                if cols != 0 {
                    for c in 0..KBD_COLS {
                        if (cols & (1 << c)) != 0 {
                            kbd.new_state.insert(
                                RowCol{r: r as _, c: c as _}
                            );
                        }
                    }
                }
            }
            xous::try_send_message(kbd.conn,
                xous::Message::new_scalar(Opcode::HandlerTrigger.to_usize().unwrap(), 0, 0, 0, 0)).ok();
        }
        if kbd.csr.rf(utra::keyboard::EV_PENDING_INJECT) != 0 {
            loop {
                let char_reg = kbd.csr.r(utra::keyboard::UART_CHAR);
                if char_reg & kbd.csr.ms(utra::keyboard::UART_CHAR_STB, 1) != 0 {
                    xous::try_send_message(kbd.conn,
                        xous::Message::new_scalar(Opcode::InjectKey.to_usize().unwrap(), (char_reg & 0xff) as _, 0, 0, 0)
                    ).ok();
                    kbd.debug += 1;
                } else {
                    break;
                }
            }
        }
        kbd.csr.wo(utra::keyboard::EV_PENDING, pending);
    }
    /// get the column activation contents of the given row
    /// row is coded as a binary number, so the result of kbd_rowchange has to be decoded from a binary
    /// vector of rows to a set of numbers prior to using this function
    fn kbd_getrow(kbd: &Keyboard, row: u8) -> u16 {
        match row {
            0 => kbd.csr.rf(utra::keyboard::ROW0DAT_ROW0DAT) as u16,
            1 => kbd.csr.rf(utra::keyboard::ROW1DAT_ROW1DAT) as u16,
            2 => kbd.csr.rf(utra::keyboard::ROW2DAT_ROW2DAT) as u16,
            3 => kbd.csr.rf(utra::keyboard::ROW3DAT_ROW3DAT) as u16,
            4 => kbd.csr.rf(utra::keyboard::ROW4DAT_ROW4DAT) as u16,
            5 => kbd.csr.rf(utra::keyboard::ROW5DAT_ROW5DAT) as u16,
            6 => kbd.csr.rf(utra::keyboard::ROW6DAT_ROW6DAT) as u16,
            7 => kbd.csr.rf(utra::keyboard::ROW7DAT_ROW7DAT) as u16,
            8 => kbd.csr.rf(utra::keyboard::ROW8DAT_ROW8DAT) as u16,
            _ => 0
        }
    }

    impl Keyboard {
        pub(crate) fn new(sid: xous::SID) -> Keyboard {
            let csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::keyboard::HW_KEYBOARD_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map Keyboard CSR range");

            let ticktimer = ticktimer_server::Ticktimer::new().expect("couldn't connect to ticktimer");
            let timestamp = ticktimer.elapsed_ms();

            let xns = xous_names::XousNames::new().unwrap();

            let ea = early_settings::EarlySettings::new(&xns).unwrap();

            let default_map = if cfg!(feature = "braille") {
                KeyMap::Braille
            } else {
                // the boot setting is intended to only contain the keyboard mapping...soooooooo.....we manage it as such
                // if feature creep causes more stuff to end up here this should be made into something more generic.
                // But! If you think you need to slap something in the boot setting, think again. This is an unverified,
                // unencrypted and plaintext bit of memory that anyone can read or mess with. The only reason the keyboard
                // layout is kept here is because you want your keyboard to be in the right language before typing in the
                // password to unlock the PDDB, which is *actually* where all the user settings should be located.
                let kb_raw = ea.get_keymap().expect("cannot fetch keymap from early settings");
                let map = KeyMap::from(kb_raw);
                map
            };

            Keyboard {
                conn: xous::connect(sid).unwrap(),
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
                new_state: HashSet::with_capacity(16), // pre-allocate space since this has to work in an interrupt context
                last_state: HashSet::with_capacity(16),
                ticktimer,
                map: default_map,
                delay: 500,
                rate: 50, // ubuntu default rate is 90, windows is 30
                shift_down: false,
                shift_up: false,
                alt_down: false,
                alt_up: false,
                repeating_key: None,
                rate_timestamp: timestamp,
                chord_timestamp: timestamp,
                chord_interval: 50,
                chord: [[false; KBD_COLS]; KBD_ROWS],
                chord_active: 0,
                chord_captured: false,
                susres: RegManager::new(csr.as_mut_ptr() as *mut u32),
                debug: 0,
                early_settings: ea,
            }
        }

        pub fn init(&mut self) {
            xous::claim_interrupt(
                utra::keyboard::KEYBOARD_IRQ,
                handle_kbd,
                self as *mut Keyboard as *mut usize,
            )
            .expect("couldn't claim irq");
            self.csr.wo(utra::keyboard::EV_PENDING, self.csr.r(utra::keyboard::EV_PENDING)); // clear in case it's pending for some reason
            self.csr.wo(utra::keyboard::EV_ENABLE,
            self.csr.ms(utra::keyboard::EV_ENABLE_KEYPRESSED, 1) |
            self.csr.ms(utra::keyboard::EV_ENABLE_INJECT, 1)
            );

            self.susres.push_fixed_value(RegOrField::Reg(utra::keyboard::EV_PENDING), 0xFFFF_FFFF);
            self.susres.push(RegOrField::Reg(utra::keyboard::EV_ENABLE), None);
        }

        pub(crate) fn suspend(&mut self) {
            self.susres.suspend();
            self.csr.wo(utra::keyboard::EV_ENABLE, 0);
        }
        pub(crate) fn resume(&mut self) {
            self.susres.resume();

            // clear the keyboard state vectors -- actually, if a key was being pressed at the time of suspend
            // it's not really relevant anymore; let's throw everything away and start from a clean slate.
            self.new_state.clear();
            self.last_state.clear();
            self.shift_down = false;
            self.shift_up = false;
            self.alt_down = false;
            self.alt_up = false;
            self.repeating_key = None;
            self.chord_captured = false;
            self.chord_active = 0;
            self.chord = [[false; KBD_COLS]; KBD_ROWS];

            // ensure interrupts are re-enabled -- this could /shouldn't/ be necessary but we're having
            // some strange resume behavior, trying to see if this resolves it.
            self.csr.wo(utra::keyboard::EV_PENDING, self.csr.r(utra::keyboard::EV_PENDING));
            self.csr.wo(utra::keyboard::EV_ENABLE,
                self.csr.ms(utra::keyboard::EV_ENABLE_KEYPRESSED, 1) |
                self.csr.ms(utra::keyboard::EV_ENABLE_INJECT, 1)
            );
        }

        pub(crate) fn set_map(&mut self, map: KeyMap) {
            self.early_settings.set_keymap(map.into()).expect("cannot set early keymap");
            self.map = map;
        }
        pub(crate) fn get_map(&mut self) -> KeyMap {
            let kb_raw = self.early_settings.get_keymap().expect("cannot fetch early keymap");
            self.map = KeyMap::from(kb_raw);
            self.map
        }
        pub(crate) fn set_repeat(&mut self, rate: u32, delay: u32) {
            self.rate = rate;
            self.delay = delay;
        }
        pub(crate) fn set_chord_interval(&mut self, delay: u32) {
            self.chord_interval = delay;
        }
        pub(crate) fn get_repeat_check_interval(&self) -> u32 {
            self.rate
        }

        pub(crate) fn poll(&mut self) {
            // disable the interrupt while we're polling, to avoid a race condition...
            self.csr.rmwf(utra::keyboard::EV_ENABLE_KEYPRESSED, 0);
            self.new_state.clear();
            for r in 0..KBD_ROWS {
                let cols: u16 = kbd_getrow(self, r as u8);
                if cols != 0 {
                    for c in 0..KBD_COLS {
                        if (cols & (1 << c)) != 0 {
                            self.new_state.insert(
                                RowCol{r: r as _, c: c as _}
                            );
                        }
                    }
                }
            }
            if self.csr.rf(utra::keyboard::EV_ENABLE_KEYPRESSED) != 0 {
                log::warn!("kbd interrupt was set when we didn't expect it, clearing anyways...");
                self.csr.wfo(utra::keyboard::EV_PENDING_KEYPRESSED, 1);
            }
            self.csr.rmwf(utra::keyboard::EV_ENABLE_KEYPRESSED, 1);
        }

        pub(crate) fn update(&mut self) -> KeyRawStates {
            // EV_PENDING_KEYPRESSED effectively does an XOR of the previous keyboard state
            // to the current state, which is why update() does not repeatedly issue results
            // for keys that are pressed & held.
            log::trace!("update new_state:  {:?}", self.new_state);
            log::trace!("update last_state: {:?}", self.last_state);

            let mut krs = KeyRawStates::new();

            // compute the key-ups: this would be codes that are in the last_state, but not in the incoming
            // new_state
            for &rc in self.last_state.difference(&self.new_state) {
                krs.keyups.push(rc);
            }

            // compute key-downs: codes that are in the new_state, but not in last_state
            for &rc in self.new_state.difference(&self.last_state) {
                krs.keydowns.push(rc);
            }

            self.last_state.clear();
            for &rc in self.new_state.iter() {
                self.last_state.insert(rc);
            }

            log::trace!("krs: {:?}", krs);
            krs
        }

        pub(crate) fn track_chord(&mut self, krs: &KeyRawStates) -> Vec<char> {
            /*
            Chording algorithm:

            1. Wait for first keydown event to happen; record as pressed in table
            2. Start chording timer
            3. Record press/unpress in table
            4. Wait for chording timer to timeout
            5. Extract chord state and turn into scancode using lookup table
            6. Return scancodes
             */
            let was_idle = self.chord_active == 0;
            for rc in krs.keydowns.iter() {
                log::info!("keydown r: {} c: {}", rc.r, rc.c);
                self.chord[rc.r as usize][rc.c as usize] = true;
                self.chord_active += 1;
            }
            log::trace!("self.chord: {:?}", self.chord);
            let mut keystates: Vec<char> = Vec::new();

            let now = self.ticktimer.elapsed_ms();
            if was_idle && self.chord_active != 0 {
                // "rising edge" of chord_active
                self.chord_timestamp = now; // record the beginning of the chord active interval
            }

            if self.chord_active != 0 && ((now - self.chord_timestamp) >= self.chord_interval as u64) && !self.chord_captured {
                self.chord_captured = true;
                log::trace!("interpreting chords");
                // extract chord state
                /*
                    keyboard:
                    2 1 0 space 3 4 5
                    braille dots:
                    0 3
                    1 4
                    2 5
                */
                /*
                    7/5  0/1   1/2       5/7     4/8  8/6
                               2/3       2/3
                    8/0  6/4                         3/9
                    8/3  5/2 3/6
                         8/2
                 */
                let keys: [bool; 6] = [
                    self.chord[1][2],
                    self.chord[0][1],
                    self.chord[7][5],
                    self.chord[5][7],
                    self.chord[4][8],
                    self.chord[8][6],
                ];
                let mut keycode: usize = 0;
                for i in 0..keys.len() {
                    if keys[i] {
                        keycode |= 1 << i;
                    }
                }
                log::trace!("keycode: 0x{:x}", keycode);
                let keychar = match keycode {
                    0b000_001 => Some('a'),
                    0b000_011 => Some('b'),
                    0b001_001 => Some('c'),
                    0b011_001 => Some('d'),
                    0b010_001 => Some('e'),
                    0b001_011 => Some('f'),
                    0b011_011 => Some('g'),
                    0b010_011 => Some('h'),
                    0b001_010 => Some('i'),
                    0b011_010 => Some('j'),

                    0b000_101 => Some('k'),
                    0b000_111 => Some('l'),
                    0b001_101 => Some('m'),
                    0b011_101 => Some('n'),
                    0b010_101 => Some('o'),
                    0b001_111 => Some('p'),
                    0b011_111 => Some('q'),
                    0b010_111 => Some('r'),
                    0b001_110 => Some('s'),
                    0b011_110 => Some('t'),

                    0b100_101 => Some('u'),
                    0b100_111 => Some('v'),
                    0b101_101 => Some('x'),
                    0b111_101 => Some('y'),
                    0b110_101 => Some('z'),
                    //0b101_111 => Some(''),
                    //0b111_111 => Some(''),
                    //0b1010_111 => Some(''),
                    //0b101_110 => Some(''),
                    0b111_010 => Some('w'),
                    _ => None,
                };
                if let Some(key) = keychar {
                    keystates.push(key);
                }

                let up = self.chord[6][4];
                if up { keystates.push('↑'); }

                let left = self.chord[8][3];
                if left { keystates.push('←'); }
                let right = self.chord[3][6];
                if right { keystates.push('→'); }
                let down = self.chord[8][2];
                if down { keystates.push('↓'); }
                let center = self.chord[5][2];
                if center { keystates.push('∴'); }

                let space = self.chord[2][3];
                if space { keystates.push(' '); }

                let esc = self.chord[8][0];
                let bs: char = 0x8_u8.into();  // back space
                if esc { keystates.push(bs); }

                let func = self.chord[3][9];
                let cr: char = 0xd_u8.into();  // carriage return
                if func { keystates.push(cr); }

                log::debug!("up {}, left {}, right {}, down, {}, center, {}, space {}, esc {}, func {}",
                    up, left, right, down, center, space, esc, func);
            }
            for rc in krs.keyups.iter() {
                self.chord[rc.r as usize][rc.c as usize] = false;
                if self.chord_active > 0 {
                    self.chord_active -= 1;
                } else {
                    log::error!("received more keyups than we had keydowns!")
                }
            }
            if self.chord_active == 0 {
                self.chord_captured = false;
            }

            keystates
        }

        pub(crate) fn track_keys(&mut self, krs: &KeyRawStates) -> Vec<char> {
            /*
              "conventional" keyboard algorithm. The goals of this are to differentiate
              the cases of "shift", "alt", and "hold".

              thus, we check for the special-case of shift/alt in the keydowns/keyups vectors, and
              track them as separate modifiers

              then for all others, we note the down time, and compare it to the current time
              to determine if a "hold" modifier applies
             */
            let mut ks: Vec<char> = Vec::new();

            // first check for shift and alt keys
            for rc in krs.keydowns.iter() {
                match self.map {
                    KeyMap::Azerty => {
                        if (rc.r == 8) && (rc.c == 5) { // left shift (orange)
                            if self.alt_up == false {
                                self.alt_down = true;
                            } else {
                                self.alt_up = false;
                            }
                        } else if (rc.r == 8) && (rc.c == 9) { // right shift (yellow)
                            if self.shift_up == false {
                                self.shift_down = true;
                            } else {
                                self.shift_up = false;
                            }
                        }
                    },
                    _ => { // the rest just have one color of shift
                        if ((rc.r == 8) && (rc.c == 5)) || ((rc.r == 8) && (rc.c == 9)) {
                            // if the shift key was tapped twice, remove the shift modifier
                            if self.shift_up == false {
                                //info!("shift down true");
                                self.shift_down = true;
                            } else {
                                //info!("shift up false");
                                self.shift_up = false;
                            }
                        }
                    }
                }
            }
            let mut keyups_noshift: Vec::<RowCol> = Vec::new();
            for &rc in krs.keyups.iter() {
                match self.map {
                    KeyMap::Azerty => {
                        if (rc.r == 8) && (rc.c == 5) { // left shift (orange)
                            if self.alt_down {
                                self.alt_up = true;
                            }
                            self.alt_down = false;
                        } else if (rc.r == 8) && (rc.c == 9) { // right shift (yellow)
                            if self.shift_down {
                                self.shift_up = true;
                            }
                            self.shift_down = false;
                        } else {
                            keyups_noshift.push(RowCol{r: rc.r as _, c: rc.c as _});
                        }
                    },
                    _ => { // the rest just have one color of shift
                        if ((rc.r == 8) && (rc.c == 5)) || ((rc.r == 8) && (rc.c == 9)) {
                            // only set the shift-up if we didn't previously clear it with a double-tap of shift
                            if self.shift_down {
                                //info!("shift up true");
                                self.shift_up = true;
                            }
                            //info!("shift down false");
                            self.shift_down = false;
                        } else {
                            //info!("adding non-shift entry {:?}", rc);
                            keyups_noshift.push(RowCol{r: rc.r as _, c: rc.c as _});
                        }
                    }
                }
            }

            // interpret keys in the context of the shift/alt modifiers
            if !krs.keydowns.is_empty() {
                self.chord_timestamp = self.ticktimer.elapsed_ms();
            }
            for &rc in krs.keydowns.iter() {
                let code = match self.map {
                    KeyMap::Qwerty => map_qwerty(rc),
                    KeyMap::Dvorak => map_dvorak(rc),
                    KeyMap::Azerty => map_azerty(rc),
                    KeyMap::Qwertz => map_qwertz(rc),
                    _ => ScanCode {key: None, shift: None, hold: None, alt: None},
                };
                if code.hold == None
                && !((rc.r == 5) && (rc.c == 2)) // scan code for the menu key
                 { // if there isn't a pre-defined meaning if the key is held *and* it's not the menu key: it's a repeating key
                    if let Some(key) = code.key {
                        self.repeating_key = Some(key);
                    }
                }
            }

            let now = self.ticktimer.elapsed_ms();
            let hold: bool;
            if (now - self.chord_timestamp) >= self.delay as u64 {
                if self.rate_timestamp <= self.chord_timestamp {
                    self.rate_timestamp = now;
                }
                hold = true;
            } else {
                hold = false;
            }

            for &rc in keyups_noshift.iter() {
                // info!("interpreting keyups_noshift entry {:?}", rc);
                let code = match self.map {
                    KeyMap::Qwerty => map_qwerty(rc),
                    KeyMap::Dvorak => map_dvorak(rc),
                    KeyMap::Azerty => map_azerty(rc),
                    KeyMap::Qwertz => map_qwertz(rc),
                    _ => ScanCode {key: None, shift: None, hold: None, alt: None},
                };
                // delete the key repeat if there is one
                if code.hold == None {
                    if let Some(key) = code.key {
                        if let Some(rk) = self.repeating_key {
                            if rk == key {
                                self.repeating_key = None;
                            }
                        }
                    }
                }

                match self.map {
                    KeyMap::Azerty => {
                        if self.shift_down || self.shift_up {
                            if let Some(shiftcode) = code.shift {
                                ks.push(shiftcode);
                            } else if let Some(keycode) = code.key {
                                ks.push(keycode);
                            }
                            self.shift_down = false;
                            self.shift_up = false;
                        } else if self.alt_down || self.alt_up {
                            if let Some(altcode) = code.alt {
                                ks.push(altcode);
                            } else if let Some(shiftcode) = code.shift {
                                ks.push(shiftcode);
                            } else if let Some(keycode) = code.key {
                                ks.push(keycode);
                            }
                            self.alt_down = false;
                            self.alt_up = false;
                        } else if hold {
                            if let Some(holdcode) = code.hold {
                                ks.push(holdcode);
                            }
                        } else {
                            if let Some(keycode) = code.key {
                                ks.push(keycode);
                            }
                        }
                    },
                    _ => {
                        if self.shift_down || self.alt_down || self.shift_up || self.alt_up {
                            if let Some(shiftcode) = code.shift {
                                ks.push(shiftcode);
                            } else if let Some(keycode) = code.key {
                                ks.push(keycode);
                            }
                            self.shift_down = false;
                            self.alt_down = false;
                            self.shift_up = false;
                            self.alt_up = false;
                        } else if hold {
                            if let Some(holdcode) = code.hold {
                                ks.push(holdcode);
                            }
                        } else {
                            if let Some(keycode) = code.key {
                                // info!("appeding normal key '{}'", keycode);
                                ks.push(keycode);
                            }
                        }
                    }
                }
            }

            // if we're in a key hold state, we've passed the rate timestamp point, and there's a repeating key defined
            if hold && ((now - self.rate_timestamp) >= self.rate as u64) && self.repeating_key.is_some() {
                self.rate_timestamp = now;
                if let Some(repeatkey) = self.repeating_key {
                    ks.push(repeatkey);
                }
            }

            ks
        }
        pub fn is_repeating_key(&self) -> bool {
            self.repeating_key.is_some()
        }
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "xous"))]
mod implementation {
    use crate::*;

    #[allow(dead_code)]
    pub(crate) struct Keyboard {
        cid: xous::CID,
        map: KeyMap,
        rate: u32,
        delay: u32,
        chord_interval: u32,
        pub debug: u32,
    }

    impl Keyboard {
        pub fn new(sid: xous::SID) -> Keyboard {
            Keyboard {
                cid: xous::connect(sid).unwrap(),
                map: KeyMap::Qwerty,
                rate: 20,
                delay: 200,
                chord_interval: 50,
                debug: 0,
            }
        }
        pub fn init(&mut self) {}
        pub fn suspend(&self) {
        }
        pub fn resume(&self) {
        }

        pub fn set_map(&mut self, map: KeyMap) {
            self.map = map;
        }
        pub fn get_map(&self) -> KeyMap {self.map}

        pub fn update(&self) -> KeyRawStates {
            KeyRawStates::new()
        }

        pub fn track_chord(&mut self, _krs: &KeyRawStates) -> Vec<char> {
            Vec::new()
        }

        pub fn track_keys(&mut self, _rs: &KeyRawStates) -> Vec<char> {
            Vec::new()
        }

        pub fn set_repeat(&mut self, rate: u32, delay: u32) {
            self.rate = rate;
            self.delay = delay;
        }

        pub fn set_chord_interval(&mut self, delay: u32) {
            self.chord_interval = delay;
        }

        pub fn is_repeating_key(&self) -> bool {
            false
        }
        pub(crate) fn get_repeat_check_interval(&self) -> u32 {
            self.rate
        }
        pub(crate) fn poll(&mut self) {}
    }
}

fn main() -> ! {
    use crate::implementation::Keyboard;
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // connections expected:
    //  - GAM
    //  - graphics (if building for hosted mode)
    //  - oqc (for factory test)
    //  - status sub system (for setting the layout, autobacklight feature)
    //  - USB (for getting layout)
    //  - Preference manager
    #[cfg(all(any(feature="precursor", feature="renode"), not(feature="dvt")))]
    let kbd_sid = xns.register_name(api::SERVER_NAME_KBD, Some(5)).expect("can't register server");
    #[cfg(all(any(feature="precursor", feature="renode"), feature="dvt"))] // dvt build has less in it
    let kbd_sid = xns.register_name(api::SERVER_NAME_KBD, Some(4)).expect("can't register server");
    #[cfg(not(target_os = "xous"))]
    let kbd_sid = xns.register_name(api::SERVER_NAME_KBD, Some(5)).expect("can't register server");
    log::trace!("registered with NS -- {:?}", kbd_sid);

    // Create a new kbd object
    let mut kbd = Keyboard::new(kbd_sid);
    kbd.init();

    // register a suspend/resume listener
    let self_cid = xous::connect(kbd_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(None, &xns, Opcode::SuspendResume as u32, self_cid).expect("couldn't create suspend/resume object");

    // start a thread that can ping the keyboard loop when a key is held down
    let ticktimer = ticktimer_server::Ticktimer::new().unwrap();

    let mut listener_conn: Option<CID> = None;
    let mut listener_op: Option<usize> = None;
    let mut raw_listener_conn: Option<CID> = None;
    let mut raw_listener_op: Option<u32> = None;
    let mut observer_conn: Option<CID> = None;
    let mut observer_op: Option<usize> = None;

    let mut vibe = false;
    let llio = llio::Llio::new(&xns);
    /*{
        log::warn!("kbd server is overriding WFI for debugging, remember to disable for production");
        llio.wfi_override(true).unwrap();
    }*/
    #[cfg(not(feature="rawserial"))]
    let mut esc_index: Option<usize> = None;
    #[cfg(not(feature="rawserial"))]
    let mut esc_chars = [0u8; 16];
    // storage for any blocking listeners
    let mut blocking_listener = Vec::<MessageSender>::new();
    #[cfg(feature="rawserial")]
    let mut blocking_queue = VecDeque::<usize>::new();

    log::trace!("starting main loop");
    loop {
        let msg = xous::receive_message(kbd_sid).unwrap(); // this blocks until we get a message
        log::trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                kbd.suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                kbd.resume();
            }),
            Some(Opcode::Vibe) => msg_scalar_unpack!(msg, ena, _,  _,  _, {
                if ena != 0 { vibe = true }
                else { vibe = false }
            }),
            Some(Opcode::BlockingKeyListener) => {
                #[cfg(feature="rawserial")]
                if blocking_queue.len() != 0 {
                    // we have a pending byte in the queue, return it.
                    let k_prime = blocking_queue.pop_front().unwrap();
                    xous::return_scalar2(msg.sender, k_prime, 0).unwrap();
                } else {
                    // by simply storing the sender address and not returning a value,
                    // the sender will remain blocked until the return value is generated
                    blocking_listener.push(msg.sender);
                }
                #[cfg(not(feature="rawserial"))]
                blocking_listener.push(msg.sender);
            },
            Some(Opcode::RegisterListener) => {
                let buffer = unsafe{Buffer::from_memory_message(msg.body.memory_message().unwrap())};
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
            },
            Some(Opcode::RegisterRawListener) => {
                let buffer = unsafe{Buffer::from_memory_message(msg.body.memory_message().unwrap())};
                let kr = buffer.as_flat::<KeyboardRegistration, _>().unwrap();
                match xns.request_connection_blocking(kr.server_name.as_str()) {
                    Ok(cid) => {
                        raw_listener_conn = Some(cid);
                        raw_listener_op = Some(kr.listener_op_id as u32);
                    }
                    Err(e) => {
                        log::error!("couldn't connect to listener: {:?}", e);
                        raw_listener_conn = None;
                        raw_listener_op = None;
                    }
                }
            },
            Some(Opcode::RegisterKeyObserver) => {
                let buffer = unsafe{Buffer::from_memory_message(msg.body.memory_message().unwrap())};
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
            },
            Some(Opcode::SelectKeyMap) => msg_scalar_unpack!(msg, km, _, _, _, {
                kbd.set_map(KeyMap::from(km))
            }),
            Some(Opcode::GetKeyMap) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender,
                    kbd.get_map().into()
                ).expect("can't retrieve keymap");
            }),
            Some(Opcode::SetRepeat) => msg_scalar_unpack!(msg, rate, delay, _, _, {
                kbd.set_repeat(rate as u32, delay as u32);
            }),
            Some(Opcode::SetChordInterval) => msg_scalar_unpack!(msg, delay, _, _, _, {
                kbd.set_chord_interval(delay as u32);
            }),
            Some(Opcode::InjectKey) => msg_scalar_unpack!(msg, k, _, _, _, {
                // key substitutions to help things work better
                // 1b5b317e = home
                // 1b5b44 = left
                // 1b5b43 = right
                // 1b5b41 = up
                // 1b5b42 = down
                log::trace!("{:x} {}", k, kbd.debug);
                kbd.debug = 0;

                #[cfg(feature="rawserial")]
                {
                    if blocking_listener.len() == 0 {
                        if blocking_queue.len() > BLOCKING_QUEUE_LEN {
                            log::warn!("blocking queue has overflowed, dropping {}", k);
                        } else {
                            blocking_queue.push_back(k);
                        }
                    } else {
                        for listener in blocking_listener.drain(..) {
                            // we must unblock anyways once the key is hit; even if the key is invalid,
                            // send the invalid key. The receiving library function will clean this up into a nil-response vector.
                            if blocking_queue.len() != 0 {
                                blocking_queue.push_back(k);
                                let k_prime = blocking_queue.pop_front().unwrap();
                                xous::return_scalar2(listener, k_prime, 0).unwrap();
                            } else {
                                xous::return_scalar2(listener, k, 0).unwrap();
                            }
                        }
                    }
                    continue; // do not execute the remaining code in this branch
                }
                #[cfg(not(feature="rawserial"))]
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
                                log::warn!("Unhandled escape sequence: {:x?}", &esc_chars[..i+1]);
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
                            let bs_del_fix = if k == 0x7f {
                                0x08
                            } else {
                                k
                            };
                            core::char::from_u32(bs_del_fix as u32).unwrap_or('\u{0000}')
                        }
                    }
                };
                #[cfg(all(feature="debuginject", not(feature="rawserial")))]
                if let Some(conn) = listener_conn {
                    if key != '\u{0000}' {
                        if key >= '\u{f700}' && key <= '\u{f8ff}' {
                            log::info!("ignoring key '{}'({:x})", key, key as u32); // ignore Apple PUA characters
                        } else {
                            log::info!("injecting key '{}'({:x})", key, key as u32); // always be noisy about this, it's an exploit path
                            xous::try_send_message(conn,
                                xous::Message::new_scalar(listener_op.unwrap(),
                                    key as u32 as usize,
                                    '\u{0000}' as u32 as usize,
                                    '\u{0000}' as u32 as usize,
                                    '\u{0000}' as u32 as usize,
                                )
                            ).unwrap_or_else(|_| {
                                log::info!("Input overflow, dropping keys!");
                                xous::Result::Ok
                            });
                        }
                    }
                }

                if observer_conn.is_some() && observer_op.is_some() {
                    log::trace!("sending observer key");
                    xous::try_send_message(observer_conn.unwrap(),
                        xous::Message::new_scalar(
                            observer_op.unwrap(),
                            0,
                            0,
                            0,
                            0,
                        )
                    ).ok();
                }

                #[cfg(all(feature="debuginject", not(feature="rawserial")))]
                for listener in blocking_listener.drain(..) {
                    // we must unblock anyways once the key is hit; even if the key is invalid,
                    // send the invalid key. The receiving library function will clean this up into a nil-response vector.
                    xous::return_scalar2(listener, key as u32 as usize, 0).unwrap();
                }
            }),
            Some(Opcode::HandlerTrigger) => {
                let rawstates = kbd.update();

                if raw_listener_conn.is_some() && raw_listener_op.is_some()
                && (rawstates.keydowns.len() > 0 || rawstates.keyups.len() > 0)
                {
                    // manual serialization of KeyRawStates, because rkyv can't derive a serializer.
                    let mut krs_ser: [(u8, u8); 32] = [(255, 255); 32];
                    for (&src, (r, c)) in rawstates.keydowns.iter().zip(krs_ser[..16].iter_mut()) {
                        *r = src.r;
                        *c = src.c;
                    }
                    for (&src, (r, c)) in rawstates.keyups.iter().zip(krs_ser[16..].iter_mut()) {
                        *r = src.r;
                        *c = src.c;
                    }

                    let buf = Buffer::into_buf(krs_ser).or(Err(xous::Error::InternalError)).expect("couldn't serialize krs buffer");
                    buf.lend(raw_listener_conn.unwrap(), raw_listener_op.unwrap()).expect("couldn't send raw scancodes");
                }

                if observer_conn.is_some() && observer_op.is_some() {
                    log::trace!("sending observer key");
                    xous::try_send_message(observer_conn.unwrap(),
                        xous::Message::new_scalar(
                            observer_op.unwrap(),
                            0,
                            0,
                            0,
                            0,
                        )
                    ).ok();
                }

                // interpret scancodes
                // the track_* functions track the keyup/keydowns to modify keys with shift, hold, and chord state
                let kc: Vec<char> = match kbd.get_map() {
                    KeyMap::Braille => {
                        kbd.track_chord(&rawstates)
                    },
                    _ => {
                        kbd.track_keys(&rawstates)
                    },
                };

                // send keys, if any
                // handle the blocking listeners
                if kc.len() > 0 {
                    for listener in blocking_listener.drain(..) {
                        xous::return_scalar2(
                            listener,
                            if kc.len() >= 1 {
                                kc[0] as u32 as usize
                            } else {
                                0
                            },
                            if kc.len() >= 2 {
                                kc[1] as u32 as usize
                            } else {
                                0
                            },
                        ).unwrap();
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
                if kc.len() > 0 && listener_conn.is_some() && listener_op.is_some() {
                    if vibe {
                        llio.vibe(llio::VibePattern::Short).unwrap();
                    }
                    for kv in kc.chunks(4) {
                        let mut keys: [char; 4] = ['\u{0000}', '\u{0000}', '\u{0000}', '\u{0000}'];
                        for i in 0..kv.len() {
                            keys[i] = kv[i];
                        }
                        log::trace!("sending keys {:?}", keys);
                        xous::try_send_message(listener_conn.unwrap(),
                            xous::Message::new_scalar(
                                listener_op.unwrap(),
                                keys[0] as u32 as usize,
                                keys[1] as u32 as usize,
                                keys[2] as u32 as usize,
                                keys[3] as u32 as usize,
                            )
                        ).ok();
                    }
                }
                // as long as we have a keydown, keep pinging the loop at a high rate. this consumes more power, but keydowns are relatively rare.
                if kbd.is_repeating_key() {
                    log::trace!("keydowns hold");
                    // fire a second call to check if we should transition to a repeating state
                    ticktimer.sleep_ms(kbd.get_repeat_check_interval() as _).unwrap();
                    kbd.poll();
                    // this is highly likely to overflow; if it does, just discard any excess
                    xous::try_send_message(self_cid,
                        xous::Message::new_scalar(Opcode::HandlerTrigger.to_usize().unwrap(), 0, 0, 0, 0)
                    ).ok();
                }
            },
            None => {log::error!("couldn't convert opcode"); break}
        }
    }
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(kbd_sid).unwrap();
    xous::destroy_server(kbd_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}

#[cfg(not(feature="rawserial"))]
fn esc_match(esc_chars: &[u8]) -> Result<Option<char>, ()> {
    let mut extended = Vec::<u8>::new();
    for (i, &c) in esc_chars.iter().enumerate() {
        match i {
            0 => if c != 0x1b { return Err(()) },
            1 => if c != 0x5b { return Err(()) },
            2 => match c {
                0x41 => return Ok(Some('↑')),
                0x42 => return Ok(Some('↓')),
                0x43 => return Ok(Some('→')),
                0x44 => return Ok(Some('←')),
                0x7e => return Err(()), // premature end
                _ => extended.push(c)
            }
            _ => if c == 0x7e {
                if extended.len() == 1 {
                    if extended[0] == 0x31 {
                        return Ok(Some('∴'))
                    }
                } else {
                    return Err(()) // code unrecognized
                }
            } else {
                extended.push(c)
            }
        }
    }
    Ok(None)
}
