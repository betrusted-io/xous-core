use crate::{RowCol, ScanCode};

/// Compute the qwertz key mapping of row/col to key tuples
#[rustfmt::skip]
pub(crate) fn map_qwertz(code: RowCol) -> ScanCode {
    let rc = (code.r, code.c);

    match rc {
        (0, 0) => ScanCode{key: Some('1'), shift: Some('1'), hold: Some('!'), alt: None},
        (0, 1) => ScanCode{key: Some('2'), shift: Some('2'), hold: Some('"'), alt: None},
        (0, 2) => ScanCode{key: Some('3'), shift: Some('3'), hold: Some('§'), alt: None},
        (0, 3) => ScanCode{key: Some('4'), shift: Some('4'), hold: Some('$'), alt: None},
        (0, 4) => ScanCode{key: Some('5'), shift: Some('5'), hold: Some('%'), alt: None},
        (4, 5) => ScanCode{key: Some('6'), shift: Some('6'), hold: Some('&'), alt: None},
        (4, 6) => ScanCode{key: Some('7'), shift: Some('7'), hold: Some('/'), alt: None},
        (4, 7) => ScanCode{key: Some('8'), shift: Some('8'), hold: Some('('), alt: None},
        (4, 8) => ScanCode{key: Some('9'), shift: Some('9'), hold: Some(')'), alt: None},
        (4, 9) => ScanCode{key: Some('0'), shift: Some('0'), hold: Some('='), alt: None},

        (1, 0) => ScanCode{key: Some('q'), shift: Some('Q'), hold: Some('@'), alt: None},
        (1, 1) => ScanCode{key: Some('w'), shift: Some('W'), hold: Some('ß'), alt: None},
        (1, 2) => ScanCode{key: Some('e'), shift: Some('E'), hold: Some('€'), alt: None},
        (1, 3) => ScanCode{key: Some('r'), shift: Some('R'), hold: Some('^'), alt: None},
        (1, 4) => ScanCode{key: Some('t'), shift: Some('T'), hold: Some('¡'), alt: None},
        (5, 5) => ScanCode{key: Some('z'), shift: Some('Z'), hold: Some('¿'), alt: None},
        (5, 6) => ScanCode{key: Some('u'), shift: Some('U'), hold: Some('ü'), alt: None},
        (5, 7) => ScanCode{key: Some('i'), shift: Some('I'), hold: Some('~'), alt: None},
        (5, 8) => ScanCode{key: Some('o'), shift: Some('O'), hold: Some('ö'), alt: None},
        (5, 9) => ScanCode{key: Some('p'), shift: Some('P'), hold: Some('#'), alt: None},

        (2, 0) => ScanCode{key: Some('a'), shift: Some('A'), hold: Some('ä'), alt: None},
        (2, 1) => ScanCode{key: Some('s'), shift: Some('S'), hold: Some('['), alt: None},
        (2, 2) => ScanCode{key: Some('d'), shift: Some('D'), hold: Some(']'), alt: None},
        (2, 3) => ScanCode{key: Some('f'), shift: Some('F'), hold: Some('*'), alt: None},
        (2, 4) => ScanCode{key: Some('g'), shift: Some('G'), hold: Some('-'), alt: None},
        (6, 5) => ScanCode{key: Some('h'), shift: Some('H'), hold: Some('+'), alt: None},
        (6, 6) => ScanCode{key: Some('j'), shift: Some('J'), hold: Some('\\'), alt: None},
        (6, 7) => ScanCode{key: Some('k'), shift: Some('K'), hold: Some('{'), alt: None},
        (6, 8) => ScanCode{key: Some('l'), shift: Some('L'), hold: Some('}'), alt: None},
        (6, 9) => ScanCode{key: Some(0x8_u8.into()), shift: Some(0x8_u8.into()), hold: None /* hold of none -> repeat */, alt: Some(0x8_u8.into())},  // backspace

        (3, 0) => ScanCode{key: None, shift: Some('?'), hold: Some('?'), alt: None},
        (3, 1) => ScanCode{key: Some('y'), shift: Some('Y'), hold: Some('|'), alt: None},
        (3, 2) => ScanCode{key: Some('x'), shift: Some('X'), hold: Some('_'), alt: None},
        (3, 3) => ScanCode{key: Some('c'), shift: Some('C'), hold: Some('`'), alt: None},
        (3, 4) => ScanCode{key: Some('v'), shift: Some('V'), hold: Some('\''), alt: None},
        (7, 5) => ScanCode{key: Some('b'), shift: Some('B'), hold: Some(':'), alt: None},
        (7, 6) => ScanCode{key: Some('n'), shift: Some('N'), hold: Some(';'), alt: None},
        (7, 7) => ScanCode{key: Some('m'), shift: Some('M'), hold: Some('µ'), alt: None},
        (7, 8) => ScanCode{key: Some('<'), shift: Some('<'), hold: Some('>'), alt: None},
        (7, 9) => ScanCode{key: Some(0xd_u8.into()), shift: Some(0xd_u8.into()), hold: Some(0xd_u8.into()), alt: Some(0xd_u8.into())}, // carriage return

        (8, 5) => ScanCode{key: Some(0xf_u8.into()), shift: Some(0xf_u8.into()), hold: Some(0xf_u8.into()), alt: Some(0xf_u8.into())}, // shift in (blue shift)
        (8, 6) => ScanCode{key: Some(','), shift: Some(0xe_u8.into()), hold: Some('福'), alt: None},  // 0xe is shift out (sym) '富' -> just for testing hanzi plane
        (8, 7) => ScanCode{key: Some(' '), shift: Some(' '), hold: None /* hold of none -> repeat */, alt: None},
        (8, 8) => ScanCode{key: Some('.'), shift: Some('😊'), hold: Some('😊'), alt: None},
        (8, 9) => ScanCode{key: Some(0xf_u8.into()), shift: Some(0xf_u8.into()), hold: Some(0xf_u8.into()), alt: Some(0xf_u8.into())}, // shift in (blue shift)

        // the F0/tab key also doubles as a secondary power key (can't do UP5K UART rx at same time)
        (8, 0) => ScanCode{key: Some(0x11_u8.into()), shift: Some(0x11_u8.into()), hold: Some('\t'), alt: Some(0x11_u8.into())}, // DC1 (F1)
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
