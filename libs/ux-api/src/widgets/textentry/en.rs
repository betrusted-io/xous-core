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

p b v k j x
e t a o i n
s h r d l c
u m w f g y
q z . ,   !
' " ? ( ) :
@ / & = + ⬅
1 2 3 4 5 6
7 8 9 0 ; -
P B V K J X
E T A O I N
S H R D L C
U M W F G Y
Q Z _ < > ~
* \ [ ] { }
| ^ $ # % `

and format them into a Rust structure defined as [[&'static str; 16]; 6],
suitable for use as a pub const in a header file.

*/

pub const OSK_MATRIX: [[&'static str; 16]; 6] = [
    ["p", "e", "s", "u", "q", "'", "@", "1", "7", "P", "E", "S", "U", "Q", "*", "|"],
    ["b", "t", "h", "m", "z", "\"", "/", "2", "8", "B", "T", "H", "M", "Z", "\\", "^"],
    ["v", "a", "r", "w", ".", "?", "&", "3", "9", "V", "A", "R", "W", "_", "[", "$"],
    ["k", "o", "d", "f", ",", "(", "=", "4", "0", "K", "O", "D", "F", "<", "]", "#"],
    ["j", "i", "l", "g", " ", ")", "+", "5", ";", "J", "I", "L", "G", ">", "{", "%"],
    ["x", "n", "c", "y", "!", ":", "⬅", "6", "-", "X", "N", "C", "Y", "~", "}", "`"],
];
