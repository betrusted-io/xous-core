use std::fmt::Write;
use std::string::String;

use locales::t;
use qrcode::{Color, QrCode};
use xous_ipc::Buffer;

use crate::minigfx::op::{HEIGHT, WIDTH};
use crate::minigfx::*;
use crate::service::api::Gid;
use crate::widgets::*;

/// Threshold of suggestable words to go from OSK to pick list for suggestions
const SUGGEST_THRESH: usize = 9;

/// UX modes:
///
/// WordList mode:
///   - List of words entered so far
///   - [ Pick word ] option
///   - [ Scan QR code ] option
///   - If the words list is not yet valid BIP-39:
///      - [ Cancel ] option appears at teh bottom of the list
///   - If the words match a valid BIP-39:
///     - [ Cancel ] is removed
///     - [ Scan QR code ] is removed; [ Show QR code ] is added
///     - [ Done ] option appears at the bottom of the list
///   - If an existing word is picked, go into WordEntry mode
///
/// WordEntry mode:
///   - Onscreen keyboard appears
///   - Top line is characters entered so far
///   - If the potential list of matching words is <6 words
///      - OSK disappears and you can pick one of the remaining words
///      - A final entry of "⬅" is available in the list to rub out the last entered character
///   - Special characters: when hovered, the word in entry is hidden, and replaced with a phrase that
///     describes one of these special operations:
///       - If "↩" is selected, the operation is canceled
///       - If "✖" is selected, the word is deleted from the list
///       - If "➕" is selected, the word is inserted above the currently selected word in the list
///
/// QR Display mode:
///   - The screen is filled with a QR code representation of the BIP-39 - represented in its raw binary form,
///     not as a list of words.
///   - Any key exits the mode and returns you to WordList mode
enum Bip39Mode {
    WordList,
    OskEntry,
    ShortList,
    QrDisplay,
}

pub struct Bip39Entry {
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub user_input: String,
    pub payload: Option<Vec<u8>>,
    pub suggested_words: Vec<String>,
    mode: Bip39Mode,
    insert_index: usize,
    items: ScrollableList,
    osk: ScrollableList,
    shortlist: ScrollableList,
    qrcode: Vec<bool>,
    qrwidth: usize,
    extra_items: usize,
    is_insert: bool,
}

impl Default for Bip39Entry {
    fn default() -> Self {
        let mut items = ScrollableList::default();
        items.set_margin(Point::new(4, 0));
        let br = items.pane().br();
        let row_height = items.row_height();
        items.pane_size(Rectangle::new(Point::new(0, row_height as isize + 2), br));

        items.add_item(0, t!("bip39.pick_word", locales::LANG));
        items.add_item(0, t!("bip39.scan_qr_code", locales::LANG));
        items.add_item(0, t!("bip39.cancel", locales::LANG));
        let extra_items = items.col_length(0).unwrap_or(0);

        let mut sl = ScrollableList::default();
        sl.set_margin(Point::new(10, 0));
        let br = sl.pane().br();
        let row_height = sl.row_height();
        sl.pane_size(Rectangle::new(Point::new(0, row_height as isize + 2), br));

        let mut osk = ScrollableList::default();
        osk.set_alignment(TextAlignment::Center);
        match locales::LANG {
            "en" => {
                /*
                    p b v k g m
                    e t a o i n
                    s h r d l u
                    c x w f j y
                    q z ⬅
                */
                let template = vec![
                    vec!["p", "e", "s", "c", "q"],
                    vec!["b", "t", "h", "x", "z"],
                    vec!["v", "a", "r", "w", "⬅"],
                    vec!["k", "o", "d", "f", "↩"],
                    vec!["g", "i", "l", "j", "➕"],
                    vec!["m", "n", "u", "y", "✖"],
                ];
                for (col_num, col) in template.iter().enumerate() {
                    for row in col {
                        osk.add_item(col_num, row);
                    }
                }
                // set the cursor on 't'
                osk.set_selected(1, 1).ok();
            }
            _ => unimplemented!("OSK matrix not yet implemented for language target"),
        }
        Self {
            action_conn: Default::default(),
            action_opcode: Default::default(),
            user_input: String::new(),
            payload: None,
            suggested_words: Vec::new(),
            mode: Bip39Mode::WordList,
            insert_index: 0,
            items,
            shortlist: sl,
            osk,
            qrcode: Vec::new(),
            qrwidth: 0,
            extra_items,
            is_insert: false,
        }
    }
}

impl Bip39Entry {
    pub fn new(_is_password: bool, action_conn: xous::CID, action_opcode: u32) -> Self {
        Self { action_conn, action_opcode, ..Default::default() }
    }

    /// TODO: reduce replication with draw_qrcode in "notification" module
    fn draw_qrcode(&self, at_height: isize, modal: &Modal) {
        // calculate pixel size of each module in the qrcode
        let qrcode_modules: isize = self.qrwidth.try_into().unwrap();
        let modules: isize = qrcode_modules + 2 * QUIET_MODULES;
        let canvas_width = modal.canvas_width - 2 * modal.margin;
        let mod_size_px: isize = canvas_width / modules;
        let qrcode_width_px = qrcode_modules * mod_size_px;
        let quiet_px: isize = (canvas_width - qrcode_width_px) / 2;

        // Iterate thru qrcode and stamp each square module like a typewriter
        let black = DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1);
        let top = at_height + quiet_px;

        // draw a white background over the whole screen
        modal
            .gfx
            .draw_rectangle(Rectangle::new_with_style(
                Point::new(0, at_height),
                Point::new(WIDTH, HEIGHT),
                DrawStyle::new(PixelColor::Light, PixelColor::Light, 1),
            ))
            .unwrap();

        let left = modal.margin + quiet_px;
        let right = left + qrcode_modules * mod_size_px;
        let mut module =
            Rectangle::new_with_style(Point::new(0, 0), Point::new(mod_size_px - 1, mod_size_px - 1), black);
        let step = Point::new(mod_size_px, 0);
        let cr_lf = Point::new(-qrcode_modules * mod_size_px, mod_size_px);
        let mut j: isize;
        let mut obj_list = ObjectList::new();
        module.translate(Point::new(right, top));
        for (i, stamp) in self.qrcode.iter().enumerate() {
            j = i.try_into().unwrap();
            if j % qrcode_modules == 0 {
                module.translate(cr_lf);
            }
            if *stamp {
                if obj_list.push(ClipObjectType::Rect(module.clone())).is_err() {
                    // the capacity of the list has exceeded the capacity of the buffer, send it and make a
                    // new one
                    modal.gfx.draw_object_list(obj_list).expect("couldn't draw qrcode module");
                    obj_list = ObjectList::new();
                    obj_list.push(ClipObjectType::Rect(module.clone())).unwrap();
                };
            }
            module.translate(step);
        }
        modal.gfx.draw_object_list(obj_list).expect("couldn't draw qrcode module");
    }
}

use crate::widgets::ActionApi;
impl ActionApi for Bip39Entry {
    fn set_action_opcode(&mut self, op: u32) { self.action_opcode = op }

    // take up all the space you got
    fn height(&self, _glyph_height: isize, _margin: isize, _modal: &Modal) -> isize {
        self.items.pane().br().y
    }

    fn key_action(&mut self, k: char) -> Option<ValidatorErr> {
        log::trace!("key_action: {}", k);
        match self.mode {
            Bip39Mode::WordList => {
                match k {
                    '←' | '→' | '↑' | '↓' => {
                        self.items.key_action(k);
                        None
                    }
                    '∴' | '\u{d}' => {
                        let selected = self.items.get_selected();
                        if selected == t!("bip39.pick_word", locales::LANG) {
                            self.insert_index = self.items.get_selected_index().1;
                            self.mode = Bip39Mode::OskEntry;
                            self.suggested_words.clear();
                            self.shortlist.clear();
                            self.user_input.clear();
                            self.is_insert = true;
                            None
                        } else if selected == t!("bip39.scan_qr_code", locales::LANG) {
                            unimplemented!();
                        } else if selected == t!("bip39.show_qr_code", locales::LANG) {
                            if let Some(bytes) = &self.payload {
                                let b64_bytes = base64::encode(bytes);
                                let qrcode = match QrCode::new(&b64_bytes) {
                                    Ok(code) => code,
                                    Err(e) => {
                                        log::error!("QR code couldn't encode data: {:?}", e);
                                        // don't redraw, just return to WordList mode
                                        self.mode = Bip39Mode::WordList;
                                        return None;
                                    }
                                };
                                self.qrwidth = qrcode.width();
                                log::info!(
                                    "qrcode {}x{} : {} bytes ",
                                    self.qrwidth,
                                    self.qrwidth,
                                    bytes.len()
                                );
                                self.qrcode.clear();
                                for color in qrcode.to_colors().iter() {
                                    match color {
                                        Color::Dark => self.qrcode.push(true),
                                        Color::Light => self.qrcode.push(false),
                                    }
                                }
                                self.mode = Bip39Mode::QrDisplay;
                            } else {
                                log::error!("Show QR code was selectable when no qr data is present!");
                                self.mode = Bip39Mode::WordList;
                            }
                            None
                        } else if selected == t!("bip39.done", locales::LANG) {
                            if let Some(data) = &self.payload {
                                let mut ret = Bip39EntryPayload::default();
                                ret.data[..data.len()].copy_from_slice(&data);
                                ret.len = data.len() as u32;

                                // relinquish focus before returning the result
                                self.items.gfx.release_modal().unwrap();
                                xous::yield_slice();

                                let buf = Buffer::into_buf(ret).expect("couldn't convert message to payload");
                                buf.send(self.action_conn, self.action_opcode)
                                    .map(|_| ())
                                    .expect("couldn't send action message");
                            } else {
                                panic!(
                                    "Illegal state: done should not be selectable with an invalid payload"
                                );
                            }
                            None
                        } else if selected == t!("bip39.cancel", locales::LANG) {
                            self.items.gfx.release_modal().unwrap();
                            xous::yield_slice();

                            let ret = Bip39EntryPayload::default(); // return a 0-length entry
                            let buf = Buffer::into_buf(ret).expect("couldn't convert message to payload");
                            buf.send(self.action_conn, self.action_opcode)
                                .map(|_| ())
                                .expect("couldn't send action message");
                            None
                        } else {
                            // we picked a word itself; allow for editing the word
                            self.insert_index = self.items.get_selected_index().1;
                            self.mode = Bip39Mode::OskEntry;
                            self.suggested_words.clear();
                            self.shortlist.clear();
                            // set to just the first four characters - the minimum for a unique word
                            self.user_input = selected.chars().take(4).collect();
                            None
                        }
                    }
                    _ => None,
                }
            }
            Bip39Mode::OskEntry => {
                match k {
                    '←' | '→' | '↑' | '↓' => {
                        self.osk.key_action(k);
                        None
                    }
                    '∴' | '\u{d}' => {
                        let selection = self.osk.get_selected();
                        match selection {
                            "⬅" => {
                                // remove the last character
                                if let Some(idx) = self.user_input.char_indices().rev().next().map(|(i, _)| i)
                                {
                                    self.user_input.truncate(idx);
                                }
                            }
                            "✖" => {
                                // delete the current list entry
                                self.items.delete_selected();
                                self.mode = Bip39Mode::WordList;
                            }
                            "➕" => {
                                self.user_input.clear();
                                self.suggested_words.clear();
                                self.shortlist.clear();
                                // reset to picking a word so it's clear the command "took"
                                self.osk.set_selected(1, 1).ok();
                                self.is_insert = true;
                            }
                            "↩" => {
                                self.mode = Bip39Mode::WordList;
                            }
                            _ => {
                                self.user_input.push_str(selection);
                                self.suggested_words = suggest_bip39(&self.user_input);
                                if self.suggested_words.len() <= SUGGEST_THRESH {
                                    self.shortlist.clear();
                                    for word in self.suggested_words.iter() {
                                        self.shortlist.add_item(0, &word);
                                    }
                                    self.shortlist.add_item(0, "⬅");
                                    self.mode = Bip39Mode::ShortList;
                                }
                            }
                        }
                        None
                    }
                    _ => None,
                }
            }
            Bip39Mode::ShortList => {
                match k {
                    '←' | '→' | '↑' | '↓' => {
                        self.shortlist.key_action(k);
                        None
                    }
                    '∴' | '\u{d}' => {
                        let selection = self.shortlist.get_selected();
                        match selection {
                            "⬅" => {
                                // remove the last character
                                if let Some(idx) = self.user_input.char_indices().rev().next().map(|(i, _)| i)
                                {
                                    self.user_input.truncate(idx);
                                }
                                self.mode = Bip39Mode::OskEntry;
                            }
                            _ => {
                                // picked a word
                                if self.is_insert {
                                    self.items.insert_item(0, self.insert_index, selection);
                                } else {
                                    self.items.update_selected(selection);
                                }
                                self.mode = Bip39Mode::WordList;

                                self.is_insert = false;
                                // see if the list is complete
                                if let Some(full_list) = self.items.get_column(0) {
                                    let word_list =
                                        &full_list[..full_list.len().saturating_sub(self.extra_items)];
                                    if let Ok(bytes) = bip39_to_bytes(&word_list.to_vec()) {
                                        self.payload = Some(bytes);
                                        self.items.replace_with(
                                            0,
                                            t!("bip39.cancel", locales::LANG),
                                            t!("bip39.done", locales::LANG),
                                        );
                                        self.items.replace_with(
                                            0,
                                            t!("bip39.scan_qr_code", locales::LANG),
                                            t!("bip39.show_qr_code", locales::LANG),
                                        );
                                    } else {
                                        self.payload = None;
                                        self.items.replace_with(
                                            0,
                                            t!("bip39.done", locales::LANG),
                                            t!("bip39.cancel", locales::LANG),
                                        );
                                        self.items.replace_with(
                                            0,
                                            t!("bip39.show_qr_code", locales::LANG),
                                            t!("bip39.scan_qr_code", locales::LANG),
                                        );
                                    }
                                }
                            }
                        }
                        None
                    }
                    _ => None,
                }
            }
            Bip39Mode::QrDisplay => {
                // return to word list mode on any key press
                self.mode = Bip39Mode::WordList;
                None
            }
        }
    }

    fn redraw(&self, _at_height: isize, modal: &Modal) {
        match self.mode {
            Bip39Mode::WordList => {
                self.items.draw(0);
            }
            Bip39Mode::OskEntry => {
                let key = self.osk.get_selected();
                let top_text = match key {
                    "✖" => t!("bip39.delete_word", locales::LANG),
                    "➕" => t!("bip39.insert_word", locales::LANG),
                    "↩" => t!("bip39.cancel_entry", locales::LANG),
                    "⬅" => {
                        // continue to show the old input until the key is selected
                        &self.user_input
                    }
                    _ => &self.user_input,
                };
                // update the top work-in-progress bar
                let pane = self.items.pane();
                let mut tv = TextView::new(
                    Gid::dummy(),
                    TextBounds::BoundingBox(Rectangle::new(
                        Point::new(0, 0),
                        Point::new(pane.width() as isize, self.osk.row_height() as isize),
                    )),
                );
                tv.margin = Point::new(0, 0);
                tv.style = self.osk.get_style();
                tv.ellipsis = true;
                tv.invert = false;
                tv.draw_border = false;
                tv.write_str(top_text).ok();
                self.osk.gfx.draw_textview(&mut tv).ok();

                self.osk.draw(self.osk.row_height() as isize);
            }
            Bip39Mode::ShortList => {
                self.shortlist.draw(0);
            }
            Bip39Mode::QrDisplay => {
                self.draw_qrcode(0, modal);
            }
        }
    }
}

// Eventually more language support can be added here:
//
// In order to integrate this well, we need to re-do the language build
// system to be based off of af #cfg feature, so that we can pick up
// the feature in this crate and select the right word list.
//
// We don't compile all the word lists in because code size is precious.
//
// Each language should simply create its table assigning to be symbol
// `const BIP39_TABLE: [&'static str; 2048]`. This allows the rest of
// the code to refer to the table without change, all we do is swap out
// which language module is included in the two lines below.
pub mod en;
pub use en::*;
use sha2::Digest;

use super::ScrollableList;

#[derive(Debug, Eq, PartialEq)]
pub enum Bip39Error {
    InvalidLength,
    InvalidChecksum,
    InvalidWordAt(usize),
}

/// This routine takes an array of bytes and attempts to return an array of Bip39
/// words. If the bytes do not conform to a valid length, we return `Bip39Error::InvalidLength`.
/// A `Vec::<String>` is returned in case the caller wants to do stupid formatting tricks
/// on the words (saving them effort of parsing a single concatenated String).
pub fn bytes_to_bip39(bytes: &Vec<u8>) -> Result<Vec<String>, Bip39Error> {
    let mut result = Vec::<String>::new();
    match bytes.len() {
        16 | 20 | 24 | 28 | 32 => (),
        _ => return Err(Bip39Error::InvalidLength),
    }
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let checksum_bits = bytes.len() / 4;
    let checksum = digest.as_slice()[0] >> (8 - checksum_bits);

    let mut bits_in_bucket = 0;
    let mut bucket = 0u32;
    for &b in bytes {
        bucket <<= 8;
        bucket |= b as u32;
        bits_in_bucket += 8;
        if bits_in_bucket >= 11 {
            let codeword = bucket >> (bits_in_bucket - 11);
            bucket &= !((0b111_1111_1111u32) << (bits_in_bucket - 11));
            bits_in_bucket -= 11;
            result.push(BIP39_TABLE[codeword as usize].to_string());
        }
    }
    assert!(bits_in_bucket + checksum_bits == 11);
    bucket <<= checksum_bits;
    bucket |= checksum as u32;
    assert!(bucket < 2048);
    result.push(BIP39_TABLE[bucket as usize].to_string());
    Ok(result)
}

/// The caller must provide a list of words parsed into individual Bip39 words.
/// The words are case-insensitive. However, if any word is invalid, the routine
/// will return `InvalidWordAt(index of invalid word)` at the first invalid word
/// detected (it may not be the only invalid word).
pub fn bip39_to_bytes(bip39: &Vec<String>) -> Result<Vec<u8>, Bip39Error> {
    // this implementation favors small runtime memory allocation over performance
    // sifting through a list of 2048 words is reasonably fast, even if doing up to 24 times;
    // this is especially in comparison to the screen redraw times. We could also create
    // a HashSet or something or a tree to do the search faster but in this system, we fight
    // for even 4kiB of RAM savings at times.
    // The inefficiency is especially small in comparison to the ridiculous SHA256 computation
    // that has to happen to checksum the result.

    match bip39.len() {
        12 | 15 | 18 | 21 | 24 => (),
        _ => return Err(Bip39Error::InvalidLength),
    }

    let mut indices = Vec::<u32>::new();
    for (index, bip) in bip39.iter().enumerate() {
        if let Some(i) = BIP39_TABLE.iter().position(|&x| x == bip) {
            indices.push(i as u32);
        } else {
            return Err(Bip39Error::InvalidWordAt(index));
        }
    }

    // collate into u8 vec
    let mut data = Vec::<u8>::new();
    let mut bucket = 0u32;
    let mut bits_in_bucket = 0;
    for index in indices {
        // add bits to bucket
        bucket = (bucket << 11) | index;
        bits_in_bucket += 11;

        while bits_in_bucket >= 8 {
            // extract the top 8 bits from the bucket, put it into the result vector
            data.push((bucket >> (bits_in_bucket - 8)) as u8);
            // mask off the "used up" bits
            bucket &= !(0b1111_1111 << bits_in_bucket - 8);

            // subtract the used bits out of the bucket
            bits_in_bucket -= 8;
        }
    }
    // the bucket should now just contain the checksum
    let entered_checksum = if bits_in_bucket == 0 {
        // edge case of exactly enough checksum bits to fill a byte (happens in 256-bit case)
        data.pop().unwrap()
    } else {
        bucket as u8
    };

    let mut hasher = sha2::Sha256::new();
    hasher.update(&data);
    let digest = hasher.finalize();
    let checksum_bits = data.len() / 4;
    let checksum = digest.as_slice()[0] >> (8 - checksum_bits);
    if checksum == entered_checksum {
        Ok(data)
    } else {
        log::warn!("checksum didn't match: {:x} vs {:x}", checksum, entered_checksum);
        Err(Bip39Error::InvalidChecksum)
    }
}

pub const BIP39_SUGGEST_LIMIT: usize = 16;
/// This turns a string into a list of suggestions. If the String is empty, the
/// suggestion list is empty. The suggestion list is limited to BIP39_SUGGEST_LIMIT hints.
pub fn suggest_bip39(start: &str) -> Vec<String> {
    let mut ret = Vec::<String>::new();
    // first see if any prefixes match; stop when we find enough
    for bip in BIP39_TABLE {
        if bip.starts_with(start) {
            ret.push(bip.to_string());
            if ret.len() >= BIP39_SUGGEST_LIMIT {
                break;
            }
        }
    }
    if ret.len() > 0 {
        return ret;
    }
    // no prefixes match, suggest substrings
    for bip in BIP39_TABLE {
        if bip.contains(start) {
            ret.push(bip.to_string());
            if ret.len() >= BIP39_SUGGEST_LIMIT {
                break;
            }
        }
    }
    ret
}

/// This routine returns `true` if the given word is a valid BIP39 word.
#[allow(dead_code)]
pub fn is_valid_bip39(word: &str) -> bool {
    let lword = word.to_ascii_lowercase();
    for w in BIP39_TABLE {
        if lword == w {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    /// PAGE_SIZE is required to be a power of two. 0x1000 -> 0x1000 - 1 = 0xFFF, which forms the bitmasks.
    fn test_11_to_8() {
        let indices = [
            0b00000110001,
            0b10110011110,
            0b01110010100,
            0b00110110010,
            0b10001011010,
            0b11100111111,
            0b01101010011,
            0b10000011000,
            0b01101011001,
            0b10110011111,
            0b10001001110,
            0b00111100110,
        ];
        let refnum = 0b00000110001101100111100111001010000110110010100010110101110011111101101010011100000110000110101100110110011111100010011100011110u128;

        // 00000110001 10110011110 01110010100 00110110010 10001011010 11100111111 01101010011 10000011000
        // 01101011001 10110011111 10001001110 00111100110 0000_0110|001/1_0110|0111_10/
        // 01|1100_1010_0/001_10110010 10001011010 11100111111 01101010011 10000011000 01101011001 10110011111
        // 10001001110 00111100110 0000_0110 0011_0110  0111_1001 1100_1010 ...
        // 0001101100101000101101011100111111011010100111000001100001101011001101100111111000100111000111100110

        let mut refvec = refnum.to_be_bytes().to_vec();
        refvec.push(6); // checksum

        let mut data = Vec::<u8>::new();
        let mut bucket = 0u32;
        let mut bits_in_bucket = 0;
        for index in indices {
            // add bits to bucket
            bucket = (bucket << 11) | index;
            bits_in_bucket += 11;

            while bits_in_bucket >= 8 {
                // extract the top 8 bits from the bucket, put it into the result vector
                data.push((bucket >> (bits_in_bucket - 8)) as u8);
                // mask off the "used up" bits
                bucket &= !(0b1111_1111 << bits_in_bucket - 8);

                // subtract the used bits out of the bucket
                bits_in_bucket -= 8;
            }
        }
        if bits_in_bucket != 0 {
            data.push(bucket as u8);
        }
        assert!(data.len() == refvec.len());
        for (index, (&a, &b)) in refvec.iter().zip(data.iter()).enumerate() {
            if a != b {
                println!("index {} error: a[{}{:x})] != b[{}({:x})]", index, a, a, b, b);
            } else {
                println!("index {} match: a[{}({:x})] == b[{}({:x})]", index, a, a, b, b);
            }
            assert!(a == b);
        }
    }
    #[test]
    fn test_bip39_to_bytes() {
        let phrase = vec![
            "alert".to_string(),
            "record".to_string(),
            "income".to_string(),
            "curve".to_string(),
            "mercy".to_string(),
            "tree".to_string(),
            "heavy".to_string(),
            "loan".to_string(),
            "hen".to_string(),
            "recycle".to_string(),
            "mean".to_string(),
            "devote".to_string(),
        ];
        let refnum = 0b00000110001101100111100111001010000110110010100010110101110011111101101010011100000110000110101100110110011111100010011100011110u128;
        let mut refvec = refnum.to_be_bytes().to_vec();
        // refvec.push(6); // checksum

        assert_eq!(Ok(refvec), bip39_to_bytes(&phrase));
    }
    #[test]
    fn test_is_valid_bip39() {
        assert_eq!(is_valid_bip39("alert"), true);
        assert_eq!(is_valid_bip39("rEcOrD"), true);
        assert_eq!(is_valid_bip39("foobar"), false);
        assert_eq!(is_valid_bip39(""), false);
    }
    #[test]
    fn test_suggest_prefix() {
        let suggestions = suggest_bip39("ag");
        let reference =
            vec!["again".to_string(), "age".to_string(), "agent".to_string(), "agree".to_string()];
        assert_eq!(suggestions, reference);
    }
    #[test]
    fn test_bytes_to_bip39() {
        let refnum = 0b00000110001101100111100111001010000110110010100010110101110011111101101010011100000110000110101100110110011111100010011100011110u128;
        let refvec = refnum.to_be_bytes().to_vec();
        let phrase = vec![
            "alert".to_string(),
            "record".to_string(),
            "income".to_string(),
            "curve".to_string(),
            "mercy".to_string(),
            "tree".to_string(),
            "heavy".to_string(),
            "loan".to_string(),
            "hen".to_string(),
            "recycle".to_string(),
            "mean".to_string(),
            "devote".to_string(),
        ];
        assert_eq!(bytes_to_bip39(&refvec), Ok(phrase));
    }
}
