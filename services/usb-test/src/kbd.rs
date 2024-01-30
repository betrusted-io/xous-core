use std::collections::HashSet;

use keyboard::{KeyRawStates, RowCol, ScanCode};
use num_traits::ToPrimitive;
use susres::{RegManager, RegOrField, SuspendResume};
use ticktimer_server::Ticktimer;
use utralib::generated::*;
use xous::CID;

use crate::api::*;

/// note: the code is structured to use at most 16 rows or 16 cols
const KBD_ROWS: usize = 9;
const KBD_COLS: usize = 10;

pub(crate) struct Keyboard {
    conn: CID,
    csr: utralib::CSR<u32>,
    /// where the interrupt handler copies the new state
    new_state: HashSet<RowCol>,
    /// remember the last key states
    last_state: HashSet<RowCol>,
    /// connection to the timer for real-time events
    ticktimer: Ticktimer,
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
    /// timestamp timekeeper for chording / hold key
    chord_timestamp: u64,
    /// timestamp to track repeating key interval
    rate_timestamp: u64,
    /// track the last key held down, which lacks a hold alternate meaning, for repeating
    repeating_key: Option<char>,
    susres: RegManager<{ utra::keyboard::KEYBOARD_NUMREGS }>,
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
                        kbd.new_state.insert(RowCol { r: r as _, c: c as _ });
                    }
                }
            }
        }
        xous::try_send_message(
            kbd.conn,
            xous::Message::new_scalar(Opcode::HandlerTrigger.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .ok();
    }
    if kbd.csr.rf(utra::keyboard::EV_PENDING_INJECT) != 0 {
        loop {
            let char_reg = kbd.csr.r(utra::keyboard::UART_CHAR);
            if char_reg & kbd.csr.ms(utra::keyboard::UART_CHAR_STB, 1) != 0 {
                xous::try_send_message(
                    kbd.conn,
                    xous::Message::new_scalar(
                        Opcode::KeyboardChar.to_usize().unwrap(),
                        (char_reg & 0xff) as _,
                        0,
                        0,
                        0,
                    ),
                )
                .ok();
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
        _ => 0,
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

        let mut kbd = Keyboard {
            conn: xous::connect(sid).unwrap(),
            csr: CSR::new(csr.as_mut_ptr() as *mut u32),
            new_state: HashSet::with_capacity(16), /* pre-allocate space since this has to work in an
                                                    * interrupt context */
            last_state: HashSet::with_capacity(16),
            ticktimer,
            delay: 500,
            rate: 20,
            shift_down: false,
            shift_up: false,
            alt_down: false,
            alt_up: false,
            repeating_key: None,
            chord_timestamp: timestamp,
            rate_timestamp: timestamp,
            susres: RegManager::new(csr.as_mut_ptr() as *mut u32),
        };

        xous::claim_interrupt(
            utra::keyboard::KEYBOARD_IRQ,
            handle_kbd,
            (&mut kbd) as *mut Keyboard as *mut usize,
        )
        .expect("couldn't claim irq");
        kbd.csr.wo(utra::keyboard::EV_PENDING, kbd.csr.r(utra::keyboard::EV_PENDING)); // clear in case it's pending for some reason
        kbd.csr.wo(
            utra::keyboard::EV_ENABLE,
            kbd.csr.ms(utra::keyboard::EV_ENABLE_KEYPRESSED, 1)
                | kbd.csr.ms(utra::keyboard::EV_ENABLE_INJECT, 1),
        );

        kbd.susres.push_fixed_value(RegOrField::Reg(utra::keyboard::EV_PENDING), 0xFFFF_FFFF);
        kbd.susres.push(RegOrField::Reg(utra::keyboard::EV_ENABLE), None);

        kbd
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

        // ensure interrupts are re-enabled -- this could /shouldn't/ be necessary but we're having
        // some strange resume behavior, trying to see if this resolves it.
        self.csr.wo(utra::keyboard::EV_PENDING, self.csr.r(utra::keyboard::EV_PENDING));
        self.csr.wo(
            utra::keyboard::EV_ENABLE,
            self.csr.ms(utra::keyboard::EV_ENABLE_KEYPRESSED, 1)
                | self.csr.ms(utra::keyboard::EV_ENABLE_INJECT, 1),
        );
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
        let mut keyups_noshift: Vec<RowCol> = Vec::new();
        for &rc in krs.keyups.iter() {
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
                keyups_noshift.push(RowCol { r: rc.r as _, c: rc.c as _ });
            }
        }

        // interpret keys in the context of the shift/alt modifiers
        if !krs.keydowns.is_empty() {
            self.chord_timestamp = self.ticktimer.elapsed_ms();
        }
        for &rc in krs.keydowns.iter() {
            let code = map_qwerty(rc);
            if code.hold == None && !((rc.r == 5) && (rc.c == 2))
            // scan code for the menu key
            {
                // if there isn't a pre-defined meaning if the key is held *and* it's not the menu key: it's a
                // repeating key
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
            let code = map_qwerty(rc);
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

        // if we're in a key hold state, we've passed the rate timestamp point, and there's a repeating key
        // defined
        if hold && ((now - self.rate_timestamp) >= self.rate as u64) && self.repeating_key.is_some() {
            self.rate_timestamp = now;
            if let Some(repeatkey) = self.repeating_key {
                ks.push(repeatkey);
            }
        }

        ks
    }
}

/// Compute the dvorak key mapping of row/col to key tuples
#[allow(dead_code)]
#[rustfmt::skip]
pub(crate) fn map_dvorak(code: RowCol) -> ScanCode {
    let rc = (code.r, code.c);

    match rc {
        (0, 0) => ScanCode{key: Some('1'), shift: Some('1'), hold: None, alt: None},
        (0, 1) => ScanCode{key: Some('2'), shift: Some('2'), hold: None, alt: None},
        (0, 2) => ScanCode{key: Some('3'), shift: Some('3'), hold: None, alt: None},
        (0, 3) => ScanCode{key: Some('4'), shift: Some('4'), hold: None, alt: None},
        (0, 4) => ScanCode{key: Some('5'), shift: Some('5'), hold: None, alt: None},
        (4, 5) => ScanCode{key: Some('6'), shift: Some('6'), hold: None, alt: None},
        (4, 6) => ScanCode{key: Some('7'), shift: Some('7'), hold: None, alt: None},
        (4, 7) => ScanCode{key: Some('8'), shift: Some('8'), hold: None, alt: None},
        (4, 8) => ScanCode{key: Some('9'), shift: Some('9'), hold: None, alt: None},
        (4, 9) => ScanCode{key: Some('0'), shift: Some('0'), hold: None, alt: None},

        (1, 0) => ScanCode{key: Some(0x8_u8.into()), shift: Some(0x8_u8.into()), hold: None /* hold of none -> repeat */, alt: Some(0x8_u8.into())}, // backspace
        (1, 1) => ScanCode{key: Some('\''), shift: Some('\''), hold: Some('@'), alt: None},
        (1, 2) => ScanCode{key: Some('p'), shift: Some('P'), hold: Some('#'), alt: None},
        (1, 3) => ScanCode{key: Some('y'), shift: Some('Y'), hold: Some('&'), alt: None},
        (1, 4) => ScanCode{key: Some('f'), shift: Some('F'), hold: Some('*'), alt: None},
        (5, 5) => ScanCode{key: Some('g'), shift: Some('G'), hold: Some('-'), alt: None},
        (5, 6) => ScanCode{key: Some('c'), shift: Some('C'), hold: Some('+'), alt: None},
        (5, 7) => ScanCode{key: Some('r'), shift: Some('R'), hold: Some('('), alt: None},
        (5, 8) => ScanCode{key: Some('l'), shift: Some('L'), hold: Some(')'), alt: None},
        (5, 9) => ScanCode{key: Some('?'), shift: Some('?'), hold: Some('!'), alt: None},

        (2, 0) => ScanCode{key: Some('a'), shift: Some('A'), hold: Some('\\'), alt: None},
        (2, 1) => ScanCode{key: Some('o'), shift: Some('O'), hold: Some('`'), alt: None},
        (2, 2) => ScanCode{key: Some('e'), shift: Some('E'), hold: Some('~'), alt: None},
        (2, 3) => ScanCode{key: Some('u'), shift: Some('U'), hold: Some('|'), alt: None},
        (2, 4) => ScanCode{key: Some('i'), shift: Some('I'), hold: Some('['), alt: None},
        (6, 5) => ScanCode{key: Some('d'), shift: Some('D'), hold: Some(']'), alt: None},
        (6, 6) => ScanCode{key: Some('h'), shift: Some('H'), hold: Some('<'), alt: None},
        (6, 7) => ScanCode{key: Some('t'), shift: Some('T'), hold: Some('>'), alt: None},
        (6, 8) => ScanCode{key: Some('n'), shift: Some('N'), hold: Some('{'), alt: None},
        (6, 9) => ScanCode{key: Some('s'), shift: Some('S'), hold: Some('}'), alt: None},

        (3, 0) => ScanCode{key: Some('q'), shift: Some('Q'), hold: Some('_'), alt: None},
        (3, 1) => ScanCode{key: Some('j'), shift: Some('J'), hold: Some('$'), alt: None},
        (3, 2) => ScanCode{key: Some('k'), shift: Some('K'), hold: Some('"'), alt: None},
        (3, 3) => ScanCode{key: Some('x'), shift: Some('X'), hold: Some(':'), alt: None},
        (3, 4) => ScanCode{key: Some('b'), shift: Some('B'), hold: Some(';'), alt: None},
        (7, 5) => ScanCode{key: Some('m'), shift: Some('M'), hold: Some('/'), alt: None},
        (7, 6) => ScanCode{key: Some('w'), shift: Some('W'), hold: Some('^'), alt: None},
        (7, 7) => ScanCode{key: Some('v'), shift: Some('V'), hold: Some('='), alt: None},
        (7, 8) => ScanCode{key: Some('z'), shift: Some('Z'), hold: Some('%'), alt: None},
        (7, 9) => ScanCode{key: Some(0xd_u8.into()), shift: Some(0xd_u8.into()), hold: Some(0xd_u8.into()), alt: Some(0xd_u8.into())}, // carriage return

        (8, 5) => ScanCode{key: Some(0xf_u8.into()), shift: Some(0xf_u8.into()), hold: Some(0xf_u8.into()), alt: Some(0xf_u8.into())}, // shift in (blue shift)
        (8, 6) => ScanCode{key: Some(','), shift: Some(0xe_u8.into()), hold: Some(0xe_u8.into()), alt: None},  // 0xe is shift out (sym)
        (8, 7) => ScanCode{key: Some(' '), shift: Some(' '), hold: None /* hold of none -> repeat */, alt: None},
        (8, 8) => ScanCode{key: Some('.'), shift: Some('😊'), hold: Some('😊'), alt: None},
        (8, 9) => ScanCode{key: Some(0xf_u8.into()), shift: Some(0xf_u8.into()), hold: Some(0xf_u8.into()), alt: Some(0xf_u8.into())}, // shift in (blue shift)

        // the F0/tab key also doubles as a secondary power key (can't do UP5K UART rx at same time)
        (8, 0) => ScanCode{key: Some(0x11_u8.into()), shift: Some(0x11_u8.into()), hold: Some(0x11_u8.into()), alt: Some(0x11_u8.into())}, // DC1 (F1)
        (8, 1) => ScanCode{key: Some(0x12_u8.into()), shift: Some(0x12_u8.into()), hold: Some(0x12_u8.into()), alt: Some(0x12_u8.into())}, // DC2 (F2)
        (3, 8) => ScanCode{key: Some(0x13_u8.into()), shift: Some(0x13_u8.into()), hold: Some(0x13_u8.into()), alt: Some(0x13_u8.into())}, // DC3 (F3)
        // the F4/ctrl key also doubles as a power key
        (3, 9) => ScanCode{key: Some(0x14_u8.into()), shift: Some(0x14_u8.into()), hold: Some(0x14_u8.into()), alt: Some(0x14_u8.into())}, // DC4 (F4)
        (8, 3) => ScanCode{key: Some('←'), shift: Some('←'), hold: None, alt: Some('←')},
        (3, 6) => ScanCode{key: Some('→'), shift: Some('→'), hold: None, alt: Some('→')},
        (6, 4) => ScanCode{key: Some('↑'), shift: Some('↑'), hold: None, alt: Some('↑')},
        (8, 2) => ScanCode{key: Some('↓'), shift: Some('↓'), hold: None, alt: Some('↓')},
        // this one is OK
        (5, 2) => ScanCode{key: Some('∴'), shift: Some('∴'), hold: None, alt: Some('∴')},

        _ => ScanCode {key: None, shift: None, hold: None, alt: None}
    }
}

/// Compute the qwerty key mapping of row/col to key tuples
#[rustfmt::skip]
pub(crate) fn map_qwerty(code: RowCol) -> ScanCode {
    let rc = (code.r, code.c);

    match rc {
        (0, 0) => ScanCode{key: Some('1'), shift: Some('1'), hold: None, alt: None},
        (0, 1) => ScanCode{key: Some('2'), shift: Some('2'), hold: None, alt: None},
        (0, 2) => ScanCode{key: Some('3'), shift: Some('3'), hold: None, alt: None},
        (0, 3) => ScanCode{key: Some('4'), shift: Some('4'), hold: None, alt: None},
        (0, 4) => ScanCode{key: Some('5'), shift: Some('5'), hold: None, alt: None},
        (4, 5) => ScanCode{key: Some('6'), shift: Some('6'), hold: None, alt: None},
        (4, 6) => ScanCode{key: Some('7'), shift: Some('7'), hold: None, alt: None},
        (4, 7) => ScanCode{key: Some('8'), shift: Some('8'), hold: None, alt: None},
        (4, 8) => ScanCode{key: Some('9'), shift: Some('9'), hold: None, alt: None},
        (4, 9) => ScanCode{key: Some('0'), shift: Some('0'), hold: None, alt: None},

        (1, 0) => ScanCode{key: Some('q'), shift: Some('Q'), hold: Some('%'), alt: None},
        (1, 1) => ScanCode{key: Some('w'), shift: Some('W'), hold: Some('^'), alt: None},
        (1, 2) => ScanCode{key: Some('e'), shift: Some('E'), hold: Some('~'), alt: None},
        (1, 3) => ScanCode{key: Some('r'), shift: Some('R'), hold: Some('|'), alt: None},
        (1, 4) => ScanCode{key: Some('t'), shift: Some('T'), hold: Some('['), alt: None},
        (5, 5) => ScanCode{key: Some('y'), shift: Some('Y'), hold: Some(']'), alt: None},
        (5, 6) => ScanCode{key: Some('u'), shift: Some('U'), hold: Some('<'), alt: None},
        (5, 7) => ScanCode{key: Some('i'), shift: Some('I'), hold: Some('>'), alt: None},
        (5, 8) => ScanCode{key: Some('o'), shift: Some('O'), hold: Some('{'), alt: None},
        (5, 9) => ScanCode{key: Some('p'), shift: Some('P'), hold: Some('}'), alt: None},

        (2, 0) => ScanCode{key: Some('a'), shift: Some('A'), hold: Some('@'), alt: None},
        (2, 1) => ScanCode{key: Some('s'), shift: Some('S'), hold: Some('#'), alt: None},
        (2, 2) => ScanCode{key: Some('d'), shift: Some('D'), hold: Some('&'), alt: None},
        (2, 3) => ScanCode{key: Some('f'), shift: Some('F'), hold: Some('*'), alt: None},
        (2, 4) => ScanCode{key: Some('g'), shift: Some('G'), hold: Some('-'), alt: None},
        (6, 5) => ScanCode{key: Some('h'), shift: Some('H'), hold: Some('+'), alt: None},
        (6, 6) => ScanCode{key: Some('j'), shift: Some('J'), hold: Some('='), alt: None},
        (6, 7) => ScanCode{key: Some('k'), shift: Some('K'), hold: Some('('), alt: None},
        (6, 8) => ScanCode{key: Some('l'), shift: Some('L'), hold: Some(')'), alt: None},
        (6, 9) => ScanCode{key: Some(0x8_u8.into()), shift: Some(0x8_u8.into()), hold: None /* hold of none -> repeat */, alt: Some(0x8_u8.into())},  // backspace

        (3, 0) => ScanCode{key: Some('!'), shift: Some('!'), hold: Some('`'), alt: None},
        (3, 1) => ScanCode{key: Some('z'), shift: Some('Z'), hold: Some('_'), alt: None},
        (3, 2) => ScanCode{key: Some('x'), shift: Some('X'), hold: Some('$'), alt: None},
        (3, 3) => ScanCode{key: Some('c'), shift: Some('C'), hold: Some('"'), alt: None},
        (3, 4) => ScanCode{key: Some('v'), shift: Some('V'), hold: Some('\''), alt: None},
        (7, 5) => ScanCode{key: Some('b'), shift: Some('B'), hold: Some(':'), alt: None},
        (7, 6) => ScanCode{key: Some('n'), shift: Some('N'), hold: Some(';'), alt: None},
        (7, 7) => ScanCode{key: Some('m'), shift: Some('M'), hold: Some('/'), alt: None},
        (7, 8) => ScanCode{key: Some('?'), shift: Some('?'), hold: Some('\\'), alt: None},
        (7, 9) => ScanCode{key: Some(0xd_u8.into()), shift: Some(0xd_u8.into()), hold: Some(0xd_u8.into()), alt: Some(0xd_u8.into())}, // carriage return

        (8, 5) => ScanCode{key: Some(0xf_u8.into()), shift: Some(0xf_u8.into()), hold: Some(0xf_u8.into()), alt: Some(0xf_u8.into())}, // shift in (blue shift)
        (8, 6) => ScanCode{key: Some(','), shift: Some(0xe_u8.into()), hold: Some('福'), alt: None},  // 0xe is shift out (sym) '富' -> just for testing hanzi plane
        (8, 7) => ScanCode{key: Some(' '), shift: Some(' '), hold: None /* hold of none -> repeat */, alt: None},
        (8, 8) => ScanCode{key: Some('.'), shift: Some('😊'), hold: Some('😊'), alt: None},
        (8, 9) => ScanCode{key: Some(0xf_u8.into()), shift: Some(0xf_u8.into()), hold: Some(0xf_u8.into()), alt: Some(0xf_u8.into())}, // shift in (blue shift)

        // the F0/tab key also doubles as a secondary power key (can't do UP5K UART rx at same time)
        (8, 0) => ScanCode{key: Some(0x11_u8.into()), shift: Some(0x11_u8.into()), hold: Some(0x11_u8.into()), alt: Some(0x11_u8.into())}, // DC1 (F1)
        (8, 1) => ScanCode{key: Some(0x12_u8.into()), shift: Some(0x12_u8.into()), hold: Some(0x12_u8.into()), alt: Some(0x12_u8.into())}, // DC2 (F2)
        (3, 8) => ScanCode{key: Some(0x13_u8.into()), shift: Some(0x13_u8.into()), hold: Some(0x13_u8.into()), alt: Some(0x13_u8.into())}, // DC3 (F3)
        // the F4/ctrl key also doubles as a power key
        (3, 9) => ScanCode{key: Some(0x14_u8.into()), shift: Some(0x14_u8.into()), hold: Some(0x14_u8.into()), alt: Some(0x14_u8.into())}, // DC4 (F4)
        (8, 3) => ScanCode{key: Some('←'), shift: Some('←'), hold: None, alt: Some('←')},
        (3, 6) => ScanCode{key: Some('→'), shift: Some('→'), hold: None, alt: Some('→')},
        (6, 4) => ScanCode{key: Some('↑'), shift: Some('↑'), hold: None, alt: Some('↑')},
        (8, 2) => ScanCode{key: Some('↓'), shift: Some('↓'), hold: None, alt: Some('↓')},
        // this one is OK
        (5, 2) => ScanCode{key: Some('∴'), shift: Some('∴'), hold: None, alt: Some('∴')},

        _ => ScanCode {key: None, shift: None, hold: None, alt: None}
    }
}
