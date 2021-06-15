#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use heapless::Vec;

use log::info;

use num_traits::{FromPrimitive, ToPrimitive};
use xous_ipc::Buffer;
use api::{Opcode, KeyRawStates};
use xous::{CID, msg_scalar_unpack};

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
    use utralib::generated::*;
    use crate::api::*;
    use crate::{map_dvorak, map_qwerty};
    use ticktimer_server::Ticktimer;
    use xous::CID;
    use num_traits::ToPrimitive;
    use susres::{RegManager, RegOrField, SuspendResume};

    use heapless::Vec;

    /// note: the code is structured to use at most 16 rows or 16 cols
    const KBD_ROWS: usize = 9;
    const KBD_COLS: usize = 10;

    pub struct Keyboard {
        conn: CID,
        csr: utralib::CSR<u32>,
        /// where the interrupt handler copies the new state
        new_state: RowColVec,
        /// remember the last key states
        last_state: RowColVec,
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
    }

    fn handle_kbd(_irq_no: usize, arg: *mut usize) {
        let kbd = unsafe { &mut *(arg as *mut Keyboard) };
        if kbd.csr.rf(utra::keyboard::EV_PENDING_KEYPRESSED) != 0 {
            kbd.new_state = kbd.kbd_getcodes();
            kbd.csr.wfo(utra::keyboard::EV_PENDING_KEYPRESSED, 1); // clear the interrupt

            xous::try_send_message(kbd.conn,
                xous::Message::new_scalar(Opcode::HandlerTrigger.to_usize().unwrap(), 0, 0, 0, 0)).ok();
        }
        if kbd.csr.rf(utra::keyboard::EV_PENDING_INJECT) != 0 {
            let c = kbd.csr.rf(utra::keyboard::UART_CHAR_CHAR);
            kbd.csr.wfo(utra::keyboard::EV_PENDING_INJECT, 1); // clear the interrupt
            xous::try_send_message(kbd.conn,
                xous::Message::new_scalar(Opcode::InjectKey.to_usize().unwrap(), c as _, 0, 0, 0)
            ).ok();
        }
    }

    impl Keyboard {
        pub fn new(sid: xous::SID) -> Keyboard {
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
                new_state: RowColVec::new(),
                last_state: RowColVec::new(),
                ticktimer,
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
                chord_active: 0,
                chord_captured: false,
                susres: RegManager::new(csr.as_mut_ptr() as *mut u32),
            };

            xous::claim_interrupt(
                utra::keyboard::KEYBOARD_IRQ,
                handle_kbd,
                (&mut kbd) as *mut Keyboard as *mut usize,
            )
            .expect("couldn't claim irq");
            kbd.csr.wo(utra::keyboard::EV_PENDING, kbd.csr.r(utra::keyboard::EV_PENDING)); // clear in case it's pending for some reason
            kbd.csr.wo(utra::keyboard::EV_ENABLE,
                kbd.csr.ms(utra::keyboard::EV_ENABLE_KEYPRESSED, 1) |
                kbd.csr.ms(utra::keyboard::EV_ENABLE_INJECT, 1)
            );
            log::trace!("hardware initialized");

            kbd.susres.push_fixed_value(RegOrField::Reg(utra::keyboard::EV_PENDING), 0xFFFF_FFFF);
            kbd.susres.push(RegOrField::Reg(utra::keyboard::EV_ENABLE), None);

            kbd
        }

        pub fn suspend(&mut self) {
            self.susres.suspend();
            self.csr.wo(utra::keyboard::EV_ENABLE, 0);
        }
        pub fn resume(&mut self) {
            self.susres.resume();
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
        /// pressed as key codes. Return format is an array of option-wrapped RowCol
        /// which is structured as (row : col), where each of row and col are a u8.
        /// Option "none" means no keys were pressed during this scan.
        /// This has O(N^2) time growth as number of keys are pressed; but, typically,
        /// it's rare that more than two keys are pressed at once, and most of the time, none are pressed.
        /// The 16-element limit is more allowing for an extremely likely worst case. On average, this will perform
        /// as well as an O(N) heapless Vec, but comes with the benefit of getting rid of that dependency.
        fn kbd_getcodes(&self) -> RowColVec {
            let mut keys = RowColVec::new();

            for r in 0..KBD_ROWS {
                let cols: u16 = self.kbd_getrow(r as u8);
                for c in 0..KBD_COLS {
                    if (cols & (1 << c)) != 0 {
                        // note: we could implement a check to flag if more than 16 keys were pressed within a single
                        // debounce quanta (5ms) at once...
                        // this is an unlikely scenario, so we're just going to count on ample space being the solution
                        // to this problem
                        keys.add_unchecked(RowCol{r: r as _, c: c as _});
                    }
                }
            }
            keys
        }

        pub fn update(&mut self) -> KeyRawStates {
            // EV_PENDING_KEYPRESSED effectively does an XOR of the previous keyboard state
            // to the current state, which is why update() does not repeatedly issue results
            // for keys that are pressed & held.

            if log::log_enabled!(log::Level::Trace) {
                log::trace!("pending detected");
                log::trace!("{:?}", self.new_state);
            }
            let new_codes = self.new_state;

            let mut keydowns = RowColVec::new();
            let mut keyups = RowColVec::new();

            // check to see if there are codes in the last state that aren't in the current codes
            for i in 0..self.last_state.len() {
                if let Some(lastcode) = self.last_state.get(i) {
                    if !new_codes.contains(lastcode) {
                        keyups.add_rc(lastcode).unwrap();
                        self.last_state.set(i, None);
                    }
                }
            }
            // check to see if the codes in the current set aren't already in the current codes
            for i in 0..new_codes.len() {
                if let Some(rc) = new_codes.get(i) {
                    match self.last_state.add_rc(rc) {
                        Ok(true) => {
                            keydowns.add_rc(rc).unwrap();
                        },
                        Ok(false) => {
                            // already in the array, don't report anything as it's not a change
                        },
                        _ => log::error!("out of key storage state in keyboard update")
                    }
                }
            }

            let krs = KeyRawStates {
                keydowns,
                keyups,
            };

            krs
        }

        pub fn track_chord(&mut self, keyups: Option<Vec<RowCol, 16>>, keydowns: Option<Vec<RowCol, 16>>) -> Vec<char, 4> {
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
            if let Some(kds) = &keydowns {
                for &rc in kds.iter() {
                    self.chord[rc.r as usize][rc.c as usize] = true;
                    self.chord_active += 1;
                }
            }
            log::trace!("self.chord: {:?}", self.chord);
            let mut keystates: Vec<char, 4> = Vec::new();

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
                    keystates.push(key).unwrap();
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
                let bs: char = 0x8_u8.into();  // back space
                if esc { keystates.push(bs).unwrap(); }

                let func = self.chord[7][5];
                let cr: char = 0xd_u8.into();  // carriage return
                if func { keystates.push(cr).unwrap(); }

                log::trace!("up {}, left {}, right {}, down, {}, center, {}, space {}, esc {}, func {}",
                    up, left, right, down, center, space, esc, func);
            }
            if let Some(kus) = &keyups {
                for &rc in kus.iter() {
                    self.chord[rc.r as usize][rc.c as usize] = false;
                    if self.chord_active > 0 {
                        self.chord_active -= 1;
                    } else {
                        log::error!("received more keyups than we had keydowns!")
                    }
                }
            }
            if self.chord_active == 0 {
                self.chord_captured = false;
            }

            keystates
        }

        pub fn track_keys(&mut self, keyups: Option<Vec<RowCol, 16>>, keydowns: Option<Vec<RowCol, 16>>) -> Vec<char, 4> {
            /*
              "conventional" keyboard algorithm. The goals of this are to differentiate
              the cases of "shift", "alt", and "hold".

              thus, we check for the special-case of shift/alt in the keydowns/keyups vectors, and
              track them as separate modifiers

              then for all others, we note the down time, and compare it to the current time
              to determine if a "hold" modifier applies
             */
            let mut ks: Vec<char, 4> = Vec::new();

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
            }
            let keyups_noshift: Option<Vec<RowCol, 16>> =
                if let Some(kus) = &keyups {
                    let mut ku_ns: Vec<RowCol, 16> = Vec::new();
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
                                        //info!("shift up true");
                                        self.shift_up = true;
                                    }
                                    //info!("shift down false");
                                    self.shift_down = false;
                                } else {
                                    //info!("adding non-shift entry {:?}", rc);
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

            fn report_ok(k: char) -> Result<(), ()> { log::error!("ran out of space saving char: {}", k); Ok(()) }

            if let Some(kus) = &keyups_noshift {
                for &rc in kus.iter() {
                    // info!("interpreting keyups_noshift entry {:?}", rc);
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
                                    // info!("appeding normal key '{}'", keycode);
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
    use crate::api::*;
    //use crate::{map_dvorak, map_qwerty};

    #[allow(dead_code)]
    pub struct Keyboard {
        cid: xous::CID,
        map: KeyMap,
        rate: u32,
        delay: u32,
        chord_interval: u32,
    }

    impl Keyboard {
        pub fn new(sid: xous::SID) -> Keyboard {
            Keyboard {
                cid: xous::connect(sid).unwrap(),
                map: KeyMap::Qwerty,
                rate: 20,
                delay: 200,
                chord_interval: 50,
            }
        }
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

        pub fn track_chord(&mut self, _keyups: Option<Vec<RowCol, 16>>, _keydowns: Option<Vec<RowCol, 16>>) -> Vec<char, 4> {
            Vec::new()
        }

        pub fn track_keys(&mut self, _keyups: Option<Vec<RowCol, 16>>, _keydowns: Option<Vec<RowCol, 16>>) -> Vec<char, 4> {
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

fn send_rawstates(cb_conns: &mut [Option<CID>; 16], krs: &KeyRawStates) {
    for maybe_conn in cb_conns.iter_mut() {
        if let Some(conn) = maybe_conn {
            let buf = Buffer::into_buf(*krs).unwrap();
            match buf.lend(*conn, Callback::KeyRawEvent.to_u32().unwrap()) {
                Err(xous::Error::ServerNotFound) => {
                    *maybe_conn = None
                },
                Ok(xous::Result::Ok) => {},
                _ => panic!("unhandled error or result in callback processing"),
            }
        }
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    use crate::implementation::Keyboard;
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // connections expected:
    //  - GAM
    //  - shellchat (for vibe demo)
    //  - graphics (if building for hosted mode)
    #[cfg(target_os = "none")]
    let kbd_sid = xns.register_name(api::SERVER_NAME_KBD, Some(2)).expect("can't register server");
    #[cfg(not(target_os = "none"))]
    let kbd_sid = xns.register_name(api::SERVER_NAME_KBD, Some(3)).expect("can't register server");
    log::trace!("registered with NS -- {:?}", kbd_sid);

    // Create a new kbd object
    let mut kbd = Keyboard::new(kbd_sid);

    // register a suspend/resume listener
    let sr_cid = xous::connect(kbd_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(&xns, Opcode::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");

    let mut normal_conns: [Option<CID>; 16] = [None; 16];
    let mut raw_conns: [Option<CID>; 16] = [None; 16];

    let mut vibe = false;
    let llio = llio::Llio::new(&xns).unwrap();

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
            Some(Opcode::RegisterListener) => msg_scalar_unpack!(msg, sid0, sid1, sid2, sid3, {
                let sid = xous::SID::from_u32(sid0 as _, sid1 as _, sid2 as _, sid3 as _);
                let cid = Some(xous::connect(sid).unwrap());
                let mut found = false;
                for entry in normal_conns.iter_mut() {
                    if *entry == None {
                        *entry = cid;
                        found = true;
                        break;
                    }
                }
                if !found {
                    log::error!("RegisterListener ran out of space registering callback");
                }
            }),
            Some(Opcode::RegisterRawListener) => msg_scalar_unpack!(msg, sid0, sid1, sid2, sid3, {
                let sid = xous::SID::from_u32(sid0 as _, sid1 as _, sid2 as _, sid3 as _);
                let cid = Some(xous::connect(sid).unwrap());
                let mut found = false;
                for entry in raw_conns.iter_mut() {
                    if *entry == None {
                        *entry = cid;
                        found = true;
                        break;
                    }
                }
                if !found {
                    log::error!("RegisterListener ran out of space registering callback");
                }
            }),
            Some(Opcode::SelectKeyMap) => msg_scalar_unpack!(msg, km, _, _, _, {
                kbd.set_map(KeyMap::from(km))
            }),
            Some(Opcode::SetRepeat) => msg_scalar_unpack!(msg, rate, delay, _, _, {
                kbd.set_repeat(rate as u32, delay as u32);
            }),
            Some(Opcode::SetChordInterval) => msg_scalar_unpack!(msg, delay, _, _, _, {
                kbd.set_chord_interval(delay as u32);
            }),
            Some(Opcode::InjectKey) => msg_scalar_unpack!(msg, _k, _, _, _, {
                let key = if let Some(a) = core::char::from_u32(_k as u32) {
                    a
                } else {
                    '\u{0000}'
                };
                info!("injecting emulation key press '{}'", key);
                for maybe_conn in normal_conns.iter_mut() {
                    if let Some(conn) = maybe_conn {
                        match xous::send_message(*conn,
                            xous::Message::new_scalar(api::Callback::KeyEvent.to_usize().unwrap(),
                            key as u32 as usize,
                            '\u{0000}' as u32 as usize,
                            '\u{0000}' as u32 as usize,
                            '\u{0000}' as u32 as usize,
                        )) {
                            Err(xous::Error::ServerNotFound) => {
                                *maybe_conn = None
                            },
                            Ok(xous::Result::Ok) => {},
                            _ => log::error!("unhandled error in key event sending")
                        }
                    }
                }
            }),
            Some(Opcode::HandlerTrigger) => {
                let rawstates = kbd.update();

                // send rawstates on to rawstate listeners
                send_rawstates(&mut raw_conns, &rawstates);

                // GROSS: we copy our IPC-compatible RowColVec type into a heapless::Vec for further processing
                // this code can probably be cleaned up and improved quite a bit to reduce CPU time.
                // We don't use heapless::Vec for the IPC because we can't serialize the object with a trait natively
                // which leaves us with ugly/dangerous slice-of-u8 cast techniques and/or inefficient serialization
                // methods that rely on effectively a substantially similar intermediate array-based implementation.
                // We retain the use of the "Vec" type here because the Vec type has some nice functions like iterators,
                // and push(), len() etc. which doesn't really make sense to re-implement. But, maybe someone much
                // more clever than I could do extend RowColVec to have these feature and get rid of this ugly copy
                // step and make the rest of this routine operate natively on RowColVec types....
                let mut keyups_core: Vec<RowCol, 16> = Vec::new();
                let mut keydowns_core: Vec<RowCol, 16> = Vec::new();
                for i in 0..rawstates.keyups.len() {
                    if let Some(rc) = rawstates.keyups.get(i) {
                        keyups_core.push(rc).unwrap();
                    }
                }
                for i in 0..rawstates.keydowns.len() {
                    if let Some(rc) = rawstates.keydowns.get(i) {
                        keydowns_core.push(rc).unwrap();
                    }
                }
                let keyups = if keyups_core.len() > 0 {
                    Some(keyups_core)
                } else {
                    None
                };
                let keydowns = if keydowns_core.len() > 0 {
                    Some(keydowns_core)
                } else {
                    None
                };

                // interpret scancodes
                // the track_* functions track the keyup/keydowns to modify keys with shift, hold, and chord state
                let kc: Vec<char, 4> = match kbd.get_map() {
                    KeyMap::Braille => {
                        kbd.track_chord(keyups, keydowns)
                    },
                    _ => {
                        kbd.track_keys(keyups, keydowns)
                    },
                };

                // send keys, if any
                if kc.len() > 0 {
                    if vibe {
                        llio.vibe(llio::VibePattern::Short).unwrap();
                    }
                    let mut keys: [char; 4] = ['\u{0000}', '\u{0000}', '\u{0000}', '\u{0000}'];
                    for i in 0..kc.len() {
                        // info!("sending key '{}'", kc[i]);
                        keys[i] = kc[i];
                    }

                    for maybe_conn in normal_conns.iter_mut() {
                        if let Some(conn) = maybe_conn {
                            match xous::send_message(*conn,
                                xous::Message::new_scalar(api::Callback::KeyEvent.to_usize().unwrap(),
                                keys[0] as u32 as usize,
                                keys[1] as u32 as usize,
                                keys[2] as u32 as usize,
                                keys[3] as u32 as usize,
                            )) {
                                Err(xous::Error::ServerNotFound) => {
                                    *maybe_conn = None
                                },
                                Ok(xous::Result::Ok) => {},
                                _ => log::error!("unhandled error in key event sending")
                            }
                        }
                    }
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
