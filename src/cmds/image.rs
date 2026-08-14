// image.rs — REPL command that receives a bitmap over serial, one base64 chunk at a time.
//
// Wire format per chunk (70 bytes, before base64):
//   [0..2]  u16  chunk index (big-endian)
//   [2..66] u8*64 pixel data
//   [66..70] u32 CRC-32 of bytes [0..66] (big-endian)
//
// Replies:
//   "OK"      — chunk accepted, image not yet complete
//   "ERR"     — bad base64, wrong length, or CRC mismatch
//   "SUCCESS" — chunk accepted and all 32 chunks are now present
//
// Dependencies (Cargo.toml):
//   base64 = { version = "0.21", default-features = false, features = ["alloc"] }
//
// CRC-32 is implemented inline (no external crate needed).

use core::fmt::Write;
use std::io::Write as FsWrite;

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use dc34_api::{DC34_DICT, DC34_IMAGE};
use pddb::Pddb;

use crate::{CommonEnv, ShellCmdApi};

// -- Constants ----------------------------------------------------------------

const CHUNK_DATA_SIZE: usize = 64;
const CHUNK_INDEX_BYTES: usize = 2;
const CHUNK_CRC_BYTES: usize = 4;
/// Total decoded wire size per chunk.
const CHUNK_WIRE_SIZE: usize = CHUNK_INDEX_BYTES + CHUNK_DATA_SIZE + CHUNK_CRC_BYTES; // 70
/// 128×128 pixels / 8 bits / 64 bytes per chunk = 32 chunks.
const NUM_CHUNKS: usize = 32;
/// Final bitmap word count (128×128 / 32 bits = 512).
const BITMAP_WORDS: usize = 512;

// -- Command state -------------------------------------------------------------

pub struct Image {
    /// Raw 64-byte payloads indexed by chunk number.
    /// `None` means that chunk has not yet been received.
    chunks: Vec<Option<[u8; CHUNK_DATA_SIZE]>>,

    /// How many distinct chunk slots are currently filled.
    received_count: usize,

    pddb: Pddb,
}

impl Image {
    pub fn new() -> Self { Image { chunks: vec![None; NUM_CHUNKS], received_count: 0, pddb: Pddb::new() } }

    /// Return true when every chunk slot is filled.
    pub fn is_complete(&self) -> bool { self.received_count == NUM_CHUNKS }

    /// Assemble all received chunks into a `[u32; 512]` bitmap.
    /// Panics if `is_complete()` is false — caller should check first.
    pub fn to_bitmap(&self) -> [u32; BITMAP_WORDS] {
        assert!(self.is_complete(), "bitmap not yet complete");
        let mut bitmap = [0u32; BITMAP_WORDS];
        for (chunk_idx, slot) in self.chunks.iter().enumerate() {
            let data = slot.as_ref().unwrap();
            let word_base = chunk_idx * (CHUNK_DATA_SIZE / 4); // 16 words per chunk
            for w in 0..(CHUNK_DATA_SIZE / 4) {
                let o = w * 4;
                bitmap[word_base + w] = u32::from_be_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
            }
        }
        bitmap
    }

    /// Reset all state
    pub fn clear(&mut self) {
        for slot in self.chunks.iter_mut() {
            *slot = None;
        }
        self.received_count = 0;
    }
}

// -- ShellCmdApi implementation ------------------------------------------------

impl<'a> ShellCmdApi<'a> for Image {
    cmd_api!(image);

    fn process(&mut self, args: String, _env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        let mut ret = String::new();
        if args == "clear" {
            self.clear();
            self.pddb.delete_key(DC34_DICT, DC34_IMAGE, None).ok();
            // trigger a reload using monkey-patched opcode
            let conn = _env.xns.request_connection_blocking("_Vault2_").unwrap();
            xous::send_message(conn, xous::Message::new_scalar(1024, 0, 0, 0, 0)).ok();
            write!(ret, "CLEAR").unwrap();
            return Ok(Some(ret));
        }

        // -- Decode base64 argument --------------------------------------------
        let b64 = args.trim();
        if b64.is_empty() {
            write!(ret, "ERR").unwrap();
            return Ok(Some(ret));
        }

        let decoded = match B64.decode(b64) {
            Ok(d) => d,
            Err(_) => {
                write!(ret, "ERR").unwrap();
                return Ok(Some(ret));
            }
        };

        if decoded.len() != CHUNK_WIRE_SIZE {
            write!(ret, "ERR").unwrap();
            return Ok(Some(ret));
        }

        // -- Parse fields ------------------------------------------------------
        let index = u16::from_be_bytes([decoded[0], decoded[1]]) as usize;
        // data lives at decoded[2..66]
        let received_crc = u32::from_be_bytes([decoded[66], decoded[67], decoded[68], decoded[69]]);

        // -- Verify CRC over (index bytes || data) -----------------------------
        let computed_crc = crc32fast::hash(&decoded[..CHUNK_INDEX_BYTES + CHUNK_DATA_SIZE]);
        if computed_crc != received_crc {
            write!(ret, "ERR").unwrap();
            return Ok(Some(ret));
        }

        // -- Bounds-check index ------------------------------------------------
        if index >= NUM_CHUNKS {
            write!(ret, "ERR").unwrap();
            return Ok(Some(ret));
        }

        // -- Store chunk (silent overwrite on duplicate) -----------------------
        let mut data_arr = [0u8; CHUNK_DATA_SIZE];
        data_arr.copy_from_slice(&decoded[CHUNK_INDEX_BYTES..CHUNK_INDEX_BYTES + CHUNK_DATA_SIZE]);

        let was_empty = self.chunks[index].is_none();
        self.chunks[index] = Some(data_arr);
        if was_empty {
            self.received_count += 1;
        }

        // -- Reply -------------------------------------------------------------
        if self.is_complete() {
            {
                let mut image_key = self
                    .pddb
                    .get(DC34_DICT, DC34_IMAGE, None, true, true, Some(2048), None::<fn()>)
                    .expect("couldn't get PDDB key");
                let words = self.to_bitmap();
                let bytes: &[u8] = bytemuck::cast_slice(&words);
                image_key.write_all(bytes).ok();
            }
            self.clear();
            // trigger a reload using monkey-patched opcode
            let conn = _env.xns.request_connection_blocking("_Vault2_").unwrap();
            xous::send_message(conn, xous::Message::new_scalar(1024, 1, 0, 0, 0)).ok();
            write!(ret, "SUCCESS").unwrap();
        } else {
            write!(ret, "OK").unwrap();
        }

        Ok(Some(ret))
    }
}
