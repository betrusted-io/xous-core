use crate::println;

pub const SIGBLOCK_SIZE: usize = 0x1000;

const VERSION_STR: &'static str = "Xous OS Loader v0.9.3\n\r";
// v0.9.0 -- initial version
// v0.9.1 -- booting with hw acceleration, and "simplest signature" check on the entire xous.img blob
// v0.9.2 -- add version and length check between header and signed area
// v0.9.3 -- add lockout of key ROM in die() routine

pub const STACK_LEN: u32 = 8192 - (7 * 4); // 7 words for backup kernel args
pub const STACK_TOP: u32 = 0x4100_0000 - STACK_LEN;

use utralib::generated::*;

#[repr(C)]
struct SignatureInFlash {
    pub version: u32,
    pub signed_len: u32,
    pub signature: [u8; 64],
}

struct Point {
    x: i16,
    y: i16,
}
#[derive(PartialEq, Eq)]
enum Color {
    Light,
    Dark
}
const FB_WIDTH_WORDS: usize = 11;
const FB_WIDTH_PIXELS: usize = 336;
const FB_LINES: usize = 536;
const FB_SIZE: usize = FB_WIDTH_WORDS * FB_LINES; // 44 bytes by 536 lines
// this font is from the embedded graphics crate https://docs.rs/embedded-graphics/0.7.1/embedded_graphics/
const FONT_IMAGE: &'static [u8] = include_bytes!("font6x12_1bpp.raw");
const CHAR_HEIGHT: u32 = 12;
const CHAR_WIDTH: u32 = 6;
const FONT_IMAGE_WIDTH: u32 = 96;
const LEFT_MARGIN: i16 = 10;

struct Gfx {
    csr: utralib::CSR<u32>,
    fb: &'static mut [u32],
}
impl<'a> Gfx {
    pub fn init(&mut self, clk_mhz: u32) {
        self.csr.wfo(utra::memlcd::PRESCALER_PRESCALER, (clk_mhz / 2_000_000) - 1);
    }
    #[allow(dead_code)]
    pub fn update_all(&mut self) {
        self.csr.wfo(utra::memlcd::COMMAND_UPDATEALL, 1);
    }
    pub fn update_dirty(&mut self) {
        self.csr.wfo(utra::memlcd::COMMAND_UPDATEDIRTY, 1);
    }
    pub fn busy(&self) -> bool {
        if self.csr.rf(utra::memlcd::BUSY_BUSY) == 1 {
            true
        } else {
            false
        }
    }
    pub fn flush(&mut self) {
        self.update_dirty();
        while self.busy() {}
        // clear the dirty bits
        for lines in 0..FB_LINES {
            self.fb[lines * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] &= 0x0000_FFFF;
        }
    }
    pub fn set_devboot(&mut self) {
        self.csr.wfo(utra::memlcd::DEVBOOT_DEVBOOT, 1);
    }

    fn char_offset(&self, c: char) -> u32 {
        let fallback = ' ' as u32 - ' ' as u32;
        if c < ' ' {
            return fallback;
        }
        if c <= '~' {
            return c as u32 - ' ' as u32;
        }
        fallback
    }
    fn put_digit(&mut self, d: u8, pos: &mut Point) {
        let mut buf: [u8; 4] = [0; 4]; // stack buffer for the character encoding
        let nyb = d & 0xF;
        if nyb < 10 {
            self.msg(((nyb + 0x30) as char).encode_utf8(&mut buf), pos);
        } else {
            self.msg(((nyb + 0x61 - 10) as char).encode_utf8(&mut buf), pos);
        }
    }
    fn put_hex(&mut self, c: u8, pos: &mut Point) {
        self.put_digit(c >> 4, pos);
        self.put_digit(c & 0xF, pos);
    }
    pub fn hex_word(&mut self, word: u32, pos: &mut Point) {
        for &byte in word.to_be_bytes().iter() {
            self.put_hex(byte, pos);
        }
    }
    pub fn msg(&mut self, text: &'a str, pos: &mut Point) {
        // this routine is adapted from the embedded graphics crate https://docs.rs/embedded-graphics/0.7.1/embedded_graphics/
        let char_per_row = FONT_IMAGE_WIDTH / CHAR_WIDTH;
        let mut idx = 0;
        let mut x_update: i16 = 0;
        for current_char in text.chars() {
            let mut char_walk_x = 0;
            let mut char_walk_y = 0;

            loop {
                // Char _code_ offset from first char, most often a space
                // E.g. first char = ' ' (32), target char = '!' (33), offset = 33 - 32 = 1
                let char_offset = self.char_offset(current_char);
                let row = char_offset / char_per_row;

                // Top left corner of character, in pixels
                let char_x = (char_offset - (row * char_per_row)) * CHAR_WIDTH;
                let char_y = row * CHAR_HEIGHT;

                // Bit index
                // = X pixel offset for char
                // + Character row offset (row 0 = 0, row 1 = (192 * 8) = 1536)
                // + X offset for the pixel block that comprises this char
                // + Y offset for pixel block
                let bitmap_bit_index = char_x
                    + (FONT_IMAGE_WIDTH * char_y)
                    + char_walk_x
                    + (char_walk_y * FONT_IMAGE_WIDTH);

                let bitmap_byte = bitmap_bit_index / 8;
                let bitmap_bit = 7 - (bitmap_bit_index % 8);

                let color = if FONT_IMAGE[bitmap_byte as usize] & (1 << bitmap_bit) != 0 {
                    Color::Light
                } else {
                    Color::Dark
                };

                let x = pos.x
                    + (CHAR_WIDTH * idx as u32) as i16
                    + char_walk_x as i16;
                let y = pos.y + char_walk_y as i16;

                // draw color at x, y
                if (current_char as u8 != 0xd) && (current_char as u8 != 0xa) { // don't draw CRLF specials
                    self.draw_pixel(Point{x, y}, color);
                }

                char_walk_x += 1;

                if char_walk_x >= CHAR_WIDTH {
                    char_walk_x = 0;
                    char_walk_y += 1;

                    // Done with this char, move on to the next one
                    if char_walk_y >= CHAR_HEIGHT {
                        if current_char as u8 == 0xd { // '\n'
                            pos.y += CHAR_HEIGHT as i16;
                        } else if current_char as u8 == 0xa { // '\r'
                            pos.x = LEFT_MARGIN as i16;
                            x_update = 0;
                        } else {
                            idx += 1;
                            x_update += CHAR_WIDTH as i16;
                        }

                        break;
                    }
                }
            }
        }
        pos.x += x_update;
        self.flush();
    }
    pub fn draw_pixel(&mut self, pix: Point, color: Color) {
        let mut clip_y: usize = pix.y as usize;
        if clip_y >= FB_LINES {
            clip_y = FB_LINES - 1;
        }
        let clip_x: usize = pix.x as usize;
        if clip_x >= FB_WIDTH_PIXELS {
            clip_y = FB_WIDTH_PIXELS - 1;
        }
        if color == Color::Light {
            self.fb[(clip_x + clip_y * FB_WIDTH_WORDS * 32) / 32] |= 1 << (clip_x % 32)
        } else {
            self.fb[(clip_x + clip_y * FB_WIDTH_WORDS * 32) / 32] &= !(1 << (clip_x % 32))
        }
        // set the dirty bit on the line that contains the pixel
        self.fb[clip_y * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] |= 0x1_0000;
    }
}

struct Keyrom {
    csr: utralib::CSR<u32>,
}
#[derive(Copy, Clone)]
enum KeyLoc {
    SelfSignPub = 0x10,
    DevPub = 0x18,
    ThirdPartyPub = 0x20,
}
impl Keyrom {
    pub fn new() -> Self {
        Keyrom {
            csr: CSR::new(utra::keyrom::HW_KEYROM_BASE as *mut u32),
        }
    }
    fn key_is_zero(&mut self, key_base: KeyLoc) -> bool {
        for offset in key_base as u32..key_base as u32 + 8 {
            self.csr.wfo(utra::keyrom::ADDRESS_ADDRESS, offset as u32);
            if self.csr.rf(utra::keyrom::DATA_DATA) != 0 {
                return false;
            }
        }
        true
    }
    fn key_is_dev(&mut self, key_base: KeyLoc) -> bool {
        for offset in 0..8 {
            self.csr.wfo(utra::keyrom::ADDRESS_ADDRESS, offset as u32 + key_base as u32);
            let kval = self.csr.rf(utra::keyrom::DATA_DATA);
            self.csr.wfo(utra::keyrom::ADDRESS_ADDRESS, offset as u32 + KeyLoc::DevPub as u32);
            let dkval = self.csr.rf(utra::keyrom::DATA_DATA);
            if kval != dkval {
                return false;
            }
        }
        true
    }
    fn read_ed25519(&mut self, key_base: KeyLoc) -> Result<ed25519_dalek::PublicKey, &'static str> {
        let mut pk_bytes: [u8; 32] = [0; 32];
        for (offset, pk_word) in pk_bytes.chunks_exact_mut(4).enumerate() {
            self.csr.wfo(utra::keyrom::ADDRESS_ADDRESS, key_base as u32 + offset as u32);
            let word = self.csr.rf(utra::keyrom::DATA_DATA);
            for (&src_byte, dst_byte) in word.to_be_bytes().iter().zip(pk_word.iter_mut()) {
                *dst_byte = src_byte;
            }
        }
        ed25519_dalek::PublicKey::from_bytes(&pk_bytes).or(Err("invalid public key"))
    }
    /// locks all the keys from future read-out
    pub fn lock(&mut self) {
        for i in 0..256 {
            self.csr.wfo(utra::keyrom::LOCKADDR_LOCKADDR, i);
        }
    }
}

// returns true if the kernel is valid
// side-effects the "devboot" register in the gfx engine if devkeys were detected
pub fn validate_xous_img(xous_img_offset: *const u32) -> bool {
    // reset the SHA block, in case we're coming out of a warm reset
    let mut sha = CSR::new(utra::sha512::HW_SHA512_BASE as *mut u32);
    sha.wfo(utra::sha512::POWER_ON, 1);
    sha.wfo(utra::sha512::CONFIG_RESET, 1); // this reset takes ~32 CPU cycles before we can use the SHA block. not a problem.

    // conjure the signature struct directly out of memory. super unsafe.
    let sig_ptr = xous_img_offset as *const SignatureInFlash;
    let sig: &SignatureInFlash = unsafe{sig_ptr.as_ref().unwrap()};
    let mut cursor = Point {x: LEFT_MARGIN, y: (FB_LINES as i16 / 2) + 10}; // draw on bottom half

    // clear screen to all black
    let mut gfx = Gfx {
        csr: CSR::new(utra::memlcd::HW_MEMLCD_BASE as *mut u32),
        fb: unsafe{core::slice::from_raw_parts_mut(utralib::HW_MEMLCD_MEM as *mut u32, FB_SIZE)},
    };
    gfx.init(100_000_000);

    #[cfg(feature = "renode-bypass")]
    {
        let (_top, bottom) = gfx.fb.split_at_mut(gfx.fb.len() / 2);
        for (i, word) in bottom.iter_mut().enumerate() {
            *word = 0xff00_ff00;
        }
        gfx.flush();
        gfx.msg("RENODE BYPASS SELECTED\n\r", &mut cursor);
        gfx.msg("THIS IS A SECURITY VIOLATION\n\r", &mut cursor);
        gfx.msg("ALL SIGCHECKS SKIPPED\n\r", &mut cursor);
        let mut keyrom = Keyrom::new();
        keyrom.lock();
        gfx.set_devboot();
        gfx.flush();
        return true;
        // this will emit a warning -- we want that. this is not a natural or intended normal code exit!
    }
    // insert a pattern of alternating 0101/1010 to create a "gray effect" on the bottom half of the fb
    // note that the gray has "stripes" every 32 bits but it's visually easier to look at than stripes every other bit
    let (_top, bottom) = gfx.fb.split_at_mut(gfx.fb.len() / 2);
    for (i, word) in bottom.iter_mut().enumerate() {
        if i % 2 == 0 {
            *word = 0xAAAA_AAAA;
        } else {
            *word = 0x5555_5555;
        }
    }
    gfx.flush();

    // now characters should actually be able to print
    gfx.msg(VERSION_STR, &mut cursor);
    println!("{}", VERSION_STR);

    // init the curve25519 engine
    let mut engine = utralib::CSR::new(utra::engine::HW_ENGINE_BASE as *mut u32);
    engine.wfo(utra::engine::POWER_ON, 1);
    engine.wfo(utra::engine::WINDOW_WINDOW, 0);
    engine.wfo(utra::engine::MPSTART_MPSTART, 0);

    // select the public key
    let mut keyrom = Keyrom::new();
    let mut keyloc = KeyLoc::SelfSignPub; // start from the self-sign key first, then work your way to less secure options
    loop {
        match keyloc {
            KeyLoc::SelfSignPub => {
                if !keyrom.key_is_zero(KeyLoc::SelfSignPub) { // self-signing key takes priority
                    if keyrom.key_is_dev(KeyLoc::SelfSignPub) {
                        println!("Self-signed key slot, but with developer public key.");
                        // mainly to protect against devs who were debugging and just stuck a dev key in the secure slot, and forgot to remove it.
                        gfx.msg("DEVELOPER KEY DETECTED\n\r", &mut cursor);
                        gfx.set_devboot();
                    }
                } else {
                    keyloc = KeyLoc::ThirdPartyPub;
                    continue;
                }
            },
            KeyLoc::ThirdPartyPub => {
                // policy note: set the devboot flag also if we're doing a thirdparty pubkey boot
                // reasoning: the purpose of the hash mark is to indicate if someone could have tampered
                // with the device. Once an update is installed, it should always be self-signed, as it
                // protects against the third party pubkey from being compromised and an alternate firmware
                // being installed with no visible warning. Hence, even tho thirdparty pubkey boots could
                // be more trusted, let's still flag it.
                gfx.set_devboot();
                if !keyrom.key_is_zero(KeyLoc::ThirdPartyPub) { // third party key is second in line
                    if keyrom.key_is_dev(KeyLoc::ThirdPartyPub) {
                        println!("Third party public key slot, but with developer public key.");
                        gfx.msg("DEVELOPER KEY DETECTED\n\r", &mut cursor);
                    }
                } else {
                    keyloc = KeyLoc::DevPub;
                    continue;
                }
            },
            KeyLoc::DevPub => {
                if keyrom.key_is_zero(KeyLoc::DevPub) {
                    gfx.msg("Can't boot: No valid keys!", &mut cursor);
                    loop {}
                }
                gfx.msg("DEVELOPER KEY DETECTED\n\r", &mut cursor);
                println!("Using developer public key");
                gfx.set_devboot();
            }
        }
        let pubkey = keyrom.read_ed25519(keyloc).unwrap_or_else(|e| {
            gfx.msg("Unable to read key: ", &mut cursor);
            gfx.msg(e, &mut cursor);
            gfx.msg("\n\r", &mut cursor);
            panic!("Unable to read key: {}", e)
        });

        println!("Public key bytes: {:x?}", pubkey.as_bytes());

        let signed_len = sig.signed_len;
        let image: &[u8] = unsafe{core::slice::from_raw_parts(
            (xous_img_offset as usize + SIGBLOCK_SIZE) as *const u8,
            signed_len as usize)};

        // extract the version and length from the signed region
        use core::convert::TryInto;
        let protected_version = u32::from_le_bytes(image[signed_len as usize - 8 .. signed_len as usize - 4].try_into().unwrap());
        let protected_len = u32::from_le_bytes(image[signed_len as usize - 4 ..].try_into().unwrap());
        // check that the signed versions match the version reported in the header
        if sig.version != 1 || (sig.version != protected_version) {
            gfx.msg("Check fail: mismatch on signature record version numbering.\n\r", &mut cursor);
            println!("Check fail: mismatch on signature record version numbering.\n\r");
            println!("sig.version: {}", sig.version);
            println!("protected_version: {}", protected_version);
            // a little insight to help debug what went wrong.
            for words in image[signed_len as usize - 16 .. ].chunks(4) {
                let _word = u32::from_le_bytes(words.try_into().unwrap());
                println!("{:x}", _word);
            }
            die();
        }
        if protected_len != signed_len - 4 {
            gfx.msg("Check fail: mismatch on header length vs protected length.\n\r", &mut cursor);
            println!("Check fail: mismatch on header length vs protected length.\n\r");
            println!("signed_len - 4: {}", signed_len - 4);
            println!("protected_len: {}", protected_len);
            // a little insight to help debug what went wrong.
            for words in image[signed_len as usize - 16 .. ].chunks(4) {
                let _word = u32::from_le_bytes(words.try_into().unwrap());
                println!("{:x}", _word);
            }
            die();
        }

        let ed25519_signature = ed25519_dalek::Signature::from(sig.signature);
        use ed25519_dalek::Verifier;
        gfx.msg("Checking signature...\n\r", &mut cursor);
        if pubkey.verify(image, &ed25519_signature).is_ok() {
            gfx.msg("Signature check passed\n\r", &mut cursor);
            println!("Signature check passed");
            break;
        } else {
            // signature didn't work out, setup the next key and try it
            match keyloc {
                KeyLoc::SelfSignPub => {
                    keyloc = KeyLoc::ThirdPartyPub;
                    continue;
                }
                KeyLoc::ThirdPartyPub => {
                    // try another key and move on
                    keyloc = KeyLoc::DevPub;
                    continue;
                }
                KeyLoc::DevPub => {
                    // we're out of keys, display a message and try to power down
                    gfx.msg("Signature check failed, powering down\n\r", &mut cursor);
                    println!("Signature check failed");
                    die();
                }
            }
        }
    }

    // check the stack usage
    let stack: &[u32] = unsafe{core::slice::from_raw_parts(STACK_TOP as *const u32, (STACK_LEN as usize / core::mem::size_of::<u32>()) as usize)};
    let mut unused_stack_words = 0;
    for &word in stack.iter() {
        if word != 0xACE0BACE {
            break;
        }
        unused_stack_words += 1;
    }
    println!("Free stack after signature check: {} bytes", unused_stack_words * 4);
    gfx.msg("Free stack after sigcheck: 0x", &mut cursor);
    gfx.hex_word(unused_stack_words * 4, &mut cursor);

    gfx.msg("\n\r\n\rLoading kernel...\n\r", &mut cursor);
    println!("Everything checks out, loading to kernel...");

    let mut sha_csr = CSR::new(utra::sha512::HW_SHA512_BASE as *mut u32);
    sha_csr.wfo(utra::sha512::POWER_ON, 0); // cut power to the SHA block; this is the expected default state after the bootloader is done.
    let mut engine_csr = CSR::new(utra::engine::HW_ENGINE_BASE as *mut u32);
    engine_csr.wfo(utra::engine::POWER_ON, 0); // cut power to the engine block; this is the expected default state after the bootloader is done.
    // note that removing power does *not* clear the RF or microcode state -- data can leak from the bootloader
    // into other areas because of this! (but I think it's OK because we just mess around with public keys here)

    true
}

fn die() {
    let ticktimer = CSR::new(utra::ticktimer::HW_TICKTIMER_BASE as *mut u32);
    let mut power = CSR::new(utra::power::HW_POWER_BASE as *mut u32);
    let mut com = CSR::new(utra::com::HW_COM_BASE as *mut u32);
    let mut start = ticktimer.rf(utra::ticktimer::TIME0_TIME);
    let mut keyrom = Keyrom::new();
    keyrom.lock();
    loop {
        // every 15 seconds, attempt to send a power down command
        // any attempt to re-flash the system must halt the CPU before we time-out to this point!
        if ticktimer.rf(utra::ticktimer::TIME0_TIME) - start > 15_000 {
            println!("Powering down, keyrom locked...");
            power.rmwf(utra::power::POWER_STATE, 0);
            power.rmwf(utra::power::POWER_SELF, 0);

            // ship mode is the safest mode -- suitable for long-term storage (~years)
            com.wfo(utra::com::TX_TX, com_rs::ComState::POWER_SHIPMODE.verb as u32);
            while com.rf(utra::com::STATUS_TIP) == 1 {}
            let _ = com.rf(utra::com::RX_RX); // discard the RX result
            start = ticktimer.rf(utra::ticktimer::TIME0_TIME);
            keyrom.lock();
        }
    }
}