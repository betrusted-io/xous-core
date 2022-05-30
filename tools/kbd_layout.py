#! /usr/bin/env python3
import argparse

# !_#$%&-()*}w[vz0123456789SsW]VZ@AXJE>UIDCHTNMBRL"POYGK<QF:/\=^{`axje.uidchtnmbrl'poygk,qf;?|+~

# !"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\]^_`abcdefghijklmnopqrstuvwxyz{|}~
ascii_to_keycode = {
    ord(' ') : [' ', ['Space']],
    ord('!') : ['!', ['Keyboard1', 'LeftShift']],
    ord('"') : ['"', ['Apostrophe', 'LeftShift']],
    ord('#') : ['#', ['Keyboard3', 'LeftShift']],
    ord('$') : ['$', ['Keyboard4', 'LeftShift']],
    ord('%') : ['%', ['Keyboard5', 'LeftShift']],
    ord('&') : ['&', ['Keyboard7', 'LeftShift']],
    ord("'") : ["'", ['Apostrophe']],
    ord('(') : ['(', ['Keyboard9', 'LeftShift']],
    ord(')') : [')', ['Keyboard0', 'LeftShift']],
    ord('*') : ['*', ['Keyboard8', 'LeftShift']],
    ord('+') : ['+', ['Equal', 'LeftShift']],
    ord(',') : [',', ['Comma']],
    ord('-') : ['-', ['Minus']],
    ord('.') : ['.', ['Dot']],
    ord('/') : ['/', ['ForwardSlash']],
    ord('0') : ['0', ['Keyboard0']],
    ord('1') : ['1', ['Keyboard1']],
    ord('2') : ['2', ['Keyboard2']],
    ord('3') : ['3', ['Keyboard3']],
    ord('4') : ['4', ['Keyboard4']],
    ord('5') : ['5', ['Keyboard5']],
    ord('6') : ['6', ['Keyboard6']],
    ord('7') : ['7', ['Keyboard7']],
    ord('8') : ['8', ['Keyboard8']],
    ord('9') : ['9', ['Keyboard9']],
    ord(':') : [':', ['Semicolon', 'LeftShift']],
    ord(';') : [';', ['Semicolon']],
    ord('<') : ['<', ['Comma', 'LeftShift']],
    ord('=') : ['=', ['Equal']],
    ord('>') : ['>', ['Dot', 'LeftShift']],
    ord('?') : ['?', ['ForwardSlash', 'LeftShift']],
    ord('@') : ['@', ['Keyboard2', 'LeftShift']],
    ord('A') : ['A', ['A', 'LeftShift']],
    ord('B') : ['B', ['B', 'LeftShift']],
    ord('C') : ['C', ['C', 'LeftShift']],
    ord('D') : ['D', ['D', 'LeftShift']],
    ord('E') : ['E', ['E', 'LeftShift']],
    ord('F') : ['F', ['F', 'LeftShift']],
    ord('G') : ['G', ['G', 'LeftShift']],
    ord('H') : ['H', ['H', 'LeftShift']],
    ord('I') : ['I', ['I', 'LeftShift']],
    ord('J') : ['J', ['J', 'LeftShift']],
    ord('K') : ['K', ['K', 'LeftShift']],
    ord('L') : ['L', ['L', 'LeftShift']],
    ord('M') : ['M', ['M', 'LeftShift']],
    ord('N') : ['N', ['N', 'LeftShift']],
    ord('O') : ['O', ['O', 'LeftShift']],
    ord('P') : ['P', ['P', 'LeftShift']],
    ord('Q') : ['Q', ['Q', 'LeftShift']],
    ord('R') : ['R', ['R', 'LeftShift']],
    ord('S') : ['S', ['S', 'LeftShift']],
    ord('T') : ['T', ['T', 'LeftShift']],
    ord('U') : ['U', ['U', 'LeftShift']],
    ord('V') : ['V', ['V', 'LeftShift']],
    ord('W') : ['W', ['W', 'LeftShift']],
    ord('X') : ['X', ['X', 'LeftShift']],
    ord('Y') : ['Y', ['Y', 'LeftShift']],
    ord('Z') : ['Z', ['Z', 'LeftShift']],
    ord('[') : ['[', ['LeftBrace']],
    ord('\\') : ['\\', ['Backslash']],
    ord(']') : [']', ['RightBrace']],
    ord('^') : ['^', ['Keyboard6', 'LeftShift']],
    ord('_') : ['_', ['Minus', 'LeftShift']],
    ord('`') : ['`', ['Grave']],
    ord('a') : ['a', ['A']],
    ord('b') : ['b', ['B']],
    ord('c') : ['c', ['C']],
    ord('d') : ['d', ['D']],
    ord('e') : ['e', ['E']],
    ord('f') : ['f', ['F']],
    ord('g') : ['g', ['G']],
    ord('h') : ['h', ['H']],
    ord('i') : ['i', ['I']],
    ord('j') : ['j', ['J']],
    ord('k') : ['k', ['K']],
    ord('l') : ['l', ['L']],
    ord('m') : ['m', ['M']],
    ord('n') : ['n', ['N']],
    ord('o') : ['o', ['O']],
    ord('p') : ['p', ['P']],
    ord('q') : ['q', ['Q']],
    ord('r') : ['r', ['R']],
    ord('s') : ['s', ['S']],
    ord('t') : ['t', ['T']],
    ord('u') : ['u', ['U']],
    ord('v') : ['v', ['V']],
    ord('w') : ['w', ['W']],
    ord('x') : ['x', ['X']],
    ord('y') : ['y', ['Y']],
    ord('z') : ['z', ['Z']],
    ord('{') : ['{', ['LeftBrace', 'LeftShift']],
    ord('|') : ['|', ['Backslash', 'LeftShift']],
    ord('}') : ['}', ['RightBrace', 'LeftShift']],
    ord('~') : ['~', ['Grave', 'LeftShift']],
}

def main():
    parser = argparse.ArgumentParser(description="Sign binary images for Precursor")
    parser.add_argument(
        "--generate", default=False, help="generate mapping based on input", action="store_true"
    )
    args = parser.parse_args()

    testval = input("Run `usb kbdtest` into the following prompt: ")
    checkval = list(map(ord, [char for char in testval]))
    START_ASCII = 0x20
    END_ASCII_EXCLUSIVE = 0x7f
    if len(checkval) != END_ASCII_EXCLUSIVE - START_ASCII:
        print("Check string length is incorrect. Expected {} chars, got {}".format(END_ASCII_EXCLUSIVE - START_ASCII + 1, len(checkval)))
        exit(1)

    if args.generate:
        # extract mapping
        mapping_dict = {}
        for i in range(START_ASCII, END_ASCII_EXCLUSIVE):
            checkindex = i - START_ASCII
            # the intended key is chr(i)
            received_char = checkval[checkindex]
            mapping_dict[received_char] = ascii_to_keycode[i]
        # generate code
        print("    pub fn char_to_hid_code_CUSTOM(key: char) -> Vec<UsbKeyCode> {")
        print("        let mut code = vec![];")
        print("        match key {")
        for i in range(START_ASCII, END_ASCII_EXCLUSIVE):
            if chr(i) == '\\':
                print("            '\\\\' => {{".format(chr(i)), end="")
            elif chr(i) == '\'':
                print("            '\\\'' => {{".format(chr(i)), end="")
            else:
                print("            '{}' => {{".format(chr(i)), end="")
            for code in mapping_dict[i][1]:
                print("code.push(UsbKeyCode::{}); ".format(code), end="")
            print("},")
        print("            '\\u{000d}' => {}, // ignore CR")
        print("            '\\u{000a}' => code.push(UsbKeyCode::ReturnEnter), // turn LF ('\\n') into enter")
        print("            '\\u{0008}' => code.push(UsbKeyCode::DeleteBackspace),")
        print("            _ => log::warn!(\"Ignoring unhandled character: {}\", key),")
        print("        };")
        print("        code")
        print("    }")
    else:
        passing = True
        for i in range(START_ASCII, END_ASCII_EXCLUSIVE):
            checkindex = i - START_ASCII
            if checkval[checkindex] != i:
                print("Expected {} but got 0x{:x}".format(chr(i), checkval[checkindex]))
                passing = False
        if not passing:
            print("Test did not pass")
        else:
            print("Test passed")

if __name__ == "__main__":
    main()
    exit(0)
