#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use heapless::Vec;
use heapless::consts::*;

#[cfg(not(target_os = "none"))]
use heapless::spsc::Queue;

use core::convert::TryFrom;

use log::{error, info};

use core::pin::Pin;
use xous::buffer;
use rkyv::archived_value;

/// Compute the dvorak key mapping of row/col to key tuples
#[allow(dead_code)]
fn map_dvorak(code: RowCol) -> ScanCode {
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
        (8, 8) => ScanCode{key: Some('.'), shift: Some('😃'), hold: Some('😃'), alt: None},
        (8, 9) => ScanCode{key: Some(0xf_u8.into()), shift: Some(0xf_u8.into()), hold: Some(0xf_u8.into()), alt: Some(0xf_u8.into())}, // shift in (blue shift)

        // these are all bugged: row values are swapped on PCB
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


/// Compute the dvorak key mapping of row/col to key tuples
#[allow(dead_code)]
fn map_qwerty(code: RowCol) -> ScanCode {
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
        (8, 8) => ScanCode{key: Some('.'), shift: Some('😃'), hold: Some('😃'), alt: None},
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

#[cfg(target_os = "none")]
mod implementation {
    use heapless::Vec;
    use heapless::consts::*;
    use utralib::generated::*;
    use crate::api::*;
    use crate::{map_dvorak, map_qwerty};
    use log::{error, info};

    /// note: the code is structured to use at most 16 rows or 16 cols
    const KBD_ROWS: usize = 9;
    const KBD_COLS: usize = 10;

    pub struct Keyboard {
        csr: utralib::CSR<u32>,
        /// debounce counter array
        debounce: [[u8; KBD_COLS]; KBD_ROWS],
        /// threshold (in ms) for considering an up or down event to be debounced, in loop interations.
        threshold: u8,
        /// last timestamp (in ms) since last call
        timestamp: u64,
        /// remember the last keycode since a change event
        lastcode: Option<Vec<RowCol, U16>>,
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
        /// memoize when chord is all false
        chord_active: bool,
    }

    impl Keyboard {
        pub fn new() -> Keyboard {
            let csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::keyboard::HW_KEYBOARD_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map Keyboard CSR range");

            let mut ticktimer = XousTickTimer::new(ticktimer_client).expect("KBD: couldn't connect to ticktimer");

            let timestamp = ticktimer.elapsed_ms();
            let kbd = Keyboard {
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
                debounce: [[0; KBD_COLS]; KBD_ROWS],
                threshold: 5,
                timestamp: timestamp,
                lastcode: None,
                ticktimer: ticktimer_conn,
                map: KeyMap::Qwerty,
                delay: 500,
                rate: 20,
                shift_down: false,
                shift_up: false,
                alt_down: false,
                alt_up: false,
                repeating_key: None,
                rate_timestamp: timestamp,
                chord_timestamp: timestamp,
                chord_interval: 50,
                chord: [[false; KBD_COLS]; KBD_ROWS],
                chord_active: false,
            };

            info!("KBD: hardware initialized"); // gratuitous info statement to keep clippy quiet about unused info import

            kbd
        }

        pub fn set_map(&mut self, map: KeyMap) {
            self.map = map;
        }
        pub fn get_map(&self) -> KeyMap {self.map}
        pub fn set_repeat(&mut self, rate: u32, delay: u32) {
            self.rate = rate;
            self.delay = delay;
        }
        pub fn set_chord_interval(&mut self, delay: u32) {
            self.chord_interval = delay;
        }


        /// get the column activation contents of the given row
        /// row is coded as a binary number, so the result of kbd_rowchange has to be decoded from a binary
        /// vector of rows to a set of numbers prior to using this function
        fn kbd_getrow(&self, row: u8) -> u16 {
            match row {
                0 => self.csr.rf(utra::keyboard::ROW0DAT_ROW0DAT) as u16,
                1 => self.csr.rf(utra::keyboard::ROW1DAT_ROW1DAT) as u16,
                2 => self.csr.rf(utra::keyboard::ROW2DAT_ROW2DAT) as u16,
                3 => self.csr.rf(utra::keyboard::ROW3DAT_ROW3DAT) as u16,
                4 => self.csr.rf(utra::keyboard::ROW4DAT_ROW4DAT) as u16,
                5 => self.csr.rf(utra::keyboard::ROW5DAT_ROW5DAT) as u16,
                6 => self.csr.rf(utra::keyboard::ROW6DAT_ROW6DAT) as u16,
                7 => self.csr.rf(utra::keyboard::ROW7DAT_ROW7DAT) as u16,
                8 => self.csr.rf(utra::keyboard::ROW8DAT_ROW8DAT) as u16,
                _ => 0
            }
        }

        /// scan the entire key matrix and return the list of keys that are currently
        /// pressed as key codes. Return format is an option-wrapped vector of u8,
        /// which is structured as (row : col), where each of row and col are a u8.
        /// Option "none" means no keys were pressed during this scan.
        fn kbd_getcodes(&self) -> Option<Vec<RowCol, U16>> {
            let mut keys: Vec<RowCol, U16> = Vec::new();

            for r in 0..KBD_ROWS {
                let cols: u16 = self.kbd_getrow(r as u8);
                for c in 0..KBD_COLS {
                    if (cols & (1 << c)) != 0 {
                        keys.push( RowCol{r: r as _, c: c as _} ).unwrap();
                    }
                }
            }

            if keys.len() > 0 {
                Some(keys)
            } else {
                None
            }
        }

        /// update() does a software-based debounce of the keyboard switches
        /// update() is designed to be called at regular intervals (not based on keyboard interrupt)
        /// it will automatically fetch new keycodes if a change event has happened, otherwise
        /// the regular calls to update() are necessary to update the debouncer state
        ///
        /// returns a struct of (keydown, keyup) scan codes.

        pub fn update(&mut self) -> ( Option<Vec<RowCol, U16>>, Option<Vec<RowCol, U16>> ) {
            let mut downs: [[bool; KBD_COLS]; KBD_ROWS] = [[false; KBD_COLS]; KBD_ROWS];
            let mut keydowns: Vec<RowCol, U16> = Vec::new();
            let mut keyups: Vec<RowCol, U16> = Vec::new();

            // EV_PENDING_KEYPRESSED effectively does an XOR of the previous keyboard state
            // to the current state, which is why update() does not repeatedly issue results
            // for keys that are pressed & held.
            if self.csr.rf(utra::keyboard::EV_PENDING_KEYPRESSED) != 0 {
                // only do the getcodes() call if we saw a change to key state
                self.lastcode = self.kbd_getcodes();
                /*
                info!("KBD: pending detected");
                if let Some(lc) = &self.lastcode {
                    for &code in lc.iter() {
                        info!("KBD: {:?}", code);
                    }
                } else {
                    info!("KBD: lastcode is None");
                }*/
                // clear the pending bit
                self.csr.wfo(utra::keyboard::EV_PENDING_KEYPRESSED, 1);

                let elapsed = self.ticktimer.elapsed_ms() - self.timestamp;
                if elapsed <= 1 {
                    // skip debounce processing if time elapsed is too short
                    (None, None)
                } else {
                    self.timestamp = elapsed;

                    // in case a lot of time has elapsed, saturate the debounce increment at the threshold so we don't
                    // overflow the debounce counter's u8
                    let increment: u8;
                    if elapsed > self.threshold as u64 {
                        increment = self.threshold;
                    } else {
                        increment = elapsed as u8;
                    }

                    // if there's keys pressed, continue to increment the debounce counter
                    if let Some(code) = &self.lastcode {
                        for &rc in code.iter() {
                            let row = rc.r as usize; let col = rc.c as usize;
                            if self.debounce[row][col] < self.threshold {
                                self.debounce[row][col] += increment;
                                // now check if we've passed the debounce threshold, and report a keydown
                                if self.debounce[row][col] >= self.threshold {
                                    keydowns.push(RowCol{r: row as _, c: col as _}).expect("KBD hw: probably ran out of space to track keydowns");
                                }
                            }
                            downs[row][col] = true;  // record that we processed the key
                        }
                    }

                    // now decrement debounce couter for all elements that don't have a press
                    for r in 0..KBD_ROWS {
                        for c in 0..KBD_COLS {
                            if !downs[r][c] && (self.debounce[r][c] > 0) {
                                if self.debounce[r][c] >= increment {
                                    self.debounce[r][c] -= increment;
                                } else {
                                    self.debounce[r][c] = 0;
                                }

                                if self.debounce[r][c] == 0 {
                                    keyups.push(RowCol{r: r as _, c: c as _}).expect("KBD hw: probably ran out of space to track keyups");
                                }
                            }
                        }
                    }

                    let retdowns: Option<Vec<RowCol, U16>>;
                    if keydowns.len() > 0 {
                        retdowns = Some(keydowns);
                    } else {
                        retdowns = None;
                    }

                    let retups: Option<Vec<RowCol, U16>>;
                    if keyups.len() > 0 {
                        retups = Some(keyups);
                    } else {
                        retups = None;
                    }

                    (retdowns, retups)
                }
            } else {
                (None, None)
            }
        }

        pub fn track_chord(&mut self, keyups: Option<Vec<RowCol, U16>>, keydowns: Option<Vec<RowCol, U16>>) -> Vec<char, U4> {
            /*
            Chording algorithm:

            1. Wait for first keydown event to happen; record as pressed in table
            2. Start chording timer
            3. Record press/unpress in table
            4. Wait for chording timer to timeout
            5. Extract chord state and turn into scancode using lookup table
            6. Return scancodes
             */
            if let Some(kds) = &keydowns {
                for &rc in kds.iter() {
                    self.chord[rc.r as usize][rc.c as usize] = true;
                }
            }
            if let Some(kus) = &keyups {
                for &rc in kus.iter() {
                    self.chord[rc.r as usize][rc.c as usize] = false;
                }
            }

            let mut keystates: Vec<char, U4> = Vec::new();

            if self.chord_active || keydowns.is_some() {
                let now = self.ticktimer.elapsed_ms();

                if !self.chord_active && keydowns.is_some() {
                    self.chord_active = true;
                    self.chord_timestamp = now;
                } else if self.chord_active && ((now - self.chord_timestamp) >= self.chord_interval as u64) {
                    // extract chord state
                    /*
                     keyboard:
                        2 1 0 space 3 4 5
                     braille dots:
                        0 3
                        1 4
                        2 5
                    */
                    let keys: [bool; 6] = [
                        self.chord[5][7],
                        self.chord[4][8],
                        self.chord[3][9],
                        self.chord[1][2],
                        self.chord[0][1],
                        self.chord[8][0],
                    ];
                    let mut keycode: usize = 0;
                    for i in 0..keys.len() {
                        if keys[i] {
                            keycode |= 1 << i;
                        }
                    }
                    let keychar = match keycode {
                        0b000_001 => 'a',
                        0b000_011 => 'b',
                        0b001_001 => 'c',
                        0b011_001 => 'd',
                        0b010_001 => 'e',
                        0b001_011 => 'f',
                        0b011_011 => 'g',
                        0b010_011 => 'h',
                        0b001_010 => 'i',
                        0b011_010 => 'j',

                        0b000_101 => 'k',
                        0b000_111 => 'l',
                        0b001_101 => 'm',
                        0b011_101 => 'n',
                        0b010_101 => 'o',
                        0b001_111 => 'p',
                        0b011_111 => 'q',
                        0b010_111 => 'r',
                        0b001_110 => 's',
                        0b011_110 => 't',

                        0b100_101 => 'u',
                        0b100_111 => 'v',
                        0b101_101 => 'x',
                        0b111_101 => 'y',
                        0b110_101 => 'z',
                        //0b101_111 => '',
                        //0b111_111 => '',
                        //0b1010_111 => '',
                        //0b101_110 => '',
                        0b111_010 => 'w',
                        _ => '🈯',
                    };
                    if keychar != '🈯' {
                        keystates.push(keychar).unwrap();
                    }

                    let up = self.chord[6][4];
                    if up { keystates.push('↑').unwrap(); }

                    let left = self.chord[8][3];
                    if left { keystates.push('←').unwrap(); }
                    let right = self.chord[3][6];
                    if right { keystates.push('→').unwrap(); }
                    let down = self.chord[8][2];
                    if down { keystates.push('↓').unwrap(); }
                    let center = self.chord[5][2];
                    if center { keystates.push('∴').unwrap(); }

                    let space = self.chord[2][3];
                    if space { keystates.push(' ').unwrap(); }

                    let esc = self.chord[8][6];
                    if esc { keystates.push('🔙').unwrap(); }
                    let func = self.chord[7][5];
                    if func { keystates.push('🏁').unwrap(); }
                }
            }
            // not sure if this is the right way to handle keyups, but let's try it.
            if keyups.is_some() {
                self.chord_active = false;
            }

            keystates
        }

        pub fn track_keys(&mut self, keyups: Option<Vec<RowCol, U16>>, keydowns: Option<Vec<RowCol, U16>>) -> Vec<char, U4> {
            /*
              "conventional" keyboard algorithm. The goals of this are to differentiate
              the cases of "shift", "alt", and "hold".

              thus, we check for the special-case of shift/alt in the keydowns/keyups vectors, and
              track them as separate modifiers

              then for all others, we note the down time, and compare it to the current time
              to determine if a "hold" modifier applies
             */
            let mut ks: Vec<char, U4> = Vec::new();

            // first check for shift and alt keys
            if let Some(kds) = &keydowns {
                for &rc in kds.iter() {
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
                                    //info!("KBD: shift down true");
                                    self.shift_down = true;
                                } else {
                                    //info!("KBD: shift up false");
                                    self.shift_up = false;
                                }
                            }
                        }
                    }
                }
            }
            let keyups_noshift: Option<Vec<RowCol, U16>> =
                if let Some(kus) = &keyups {
                    let mut ku_ns: Vec<RowCol, U16> = Vec::new();
                    for &rc in kus.iter() {
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
                                    ku_ns.push(RowCol{r: rc.r as _, c: rc.c as _}).unwrap();
                                }
                            },
                            _ => { // the rest just have one color of shift
                                if ((rc.r == 8) && (rc.c == 5)) || ((rc.r == 8) && (rc.c == 9)) {
                                    // only set the shift-up if we didn't previously clear it with a double-tap of shift
                                    if self.shift_down {
                                        //info!("KBD: shift up true");
                                        self.shift_up = true;
                                    }
                                    //info!("KBD: shift down false");
                                    self.shift_down = false;
                                } else {
                                    //info!("KBD: adding non-shift entry {:?}", rc);
                                    ku_ns.push(RowCol{r: rc.r as _, c: rc.c as _}).unwrap();
                                }
                            }
                        }
                    }
                    Some(ku_ns)
                } else {
                    None
                };

            // interpret keys in the context of the shift/alt modifiers
            if let Some(kds) = &keydowns {
                self.chord_timestamp = self.ticktimer.elapsed_ms();
                // if more than one is held, the key that gets picked for the repeat function is arbitrary!
                for &rc in kds.iter() {
                    let code = match self.map {
                        KeyMap::Qwerty => map_qwerty(rc),
                        KeyMap::Dvorak => map_dvorak(rc),
                        _ => ScanCode {key: None, shift: None, hold: None, alt: None},
                    };
                    if code.hold == None { // if there isn't a pre-defined meaning if the key is held, it's a repeating key
                        if let Some(key) = code.key {
                            self.repeating_key = Some(key);
                        }
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

            fn report_ok(k: char) -> Result<(), ()> { error!("KBD: ran out of space saving char: {}", k); Ok(()) }

            if let Some(kus) = &keyups_noshift {
                for &rc in kus.iter() {
                    // info!("KBD: interpreting keyups_noshift entry {:?}", rc);
                    let code = match self.map {
                        KeyMap::Qwerty => map_qwerty(rc),
                        KeyMap::Dvorak => map_dvorak(rc),
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
                                    ks.push(shiftcode).or_else(report_ok).ok();
                                } else if let Some(keycode) = code.key {
                                    ks.push(keycode).or_else(report_ok).ok();
                                }
                                self.shift_down = false;
                                self.shift_up = false;
                            } else if self.alt_down || self.alt_up {
                                if let Some(altcode) = code.alt {
                                    ks.push(altcode).or_else(report_ok).ok();
                                } else if let Some(shiftcode) = code.shift {
                                    ks.push(shiftcode).or_else(report_ok).ok();
                                } else if let Some(keycode) = code.key {
                                    ks.push(keycode).or_else(report_ok).ok();
                                }
                                self.alt_down = false;
                                self.alt_up = false;
                            } else if hold {
                                if let Some(holdcode) = code.hold {
                                    ks.push(holdcode).or_else(report_ok).ok();
                                }
                            } else {
                                if let Some(keycode) = code.key {
                                    ks.push(keycode).or_else(report_ok).ok();
                                }
                            }
                        },
                        _ => {
                            if self.shift_down || self.alt_down || self.shift_up || self.alt_up {
                                if let Some(shiftcode) = code.shift {
                                    ks.push(shiftcode).or_else(report_ok).ok();
                                } else if let Some(keycode) = code.key {
                                    ks.push(keycode).or_else(report_ok).ok();
                                }
                                self.shift_down = false;
                                self.alt_down = false;
                                self.shift_up = false;
                                self.alt_up = false;
                            } else if hold {
                                if let Some(holdcode) = code.hold {
                                    ks.push(holdcode).or_else(report_ok).ok();
                                }
                            } else {
                                if let Some(keycode) = code.key {
                                    // info!("KBD: appeding normal key '{}'", keycode);
                                    ks.push(keycode).or_else(report_ok).ok();
                                }
                            }
                        }
                    }
                }
            }

            // if we're in a key hold state, we've passed the rate timestamp point, and there's a repeating key defined
            if hold && ((now - self.rate_timestamp) >= self.rate as u64) && self.repeating_key.is_some() {
                self.rate_timestamp = now;
                if let Some(repeatkey) = self.repeating_key {
                    ks.push(repeatkey).or_else(report_ok).ok();
                }
            }

            ks
        }
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "none"))]
mod implementation {
    use heapless::Vec;
    use heapless::consts::*;
    use crate::api::*;
    //use crate::{map_dvorak, map_qwerty};
    //use log::{error, info};

    #[allow(dead_code)]
    pub struct Keyboard {
        map: KeyMap,
        rate: u32,
        delay: u32,
        chord_interval: u32,
    }

    impl Keyboard {
        pub fn new() -> Keyboard {
            Keyboard {
                map: KeyMap::Qwerty,
                rate: 20,
                delay: 200,
                chord_interval: 50,
            }
        }

        pub fn set_map(&mut self, map: KeyMap) {
            self.map = map;
        }
        pub fn get_map(&self) -> KeyMap {self.map}

        pub fn update(&self) -> ( Option<Vec<RowCol, U16>>, Option<Vec<RowCol, U16>> ) {
            (None, None)
        }

        pub fn track_chord(&mut self, _keyups: Option<Vec<RowCol, U16>>, _keydowns: Option<Vec<RowCol, U16>>) -> Vec<char, U4> {
            Vec::new()
        }

        pub fn track_keys(&mut self, _keyups: Option<Vec<RowCol, U16>>, _keydowns: Option<Vec<RowCol, U16>>) -> Vec<char, U4> {
            Vec::new()
        }

        pub fn set_repeat(&mut self, rate: u32, delay: u32) {
            self.rate = rate;
            self.delay = delay;
        }

        pub fn set_chord_interval(&mut self, delay: u32) {
            self.chord_interval = delay;
        }
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    use crate::implementation::Keyboard;

    log_server::init_wait().unwrap();
    info!("KBD: my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let kbd_sid = xns.register_name(xous::names::SERVER_NAME_KBD).expect("KBD: can't register server");
    info!("KBD: registered with NS -- {:?}", kbd_sid);

    // Create a new kbd object
    let mut kbd = Keyboard::new();

    let mut normal_conns: Vec<xous::CID, U64> = Vec::new();
    let mut raw_conns: Vec<xous::CID, U64> = Vec::new();

    info!("KBD: starting main loop");
    #[cfg(not(target_os = "none"))]
    let mut injected_keys: Queue<char, U64, _> = Queue::u8();
    #[cfg(not(target_os = "none"))]
    let (mut key_enqueue, mut key_dequeue) = injected_keys.split();

    loop {
        let maybe_env = xous::try_receive_message(kbd_sid).unwrap();
        match maybe_env {
            Some(envelope) => {
                info!("KBD: Message: {:?}", envelope);
                if let xous::Message::Borrow(m) = &envelope.body {
                    let buf = unsafe { buffer::XousBuffer::from_memory_message(m) };
                    let bytes = Pin::new(buf.as_ref());
                    let value = unsafe {
                        archived_value::<api::Opcode>(&bytes, m.id as usize)
                    };
                    match &*value {
                        rkyv::Archived::<api::Opcode>::RegisterListener(registration) => {
                            let cid = xns.request_connection_blocking(registration.as_str()).expect("KBD: can't connect to requested listener for reporting events");
                            normal_conns.push(cid).expect("KBD: probably ran out of slots for keyboard event reporting");
                        },
                        rkyv::Archived::<api::Opcode>::RegisterRawListener(registration) => {
                            let cid = xns.request_connection_blocking(registration.as_str()).expect("KBD: can't connect to requested listener for reporting events");
                            raw_conns.push(cid).expect("KBD: probably ran out of slots for raw keyboard event reporting");
                        },
                        _ => panic!("Invalid memory message response -- corruption occurred"),
                    };
                } else if let Ok(opcode) = Opcode::try_from(&envelope.body) {
                    match opcode {
                        Opcode::SelectKeyMap(map) => {
                            kbd.set_map(map);
                        },
                        Opcode::SetRepeat(rate, delay) => {
                            kbd.set_repeat(rate, delay);
                        },
                        Opcode::SetChordInterval(delay) => {
                            kbd.set_chord_interval(delay);
                        },
                        Opcode::KeyboardEvent(_keys) => {
                            error!("KBD: somehow received an outgoing event code, this shouldn't happen!");
                        },
                        Opcode::HostModeInjectKey(key) => {
                            info!("KBD: injecting emulation key press '{}'", key);
                            #[cfg(not(target_os = "none"))]
                            key_enqueue.enqueue(key).unwrap();
                        },
                        _ => panic!("KBD: received unknown opcode")
                    }
                } else {
                    error!("KBD: couldn't convert opcode");
                }
            }
            _ => xous::yield_slice(),
        }

        let (keydowns, keyups) = kbd.update();
        /*
        if let Some(ku) = &keyups {
            for &kus in ku.iter() {
                info!("KBD: keyup r{} c{}", kus.r, kus.c);
            }
        }
        if let Some(kd) = &keydowns {
            for &kds in kd.iter() {
                info!("KBD: keydown r{} c{}", kds.r, kds.c);
            }
        }
        */

        // this code is totally untested
        if keyups.is_some() || keydowns.is_some() {
            // send the raw codes
            for conn in raw_conns.iter() {
                let mut rs: KeyRawStates = KeyRawStates::new();
                if let Some(ku) = &keyups {
                    for &k in ku.iter() {
                        unsafe { // because rs is "repr packed"
                            rs.keyups.push(k).unwrap();
                        }
                    }
                }
                if let Some(kd) = &keydowns {
                    for &k in kd.iter() {
                        unsafe {
                            rs.keydowns.push(k).unwrap();
                        }
                    }
                }
                let mut writer = rkyv::ser::serializers::BufferSerializer::new(xous::XousBuffer::new(4096));
                let pos = {
                    use rkyv::ser::Serializer;
                    writer.serialize_value(&rs).expect("KBD: couldn't archive request")
                };
                let xous_buffer = writer.into_inner();
                xous_buffer.send(*conn, pos as u32).expect("KBD: can't send raw code");
            }
        }

        // interpret scan codes
        // the track_* functions track the keyup/keydowns to modify keys with shift, hold, and chord state
        let kc: Vec<char, U4> = match kbd.get_map() {
            KeyMap::Braille => {
                kbd.track_chord(keyups, keydowns)
            },
            _ => {
                kbd.track_keys(keyups, keydowns)
            },
        };

        // this is used for hosted mode emulation injection of keys
        #[cfg(not(target_os = "none"))]
        {
            let mut keys: [char; 4] = ['\u{0000}', '\u{0000}', '\u{0000}', '\u{0000}'];
            let mut i = 0;
            while let Some(c) = key_dequeue.dequeue() {
                keys[i] = c;
                i = i + 1;
                if i == 4 { break; } // see https://github.com/rust-lang/rfcs/pull/2497 for why this can't be at the top of the loop conditional
            };
            if i != 0 {
                for conn in normal_conns.iter() {
                    xous::send_message(*conn, api::Opcode::KeyboardEvent(keys).into()).map(|_| ()).expect("KBD: Couldn't send event to listener");
                }
            }
        }

        // send keys, if any
        if kc.len() > 0 {
            let mut keys: [char; 4] = ['\u{0000}', '\u{0000}', '\u{0000}', '\u{0000}'];
            for i in 0..kc.len() {
                // info!("KBD: sending key '{}'", kc[i]);
                keys[i] = kc[i];
            }
            for conn in normal_conns.iter() {
                xous::send_message(*conn, api::Opcode::KeyboardEvent(keys).into()).map(|_| ()).expect("KBD: Couldn't send event to listener");
            }
        }
    }
    // this shuts up the keyboard thread from polluting the kernel logs when panics happen elsewhere
    //loop {xous::receive_message(kbd_sid).unwrap(); xous::yield_slice();}
}
