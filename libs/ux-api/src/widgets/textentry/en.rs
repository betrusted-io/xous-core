/*
ASCII charset OSK.

This is the full ASCII character set:
  0 @ P ` p
! 1 A Q a q
" 2 B R b r
# 3 C S c s
$ 4 D T d t
% 5 E U e u
& 6 F V f v
' 7 G W g w
( 8 H X h x
) 9 I Y i y
* : J Z j z
+ ; K [ k {
, < L \ l |
- = M ] m }
. > N ^ n ~
/ ? O _ o DEL

Reformat them by top characters
used in English language, then
common punctuations on the Internet
(http://, base-64).

Then format with ChatGPT using this exact query:

Take this matrix of characters:

p b v k g m
e t a o i n
s h r d l u
c x w f j y
q z . ,   !
' " ? ( ) :
@ / & = + ⬅
1 2 3 4 5 6
7 8 9 0 ; -
P B V K G M
E T A O I N
S H R D L U
C X W F J Y
Q Z _ < > ~
* \ [ ] { }
| ^ $ # % `

and format them into a Rust structure defined as [[&'static str; 16]; 6],
suitable for use as a pub const in a header file.

*/

pub const OSK_MATRIX: [[&'static str; 16]; 6] = [
    ["p", "e", "s", "c", "q", "'", "@", "1", "7", "P", "E", "S", "C", "Q", "*", "|"],
    ["b", "t", "h", "x", "z", "\"", "/", "2", "8", "B", "T", "H", "X", "Z", "\\", "^"],
    ["v", "a", "r", "w", ".", "?", "&", "3", "9", "V", "A", "R", "W", "_", "[", "$"],
    ["k", "o", "d", "f", ",", "(", "=", "4", "0", "K", "O", "D", "F", "<", "]", "#"],
    ["g", "i", "l", "j", " ", ")", "+", "5", ";", "G", "I", "L", "J", ">", "{", "%"],
    ["m", "n", "u", "y", "!", ":", "⬅", "6", "-", "M", "N", "U", "Y", "~", "}", "`"],
];

/*
    1 2 3 4 5 6
    7 8 9 0 . -
    ⬅ b v k g m
    e t a o i n
    s h r d l u
    c p w f j y
    q z x ,   !
    ' " ? ( ) :
    @ / & = + ;
    P B V K G M
    E T A O I N
    S H R D L U
    C X W F J Y
    Q Z _ < > ~
    * \ [ ] { }
    | ^ $ # % `
*/
pub const OSK_NUM_MATRIX: [[&'static str; 16]; 6] = [
    ["1", "7", "⬅", "e", "s", "c", "q", "'", "@", "P", "E", "S", "C", "Q", "*", "|"],
    ["2", "8", "b", "t", "h", "p", "z", "\"", "/", "B", "T", "H", "X", "Z", "\\", "^"],
    ["3", "9", "v", "a", "r", "w", "x", "?", "&", "V", "A", "R", "W", "_", "[", "$"],
    ["4", "0", "k", "o", "d", "f", ",", "(", "=", "K", "O", "D", "F", "<", "]", "#"],
    ["5", ".", "g", "i", "l", "j", " ", ")", "+", "G", "I", "L", "J", ">", "{", "%"],
    ["6", "-", "m", "n", "u", "y", "!", ":", ";", "M", "N", "U", "Y", "~", "}", "`"],
];
