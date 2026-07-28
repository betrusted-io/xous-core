//! The Mail app's model + UI flows.
//!
//! The IMAP/SMTP wire protocol lives in `libs/mail` (the same client the
//! edlin app used). The MIME / RFC-2047 / quoted-printable parsing and the
//! connect-list-read-send orchestration are carried over from edlin's
//! `cmds.rs`, but the command-line surface is replaced with graphical GAM
//! screens. The app runs its own thin GAM shell (own `UxRegistration` +
//! content canvas + F-key legend via `crate::icontray`, like apps/vault) --
//! it does NOT use the `chat` library -- and drives every screen with the
//! `modals` service:
//!
//!   * F1 (`inbox`)    — list recent subjects + senders, pick one to read.
//!   * F2 (`compose`)  — a To/Subject/Body form, then SMTP send.
//!   * F3 (`settings`) — IMAP/SMTP server, user and password forms, saved
//!                       to the pddb.
//!   * F4 (`reply`)    — pre-filled reply to the open message.
//!
//! Account settings are persisted the same way edlin persisted its "mail"
//! file: as a pddb-backed key (encrypted at rest on real hardware) holding
//! "key=value" lines. See [`MailApp::save_config`] / [`MailApp::load_config`].

use core::fmt::Write as _; // for write!() into a TextView; aliased so it doesn't clash with std::io::Write
use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use gam::{DrawStyle, Gid, GlyphStyle, PixelColor, Point, Rectangle, TextBounds, TextView, UxRegistration};
use mail::{ImapChunk, ImapClient, SmtpClient};
use modals::Modals;
use num_traits::ToPrimitive; // for MailOp::*.to_u32() in the UxRegistration

use crate::api::MailOp;

/// pddb dict for this app (account settings live here).
pub const MAIL_DICT: &str = "mail";
/// pddb key (within [`MAIL_DICT`]) holding the "key=value" account config.
const MAIL_CONFIG_KEY: &str = "config";

/// Default implicit-TLS ports (IMAPS / SMTPS). See `ImapClient::connect` /
/// `SmtpClient::connect` in libs/mail.
const DEFAULT_IMAP_PORT: u16 = 993;
const DEFAULT_SMTP_PORT: u16 = 465;

/// How many messages the F1 inbox lists per page.
const PAGE_SIZE: usize = 10;

/// Geometry for sizing the message-reader pager so a page never overflows
/// the modal (overflow clips the text and pushes the nav buttons
/// off-screen). The modal content area is at most `MODAL_Y_MAX_PX` tall
/// (matches gam::api::MODAL_Y_MAX) on a screen `MODAL_WIDTH_PX` wide. The
/// glyph *height* is queried at runtime (SYSTEM_STYLE — currently Large,
/// 24px), so page sizing tracks the configured font size; width is derived
/// from that height since the fonts are proportional and GAM exposes no
/// width hint.
const MODAL_Y_MAX_PX: usize = 350;
/// Usable text width inside the modal, in pixels. Tuned down from an initial
/// 320: at 320 a full content line overran the real text area by ~2-3
/// characters, so the modal re-wrapped the tail onto a second line and
/// wasted vertical space. 285 drops the wrap width by ~4 chars at the Large
/// font (page_cols 35 -> 31), clearing the overrun plus a small margin. This
/// is the effective wrap width, so keep it a little under the true content
/// width to leave that margin.
const MODAL_WIDTH_PX: usize = 285;
/// Lines held back from the *content* budget (`page_lines`): the "page i/N"
/// title plus enough slack that a page's real text -- even when the modal's
/// proportional font wraps a few lines wider than our character estimate --
/// still fits under the modal's clamp height (see `paginate` / `PAD_EXTRA`).
///
/// The modal clamps *text* to `MODAL_Y_MAX - 2*line` (~12 lines at the Large
/// font), and the reader is a plain notification with no nav buttons, so we
/// only really need to reserve the title line + a small wrap margin. This
/// was 7 (a leftover from the earlier button-based pager) then 5; 3 fills
/// nearly the whole clamp with text. This is close to the limit -- if the
/// bottom line ever clips (e.g. on a page with several wrapping lines),
/// raise it back toward 4-5.
const MODAL_RESERVED_LINES: usize = 5;

/// Extra blank lines appended to every page (beyond the modal's max height)
/// so the modal *always* renders at its clamped maximum height. The modal
/// auto-sizes to content and doesn't clear the screen when it shrinks, so
/// forcing a constant (max) height is what stops a shorter page leaving the
/// previous page's residue behind. See `paginate`.
const PAD_EXTRA: usize = 4;

// =======================================================================
// Mail parsing helpers (no self needed).
//
// Carried over verbatim from edlin's cmds.rs, except `extract_subject` is
// generalized into `extract_header` so the inbox listing can pull both
// Subject and From out of a HEADER.FIELDS fetch. See the individual doc
// comments (kept from edlin) for the reasoning behind each.
// =======================================================================

/// Parses the message count out of a SELECT response's "* <n> EXISTS"
/// untagged line.
fn parse_exists(select_response: &[String]) -> Option<u32> {
    select_response.iter().find_map(|line| {
        let mut tokens = line.split_whitespace();
        if tokens.next()? != "*" {
            return None;
        }
        let n: u32 = tokens.next()?.parse().ok()?;
        if tokens.next()?.eq_ignore_ascii_case("EXISTS") { Some(n) } else { None }
    })
}

/// Parses the sequence number out of a FETCH response's leading
/// "* <n> FETCH ..." text.
fn parse_seq_num(text: &str) -> Option<u32> {
    let mut tokens = text.split_whitespace();
    if tokens.next()? != "*" {
        return None;
    }
    tokens.next()?.parse::<u32>().ok()
}

/// Case-insensitive (ASCII-only) substring search returning a byte offset
/// safe to slice the original `&str` at.
fn find_ascii_ci(haystack: &str, needle: &str) -> Option<usize> {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    for i in 0..=(h.len() - n.len()) {
        if h[i..i + n.len()].eq_ignore_ascii_case(n) {
            return Some(i);
        }
    }
    None
}

/// Pulls a header value out of a header-fields fetch response by unanchored
/// search (the IMAP literal concatenates onto the response syntax with no
/// leading newline, so the header name is rarely line-anchored here —
/// unlike a real RFC 5322 block, where `header_value` is correct). Decodes
/// RFC 2047 encoded-words. `field_lower` must include the trailing colon,
/// e.g. "subject:" or "from:".
fn extract_header(header_text: &str, field_lower: &str) -> Option<String> {
    let pos = find_ascii_ci(header_text, field_lower)?;
    let after = &header_text[pos + field_lower.len()..];
    let lines: Vec<&str> = after.lines().collect();
    if lines.is_empty() {
        return None;
    }
    let mut value = lines[0].trim().to_string();
    let mut j = 1;
    while j < lines.len() && (lines[j].starts_with(' ') || lines[j].starts_with('\t')) {
        value.push(' ');
        value.push_str(lines[j].trim());
        j += 1;
    }
    if value.is_empty() { None } else { Some(decode_rfc2047(&value)) }
}

/// Flattens a FETCH response's chunks into one lossy-UTF8 string.
fn flatten_chunks(chunks: &[ImapChunk]) -> String {
    chunks.iter().map(|c| String::from_utf8_lossy(c.as_bytes()).into_owned()).collect()
}

/// Splits raw message/part text into (header block, body) at the first
/// blank line.
fn split_headers_body(raw_text: &str) -> (&str, &str) {
    if let Some(pos) = raw_text.find("\r\n\r\n") {
        (&raw_text[..pos], &raw_text[pos + 4..])
    } else if let Some(pos) = raw_text.find("\n\n") {
        (&raw_text[..pos], &raw_text[pos + 2..])
    } else {
        (raw_text, "")
    }
}

/// Returns a header's value by parsing the header block into fields
/// (line-anchored, folding-aware). Correct for a real RFC 5322 header
/// block; see edlin's original note on why the anchoring is required
/// (DKIM-Signature's `h=` tag otherwise trips an unanchored search).
fn header_value(header_block: &str, name: &str) -> Option<String> {
    let lower_name = name.to_lowercase();
    let lines: Vec<&str> = header_block.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with(' ') || line.starts_with('\t') {
            i += 1; // orphan continuation line -- not a field start, skip
            continue;
        }
        match line.find(':') {
            Some(colon) => {
                let field_name = line[..colon].trim();
                let mut j = i + 1;
                while j < lines.len() && (lines[j].starts_with(' ') || lines[j].starts_with('\t')) {
                    j += 1;
                }
                if field_name.eq_ignore_ascii_case(&lower_name) {
                    let mut value = line[colon + 1..].trim().to_string();
                    for cont in &lines[i + 1..j] {
                        value.push(' ');
                        value.push_str(cont.trim());
                    }
                    return Some(value);
                }
                i = j;
            }
            None => i += 1,
        }
    }
    None
}

/// Extracts the "boundary" parameter from a Content-Type header value.
fn parse_boundary(content_type_value: &str) -> Option<String> {
    let pos = find_ascii_ci(content_type_value, "boundary=")?;
    let after = content_type_value[pos + "boundary=".len()..].trim_start();
    if let Some(rest) = after.strip_prefix('"') {
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    } else {
        let end = after.find(|c: char| c == ';' || c.is_whitespace()).unwrap_or(after.len());
        let value = after[..end].trim();
        if value.is_empty() { None } else { Some(value.to_string()) }
    }
}

/// Splits a multipart body into each part's raw (headers+body) text, given
/// the enclosing boundary. Preamble/epilogue are discarded.
fn split_multipart(body: &str, boundary: &str) -> Vec<String> {
    let delim = format!("--{boundary}");
    let mut parts = Vec::new();
    let mut remaining = match body.find(&delim) {
        Some(pos) => &body[pos..],
        None => return parts,
    };
    loop {
        remaining = &remaining[delim.len()..];
        if remaining.starts_with("--") {
            break; // closing delimiter "--boundary--"
        }
        remaining = remaining.strip_prefix("\r\n").or_else(|| remaining.strip_prefix('\n')).unwrap_or(remaining);
        match remaining.find(&delim) {
            Some(next_pos) => {
                parts.push(remaining[..next_pos].to_string());
                remaining = &remaining[next_pos..];
            }
            None => {
                parts.push(remaining.to_string());
                break;
            }
        }
    }
    parts
}

/// Walks a (possibly nested) multipart structure for a readable text/plain
/// part, preferring it over text/html. Returns the input unchanged when not
/// multipart (the common case). `depth` bounds the recursion.
fn find_text_part(header_block: &str, body: &str, depth: u8) -> (String, String) {
    let content_type = header_value(header_block, "content-type").unwrap_or_default();
    if depth == 0 || !content_type.to_lowercase().starts_with("multipart/") {
        return (header_block.to_string(), body.to_string());
    }
    let boundary = match parse_boundary(&content_type) {
        Some(b) => b,
        None => return (header_block.to_string(), body.to_string()),
    };
    let raw_parts = split_multipart(body, &boundary);
    if raw_parts.is_empty() {
        return (header_block.to_string(), body.to_string());
    }

    let mut fallback: Option<(String, String)> = None;
    for raw_part in &raw_parts {
        let (part_headers, part_body) = split_headers_body(raw_part);
        let part_content_type = header_value(part_headers, "content-type").unwrap_or_default().to_lowercase();

        let (resolved_headers, resolved_body) = if part_content_type.starts_with("multipart/") {
            find_text_part(part_headers, part_body, depth - 1)
        } else {
            (part_headers.to_string(), part_body.to_string())
        };

        let resolved_content_type =
            header_value(&resolved_headers, "content-type").unwrap_or_default().to_lowercase();
        if resolved_content_type.starts_with("text/plain") || resolved_content_type.is_empty() {
            return (resolved_headers, resolved_body);
        }
        if fallback.is_none() {
            fallback = Some((resolved_headers, resolved_body));
        }
    }
    fallback.unwrap_or_else(|| (header_block.to_string(), body.to_string()))
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Decodes RFC 2045 quoted-printable content ("=XX" hex escapes, trailing
/// "=" soft line breaks). Best-effort on malformed escapes.
fn decode_quoted_printable(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' {
            if bytes.get(i + 1) == Some(&b'\r') && bytes.get(i + 2) == Some(&b'\n') {
                i += 3;
                continue;
            }
            if bytes.get(i + 1) == Some(&b'\n') {
                i += 2;
                continue;
            }
            if let (Some(&h1), Some(&h2)) = (bytes.get(i + 1), bytes.get(i + 2)) {
                if let (Some(hi), Some(lo)) = (hex_digit(h1), hex_digit(h2)) {
                    out.push((hi << 4) | lo);
                    i += 3;
                    continue;
                }
            }
            out.push(b'=');
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Decodes RFC 2047 "Q" encoding (inside `=?charset?Q?...?=`): "=XX" hex
/// escapes plus "_" -> space.
fn decode_rfc2047_q(input: &str) -> Vec<u8> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'_' => {
                out.push(b' ');
                i += 1;
            }
            b'=' => {
                if let (Some(&h1), Some(&h2)) = (bytes.get(i + 1), bytes.get(i + 2)) {
                    if let (Some(hi), Some(lo)) = (hex_digit(h1), hex_digit(h2)) {
                        out.push((hi << 4) | lo);
                        i += 3;
                        continue;
                    }
                }
                out.push(b'=');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

/// Interprets decoded encoded-word bytes per the declared charset. Exact
/// for UTF-8 / ISO-8859-1 / US-ASCII; lossy-UTF8 fallback otherwise.
fn bytes_to_string_for_charset(bytes: &[u8], charset: &str) -> String {
    let lower = charset.to_lowercase();
    if lower == "us-ascii" || lower == "iso-8859-1" || lower == "iso8859-1" || lower == "latin1" {
        bytes.iter().map(|&b| b as char).collect()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Tries to decode one RFC 2047 encoded-word at the start of `s`. Returns
/// (decoded text, byte length consumed) or None.
fn try_decode_encoded_word(s: &str) -> Option<(String, usize)> {
    if !s.starts_with("=?") {
        return None;
    }
    let rest = &s[2..];
    let charset_end = rest.find('?')?;
    let charset = &rest[..charset_end];
    let after_charset = &rest[charset_end + 1..];
    if after_charset.as_bytes().get(1) != Some(&b'?') {
        return None;
    }
    let encoding = *after_charset.as_bytes().first()?;
    let after_encoding = &after_charset[2..];
    let text_end = after_encoding.find("?=")?;
    let encoded_text = &after_encoding[..text_end];

    let decoded_bytes: Vec<u8> = match encoding.to_ascii_uppercase() {
        b'Q' => decode_rfc2047_q(encoded_text),
        b'B' => B64.decode(encoded_text.as_bytes()).ok()?,
        _ => return None,
    };
    let decoded_string = bytes_to_string_for_charset(&decoded_bytes, charset);
    let total_len = 2 + charset_end + 1 + 1 + 1 + text_end + 2;
    Some((decoded_string, total_len))
}

/// Decodes every RFC 2047 encoded-word in a header value; whitespace
/// between adjacent encoded-words is removed, plain text passes through.
fn decode_rfc2047(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.char_indices().peekable();
    let mut last_was_encoded_word = false;

    while let Some((idx, ch)) = chars.next() {
        if ch == '=' && input[idx..].starts_with("=?") {
            if let Some((decoded, token_len)) = try_decode_encoded_word(&input[idx..]) {
                out.push_str(&decoded);
                let end = idx + token_len;
                while let Some(&(next_idx, _)) = chars.peek() {
                    if next_idx >= end {
                        break;
                    }
                    chars.next();
                }
                last_was_encoded_word = true;
                continue;
            }
        }

        if ch.is_whitespace() && last_was_encoded_word {
            let ws_start = idx;
            let mut ws_end = idx + ch.len_utf8();
            while let Some(&(next_idx, next_ch)) = chars.peek() {
                if next_ch.is_whitespace() {
                    ws_end = next_idx + next_ch.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            if let Some((decoded, token_len)) = try_decode_encoded_word(&input[ws_end..]) {
                out.push_str(&decoded);
                let end = ws_end + token_len;
                while let Some(&(next_idx, _)) = chars.peek() {
                    if next_idx >= end {
                        break;
                    }
                    chars.next();
                }
                last_was_encoded_word = true;
                continue;
            }
            out.push_str(&input[ws_start..ws_end]);
            last_was_encoded_word = false;
            continue;
        }

        out.push(ch);
        last_was_encoded_word = false;
    }
    out
}

// =======================================================================
// The app model
// =======================================================================

/// One row in the F1 inbox listing.
struct InboxEntry {
    /// Recency index (1 = most recent); used to re-fetch on selection.
    recency: usize,
    from: String,
    subject: String,
}

/// The message currently displayed in the view, remembered so F4 can
/// pre-fill a reply from it.
struct OpenMessage {
    from: String,
    subject: String,
    body: String,
}

pub struct MailApp {
    modals: Modals,

    // Mail (IMAP/SMTP) account settings. Loaded from the pddb on startup
    // (see load_config) and editable under F3 (see settings). Blank
    // credentials fail loudly at connect time rather than silently talking
    // to nothing. Note: like edlin, these live decrypted in this process's
    // RAM for its lifetime -- fine for a personal device, not for one you
    // hand to someone else with mail configured.
    imap_user: String,
    imap_pass: String,
    imap_host: String,
    imap_port: u16,

    smtp_user: String,
    smtp_pass: String,
    /// Envelope/header "From" address. Some providers require this to match
    /// smtp_user (or an alias) or they'll reject the send.
    smtp_from: String,
    smtp_host: String,
    smtp_port: u16,

    /// Last inbox listing, so a picked label maps back to a recency index.
    inbox: Vec<InboxEntry>,

    /// The message most recently opened under F1, so F4 can reply to it.
    open_msg: Option<OpenMessage>,

    /// "host:port" endpoints whose TLS chain we've already probed and
    /// trusted (or prompted for) this session, so we don't re-probe on
    /// every connection.
    trusted: HashSet<String>,

    /// Our GAM shell: connection, the content canvas we draw the home
    /// screen / transient status onto, and its size. We own these directly
    /// (rather than delegating to the chat library) so we can supply our own
    /// F-key legend via `crate::icontray`.
    gam: gam::Gam,
    content: Gid,
    screensize: Point,

    /// Message-pager page geometry, computed once from the runtime glyph
    /// height so each page fits the modal at the configured font size.
    /// `page_cols` = characters per wrapped line, `page_lines` = content
    /// lines per page, `pad_lines` = fixed line count each multi-page page is
    /// padded to so every page renders at the same (max) modal height.
    page_cols: usize,
    page_lines: usize,
    pad_lines: usize,
}

impl MailApp {
    /// Stands up the app's own GAM shell (no chat library): registers a Ux
    /// context whose `predictor` is our icontray (so the F-key legend reads
    /// INBOX/WRITE/CONFIG/REPLY), spawns that icontray, and grabs a content
    /// canvas to draw the home screen on. `sid` is our server, given to the
    /// GAM as the listener for redraw/rawkeys/focus events.
    pub fn new(xns: &xous_names::XousNames, sid: xous::SID) -> Self {
        // Ensure the pddb is mounted before we read account config or write
        // TLS trust anchors. The chat library used to do this in its Ux
        // setup; now that we run our own shell we must do it ourselves,
        // otherwise pddb writes fail with "Uninit".
        pddb::Pddb::new().try_mount();

        let gam = gam::Gam::new(xns).expect("can't connect to GAM");

        // Our F-key legend server. Spawned before registering the Ux so the
        // predictor name resolves when the GAM connects on focus.
        std::thread::spawn(|| crate::icontray::icontray_server());

        let token = gam
            .register_ux(UxRegistration {
                app_name: String::from(gam::APP_NAME_MAILAPP),
                ux_type: gam::UxType::Chat,
                predictor: Some(String::from(crate::icontray::SERVER_NAME_ICONTRAY)),
                listener: sid.to_array(),
                redraw_id: MailOp::Redraw.to_u32().unwrap(),
                gotinput_id: Some(MailOp::Line.to_u32().unwrap()),
                audioframe_id: None,
                rawkeys_id: Some(MailOp::Rawkeys.to_u32().unwrap()),
                focuschange_id: Some(MailOp::ChangeFocus.to_u32().unwrap()),
            })
            .expect("couldn't register Ux context for mail")
            .unwrap();

        // Put the IME into "menu mode" so the F1-F4 keys act as menu
        // selects (delivered to us as raw keys) instead of picking the
        // predictor slots and *typing out* their labels ("INBOX", ...). This
        // is what apps/vault does for its FIDO/123/*** legend.
        gam.toggle_menu_mode(token).expect("couldn't toggle menu mode");

        let content = gam.request_content_canvas(token).expect("couldn't get content canvas");
        let screensize = gam.get_canvas_bounds(content).expect("couldn't get content canvas bounds");

        // Enable the system main menu (the Home/menu key) while this app is
        // in the foreground. It's a global GAM flag normally turned on by the
        // pddb after the boot PIN; the chat library relied on that being set
        // already. We assert it ourselves (idempotent -- same as
        // services/cram-console) so the Home key raises the app switcher.
        gam.allow_mainmenu().ok();

        let modals = Modals::new(xns).expect("can't connect to Modals server");
        let (page_cols, page_lines, pad_lines) = compute_page_geometry(&gam);
        log::info!("mail: pager geometry {} cols x {} lines (pad to {})", page_cols, page_lines, pad_lines);

        let mut app = MailApp {
            modals,
            imap_user: String::new(),
            imap_pass: String::new(),
            imap_host: String::new(),
            imap_port: DEFAULT_IMAP_PORT,
            smtp_user: String::new(),
            smtp_pass: String::new(),
            smtp_from: String::new(),
            smtp_host: String::new(),
            smtp_port: DEFAULT_SMTP_PORT,
            inbox: Vec::new(),
            open_msg: None,
            trusted: HashSet::new(),
            gam,
            content,
            screensize,
            page_cols,
            page_lines,
            pad_lines,
        };
        app.load_config();
        app
    }

    // ---- home screen drawing ------------------------------------------

    /// Clears the content canvas to the background colour.
    fn clear(&self) {
        self.gam
            .draw_rectangle(
                self.content,
                Rectangle::new_with_style(
                    Point::new(0, 0),
                    self.screensize,
                    DrawStyle { fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0 },
                ),
            )
            .ok();
    }

    /// Draws the home screen: a title and the F-key legend. Called on focus
    /// and after any modal flow returns.
    pub fn redraw(&self) {
        self.clear();
        let mut tv = TextView::new(
            self.content,
            TextBounds::GrowableFromTl(Point::new(6, 6), (self.screensize.x - 12) as u16),
        );
        tv.style = GlyphStyle::Regular;
        tv.draw_border = false;
        tv.clear_area = true;
        tv.margin = Point::new(0, 0);
        write!(
            tv.text,
            "Mail\n\nF1  INBOX   - list & read messages\nF2  WRITE   - compose a new message\nF3  CONFIG  - IMAP/SMTP account settings\nF4  REPLY   - reply to the open message\n\nNo account yet? Start under F3 (CONFIG).\nThe labels above also show under the F-keys.\n\nWhile reading an email:\n - down/space (next page)\n - up (prev page) \n - backspace/enter (exit reading)"
        )
        .ok();
        self.gam.post_textview(&mut tv).ok();
        self.gam.redraw().ok();
    }

    /// Draws a transient one-line status message (e.g. "Fetching inbox...")
    /// centred on the canvas, before a blocking network call. It stays on
    /// screen until the operation finishes and we `redraw()` or a modal
    /// covers it.
    fn status(&self, msg: &str) {
        self.clear();
        let mut tv = TextView::new(
            self.content,
            TextBounds::GrowableFromTl(Point::new(6, self.screensize.y / 3), (self.screensize.x - 12) as u16),
        );
        tv.style = GlyphStyle::Large;
        tv.draw_border = false;
        tv.clear_area = true;
        tv.margin = Point::new(0, 0);
        write!(tv.text, "{}", msg).ok();
        self.gam.post_textview(&mut tv).ok();
        self.gam.redraw().ok();
    }

    // ---- F1: inbox ----------------------------------------------------

    pub fn inbox(&mut self) {
        if self.imap_host.is_empty() {
            self.notify("No IMAP server configured. Set one up under F3 (CONFIG) first.");
            return;
        }
        // Deal with TLS trust first, so the trust modal (if the chain isn't
        // trusted yet) is presented while the app is interactive.
        let (host, port) = (self.imap_host.clone(), self.imap_port);
        self.ensure_trusted(&host, port);

        // Page through the mailbox PAGE_SIZE messages at a time. The picker
        // gets "Previous 10" / "Next 10" rows in addition to the messages;
        // picking one navigates and re-fetches, picking a message opens it.
        const PREV_LABEL: &str = "<< Previous 10";
        const NEXT_LABEL: &str = "Next 10 >>";
        let mut page = 0usize;
        loop {
            self.status("Fetching inbox...");
            let result = self.imap_list_page(page, PAGE_SIZE);

            let (total, entries) = match result {
                Ok(v) => v,
                Err(e) => {
                    self.notify(&e);
                    return;
                }
            };
            if total == 0 {
                self.notify("Mailbox is empty.");
                return;
            }
            if entries.is_empty() {
                // Ran off the end (shouldn't normally happen since we only
                // offer Next when there's more); step back a page.
                if page == 0 {
                    self.notify("Mailbox is empty.");
                    return;
                }
                page -= 1;
                continue;
            }
            self.inbox = entries;

            // "From - Subject" per message, truncated for the narrow LCD.
            let msg_labels: Vec<String> = self
                .inbox
                .iter()
                .map(|e| format!("{} - {}", truncate(&e.from, 22), truncate(&e.subject, 34)))
                .collect();

            // Absolute recency range shown on this page (1 = newest).
            let first_recency = page * PAGE_SIZE + 1;
            let last_recency = first_recency + self.inbox.len() - 1;
            let has_prev = page > 0;
            let has_next = (last_recency as u32) < total;

            // Display order: Previous (top), messages, Next (bottom).
            if has_prev {
                self.modals.add_list_item(PREV_LABEL).ok();
            }
            for label in &msg_labels {
                self.modals.add_list_item(label).ok();
            }
            if has_next {
                self.modals.add_list_item(NEXT_LABEL).ok();
            }

            let title = format!("Inbox {}-{} of {}", first_recency, last_recency, total);
            let chosen = match self.modals.get_radiobutton(&title) {
                Ok(c) => c,
                Err(_) => return, // dismissed
            };

            if chosen == NEXT_LABEL {
                page += 1;
                continue;
            }
            if chosen == PREV_LABEL {
                page = page.saturating_sub(1);
                continue;
            }
            // Otherwise a message was picked: map its label back to a recency.
            if let Some(idx) = msg_labels.iter().position(|l| l == &chosen) {
                let recency = self.inbox[idx].recency;
                self.open_message(recency);
            }
            return;
        }
    }

    /// Fetch and decode message `recency` (1 = most recent), then display it
    /// in a self-managed modal pager (see `page_message`).
    fn open_message(&mut self, recency: usize) {
        let (host, port) = (self.imap_host.clone(), self.imap_port);
        self.ensure_trusted(&host, port);
        self.status("Loading message...");
        let result = self.imap_read_body(recency);

        match result {
            Ok((from, subject, body)) => {
                self.page_message(&from, &subject, &body);
                // Remember it so F4 (reply) can pre-fill from it.
                self.open_msg = Some(OpenMessage { from, subject, body });
            }
            Err(e) => self.notify(&e),
        }
    }

    /// Displays a message in a modal pager with per-key navigation. The
    /// From/Subject header and body are word-wrapped and split into
    /// fixed-size pages (see `paginate`, sized by `page_cols`/`page_lines` to
    /// fit the modal), then shown one page at a time in a `dynamic_notification`
    /// whose keystrokes are delivered to us (rather than any key dismissing):
    ///
    ///   * Down or Space -> next page
    ///   * Up    -> previous page
    ///   * Enter or Backspace -> close the reader
    ///   * anything else -> ignored (stays on the page)
    ///
    /// Every open starts at page 1 (top of the message) -- there's no shared
    /// scroll state to carry over.
    fn page_message(&mut self, from: &str, subject: &str, body: &str) {
        let full = format!("From: {}\nSubject: {}\n\n{}", from, subject, body);
        let pages = paginate(&full, self.page_cols, self.page_lines, self.pad_lines);
        let n = pages.len();
        let mut idx = 0usize;

        self.modals.dynamic_notification(page_title(idx, n).as_deref(), Some(pages[idx].as_str())).ok();

        // The blocking listener returns one key per call and re-arms while
        // the notification stays open (see apps/vault for the same idiom).
        let token = self.modals.token();
        let conn = self.modals.conn();
        loop {
            match modals::dynamic_notification_blocking_listener(token, conn) {
                Ok(Some(key)) => match key {
                    // Down or Space: next page; Up: previous page.
                    // Clamped at the ends (no wrap).
                    '\u{2193}' | ' ' => {
                        if idx + 1 < n {
                            idx += 1;
                            self.modals
                                .dynamic_notification_update(page_title(idx, n).as_deref(), Some(pages[idx].as_str()))
                                .ok();
                        }
                    }
                    '\u{2191}' => {
                        if idx > 0 {
                            idx -= 1;
                            self.modals
                                .dynamic_notification_update(page_title(idx, n).as_deref(), Some(pages[idx].as_str()))
                                .ok();
                        }
                    }
                    // Enter ('∴' or CR/LF) or Backspace/Delete: close.
                    '\u{2234}' | '\u{d}' | '\n' | '\u{8}' | '\u{7f}' => break,
                    _ => {} // ignore other keys; stay on the current page
                },
                Ok(None) => break, // modal closed / unblocked with no key
                Err(_) => break,
            }
        }
        self.modals.dynamic_notification_close().ok();
    }

    // ---- F2: compose --------------------------------------------------

    pub fn compose(&mut self) {
        if self.smtp_host.is_empty() {
            self.notify("No SMTP server configured. Set one up under F3 (CONFIG) first.");
            return;
        }
        // Gather To / Subject / Body in one form. Body is growable so it can
        // hold more than a single line. Each `.field()`/`.set_growable()`
        // returns `&mut Self`, so the builder is threaded through by
        // rebinding (see apps/mtxchat for the same pattern).
        let payloads = {
            let mut builder = self.modals.alert_builder("Compose message");
            let builder = builder.field(Some("recipient@example.com".to_string()), None);
            let builder = builder.field(Some("Subject".to_string()), None);
            let builder = builder.field(Some("Message body".to_string()), None);
            let builder = builder.set_growable();
            match builder.build() {
                Ok(p) => p,
                Err(_) => return, // dismissed
            }
        };
        let content = payloads.content();
        let to = content[0].content.as_str().trim().to_string();
        let subject = content[1].content.as_str().trim().to_string();
        let body = content[2].content.as_str().to_string();
        self.send_and_report(&to, &subject, &body);
    }

    // ---- F4: reply ----------------------------------------------------

    /// Pre-fills a compose form as a reply to the message currently open
    /// under F1: To = the sender's address, Subject = "Re: ...", Body = a
    /// blank space to type in above the quoted original. Then sends.
    pub fn reply(&mut self) {
        // Snapshot the open message up front so we don't hold a borrow of
        // self across the modal / send calls (which need &mut self).
        let (orig_from, orig_subject, orig_body) = match &self.open_msg {
            Some(m) => (m.from.clone(), m.subject.clone(), m.body.clone()),
            None => {
                self.notify("No message open. Open one under F1 (Inbox), then F4 to reply.");
                return;
            }
        };
        if self.smtp_host.is_empty() {
            self.notify("No SMTP server configured. Set one up under F3 (CONFIG) first.");
            return;
        }

        let to = reply_address(&orig_from);
        let subject = reply_subject(&orig_subject);
        let quoted = quote_body(&orig_from, &orig_body);

        // Same rebinding-builder idiom as compose, but every field is
        // pre-filled (persistent placeholders) so the reply is ready to send
        // or edit.
        let payloads = {
            let mut builder = self.modals.alert_builder("Reply");
            let builder = builder.field_placeholder_persist(Some(to.clone()), None);
            let builder = builder.field_placeholder_persist(Some(subject.clone()), None);
            let builder = builder.field_placeholder_persist(Some(quoted.clone()), None);
            let builder = builder.set_growable();
            match builder.build() {
                Ok(p) => p,
                Err(_) => return, // dismissed
            }
        };
        let content = payloads.content();
        let to = content[0].content.as_str().trim().to_string();
        let subject = content[1].content.as_str().trim().to_string();
        let body = content[2].content.as_str().to_string();
        self.send_and_report(&to, &subject, &body);
    }

    /// Shared send tail for compose (F2) and reply (F4): validates the
    /// recipient, sends over SMTP with a status indicator, and reports the
    /// outcome in a notification.
    fn send_and_report(&mut self, to: &str, subject: &str, body: &str) {
        if to.is_empty() {
            self.notify("No recipient entered; nothing sent.");
            return;
        }
        // Trust the SMTP server's chain (prompting if needed) first.
        let (host, port) = (self.smtp_host.clone(), self.smtp_port);
        self.ensure_trusted(&host, port);
        self.status("Sending...");
        let result = self.smtp_send(to, subject, body);
        self.notify(&result);
    }

    // ---- F3: settings -------------------------------------------------

    pub fn settings(&mut self) {
        // Two groups so neither form is taller than the screen.
        self.modals.add_list_item("IMAP (incoming) server").ok();
        self.modals.add_list_item("SMTP (outgoing) server").ok();
        // get_radiobutton() renders and blocks on the modal; get_radio_index()
        // only reads back which item was chosen *after* that. Calling the
        // latter alone (as before) showed no modal at all -- the F3 no-op bug.
        if self.modals.get_radiobutton("Settings").is_err() {
            return; // dismissed
        }
        match self.modals.get_radio_index() {
            Ok(0) => self.edit_imap(),
            Ok(1) => self.edit_smtp(),
            _ => {} // dismissed / error
        }
    }

    fn edit_imap(&mut self) {
        // Pre-fill each field with the current value as a persistent
        // (editable) placeholder; fall back to a disappearing hint for empty
        // fields. The builder is threaded through by rebinding because each
        // method returns `&mut Self` (see apps/mtxchat for the same idiom).
        // The password field is pre-filled with a run of asterisks the same
        // length as the stored password (so its length is visible without
        // revealing it); if it comes back unchanged we keep the real one.
        let pass_mask = mask(&self.imap_pass);
        let payloads = {
            let mut builder = self.modals.alert_builder("IMAP settings");
            let builder = if self.imap_host.is_empty() {
                builder.field(Some("imap.example.com".to_string()), None)
            } else {
                builder.field_placeholder_persist(Some(self.imap_host.clone()), None)
            };
            let builder = if self.imap_user.is_empty() {
                builder.field(Some("username".to_string()), None)
            } else {
                builder.field_placeholder_persist(Some(self.imap_user.clone()), None)
            };
            let builder = if self.imap_pass.is_empty() {
                builder.field(Some("password".to_string()), None)
            } else {
                builder.field_placeholder_persist(Some(pass_mask.clone()), None)
            };
            let builder = builder.field_placeholder_persist(Some(self.imap_port.to_string()), None);
            match builder.build() {
                Ok(p) => p,
                Err(_) => return,
            }
        };
        let content = payloads.content();
        self.imap_host = content[0].content.as_str().trim().to_string();
        self.imap_user = content[1].content.as_str().trim().to_string();
        let pass_in = content[2].content.as_str();
        // Unchanged if it still equals the mask we pre-filled (all asterisks,
        // same length); otherwise the user typed a new password.
        if pass_in != pass_mask.as_str() {
            self.imap_pass = pass_in.to_string();
        }
        let mut port_note = "";
        match content[3].content.as_str().trim().parse::<u16>() {
            Ok(p) => self.imap_port = p,
            Err(_) => port_note = " (port unchanged: not a number)",
        }
        log::info!(
            "--> IMAP settings captured: host='{}' user='{}' pass={} chars port={}",
            self.imap_host,
            self.imap_user,
            self.imap_pass.chars().count(),
            self.imap_port
        );
        self.save_config();
        self.notify(&format!("IMAP settings saved.{}", port_note));
    }

    fn edit_smtp(&mut self) {
        let pass_mask = mask(&self.smtp_pass);
        let payloads = {
            let mut builder = self.modals.alert_builder("SMTP settings");
            let builder = if self.smtp_host.is_empty() {
                builder.field(Some("smtp.example.com".to_string()), None)
            } else {
                builder.field_placeholder_persist(Some(self.smtp_host.clone()), None)
            };
            let builder = if self.smtp_user.is_empty() {
                builder.field(Some("username".to_string()), None)
            } else {
                builder.field_placeholder_persist(Some(self.smtp_user.clone()), None)
            };
            let builder = if self.smtp_pass.is_empty() {
                builder.field(Some("password".to_string()), None)
            } else {
                builder.field_placeholder_persist(Some(pass_mask.clone()), None)
            };
            let builder = if self.smtp_from.is_empty() {
                builder.field(Some("you@example.com (From address)".to_string()), None)
            } else {
                builder.field_placeholder_persist(Some(self.smtp_from.clone()), None)
            };
            let builder = builder.field_placeholder_persist(Some(self.smtp_port.to_string()), None);
            match builder.build() {
                Ok(p) => p,
                Err(_) => return,
            }
        };
        let content = payloads.content();
        self.smtp_host = content[0].content.as_str().trim().to_string();
        self.smtp_user = content[1].content.as_str().trim().to_string();
        let pass_in = content[2].content.as_str();
        if pass_in != pass_mask.as_str() {
            self.smtp_pass = pass_in.to_string();
        }
        self.smtp_from = content[3].content.as_str().trim().to_string();
        let mut port_note = "";
        match content[4].content.as_str().trim().parse::<u16>() {
            Ok(p) => self.smtp_port = p,
            Err(_) => port_note = " (port unchanged: not a number)",
        }
        log::info!(
            "--> SMTP settings captured: host='{}' user='{}' pass={} chars from='{}' port={}",
            self.smtp_host,
            self.smtp_user,
            self.smtp_pass.chars().count(),
            self.smtp_from,
            self.smtp_port
        );
        self.save_config();
        self.notify(&format!("SMTP settings saved.{}", port_note));
    }

    // ---- config persistence (pddb) ------------------------------------

    fn config_path() -> PathBuf {
        let mut p = PathBuf::new();
        p.push(MAIL_DICT);
        p.push(MAIL_CONFIG_KEY);
        p
    }

    /// Serializes the account settings as "key=value" lines and writes them
    /// to the pddb (creating the dict on first save). Same on-disk shape as
    /// edlin's "mail" file, so the format is familiar.
    fn save_config(&self) {
        let mut dict = PathBuf::new();
        dict.push(MAIL_DICT);
        if std::fs::metadata(&dict).is_err() {
            if let Err(e) = std::fs::create_dir_all(&dict) {
                log::error!("mail: couldn't create pddb dict '{}': {}", MAIL_DICT, e);
                return;
            }
        }
        let body = format!(
            "imap_host={}\nimap_user={}\nimap_pass={}\nimap_port={}\nsmtp_host={}\nsmtp_user={}\nsmtp_pass={}\nsmtp_from={}\nsmtp_port={}\n",
            self.imap_host,
            self.imap_user,
            self.imap_pass,
            self.imap_port,
            self.smtp_host,
            self.smtp_user,
            self.smtp_pass,
            self.smtp_from,
            self.smtp_port,
        );
        log::info!(
            "--> saving config to '{}/{}': imap_host='{}' imap_user='{}' imap_pass={} chars imap_port={} smtp_host='{}' smtp_user='{}' smtp_pass={} chars smtp_from='{}' smtp_port={} (total {} bytes)",
            MAIL_DICT,
            MAIL_CONFIG_KEY,
            self.imap_host,
            self.imap_user,
            self.imap_pass.chars().count(),
            self.imap_port,
            self.smtp_host,
            self.smtp_user,
            self.smtp_pass.chars().count(),
            self.smtp_from,
            self.smtp_port,
            body.len(),
        );
        match File::create(Self::config_path()) {
            Ok(mut f) => {
                if let Err(e) = f.write_all(body.as_bytes()) {
                    log::error!("mail: couldn't write config: {}", e);
                } else {
                    log::info!("--> config written OK");
                }
            }
            Err(e) => log::error!("mail: couldn't open config for write: {}", e),
        }
    }

    /// Loads and applies the "key=value" account settings from the pddb.
    /// Silently no-ops when nothing has been saved yet (first run).
    fn load_config(&mut self) {
        let mut f = match File::open(Self::config_path()) {
            Ok(f) => f,
            Err(_) => {
                log::info!("mail: no saved config yet");
                return;
            }
        };
        let mut buf = String::new();
        if let Err(e) = f.read_to_string(&mut buf) {
            log::warn!("mail: couldn't read config: {}", e);
            return;
        }
        log::info!("--> loading config from '{}/{}' ({} bytes)", MAIL_DICT, MAIL_CONFIG_KEY, buf.len());
        self.apply_config_lines(&buf);
        log::info!(
            "--> config loaded: imap_host='{}' imap_user='{}' imap_pass={} chars imap_port={} smtp_host='{}' smtp_user='{}' smtp_pass={} chars smtp_from='{}' smtp_port={}",
            self.imap_host,
            self.imap_user,
            self.imap_pass.chars().count(),
            self.imap_port,
            self.smtp_host,
            self.smtp_user,
            self.smtp_pass.chars().count(),
            self.smtp_from,
            self.smtp_port,
        );
    }

    /// Parses "key=value" lines and applies recognized keys. Blank lines and
    /// '#' comments are ignored; unparseable ports leave the default.
    fn apply_config_lines(&mut self, text: &str) {
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = match line.split_once('=') {
                Some((k, v)) => (k.trim(), v.trim()),
                None => continue,
            };
            // Don't dump passwords to the log; show their length instead.
            if key == "imap_pass" || key == "smtp_pass" {
                log::info!("--> grabbing {}=<{} chars>", key, value.chars().count());
            } else {
                log::info!("--> grabbing {}={}", key, value);
            }
            match key {
                "imap_user" => self.imap_user = value.to_string(),
                "imap_pass" => self.imap_pass = value.to_string(),
                "imap_host" => self.imap_host = value.to_string(),
                "imap_port" => {
                    if let Ok(p) = value.parse::<u16>() {
                        self.imap_port = p;
                    }
                }
                "smtp_user" => self.smtp_user = value.to_string(),
                "smtp_pass" => self.smtp_pass = value.to_string(),
                "smtp_from" => self.smtp_from = value.to_string(),
                "smtp_host" => self.smtp_host = value.to_string(),
                "smtp_port" => {
                    if let Ok(p) = value.parse::<u16>() {
                        self.smtp_port = p;
                    }
                }
                _ => {}
            }
        }
    }

    // ---- TLS trust ----------------------------------------------------

    /// Probes `host:port`'s offered TLS certificate chain and, if none of it
    /// is already trusted, shows the GAM "trust this certificate?" modal so
    /// the user can save a trust anchor to the pddb. Call this *before*
    /// connecting (and before entering a Chat busy-state), so the modal is
    /// presented while the app is interactive -- the same trust prompt that
    /// apps/edlin surfaced when checking mail.
    ///
    /// This is deliberately explicit rather than relying only on the
    /// implicit retry inside libs/mail: doing the probe up front makes the
    /// prompt deterministic, and once a trust anchor is saved the subsequent
    /// `ImapClient`/`SmtpClient` connect verifies against it normally.
    ///
    /// Results are cached per endpoint for the session so we only probe once
    /// per host:port (unless the user declined, in which case we re-prompt).
    fn ensure_trusted(&mut self, host: &str, port: u16) {
        if host.is_empty() {
            return;
        }
        let key = format!("{host}:{port}");
        if self.trusted.contains(&key) {
            return;
        }
        let tls = tls::Tls::new();
        match tls.probe_port(host, port) {
            Ok(certs) if !certs.is_empty() => {
                if certs.iter().any(|c| tls.is_trusted_cert(c.clone())) {
                    log::info!("--> TLS chain for {key} already has a trusted anchor");
                    self.trusted.insert(key);
                } else {
                    log::info!("--> TLS chain for {key} is untrusted; prompting user to trust it");
                    let n = tls.trust_modal(certs);
                    log::info!("--> user trusted {n} cert(s) for {key}");
                    // Only cache if the user actually trusted something;
                    // otherwise let the next attempt prompt again.
                    if n > 0 {
                        self.trusted.insert(key);
                    }
                }
            }
            Ok(_) => log::info!("--> TLS probe of {key} returned no certificates"),
            Err(e) => log::info!("--> TLS probe of {key} failed: {e}"),
        }
    }

    // ---- IMAP / SMTP operations ---------------------------------------

    /// Connects, selects INBOX, and returns one page of messages' From +
    /// Subject. `page` is 0-based; each page holds up to `page_size`
    /// messages ordered newest-first. Each entry's `recency` is its absolute
    /// position in the mailbox (1 = newest), so `imap_fetch_raw` can re-fetch
    /// it regardless of which page it came from.
    ///
    /// Returns `(total_messages_in_mailbox, page_entries)`. An empty entries
    /// vec with a non-zero total means the requested page is past the end.
    /// Uses BODY.PEEK so listing has no side effects on the mailbox.
    fn imap_list_page(&mut self, page: usize, page_size: usize) -> Result<(u32, Vec<InboxEntry>), String> {
        log::info!(
            "--> IMAP connect {}:{} user='{}' pass={} chars (inbox page {})",
            self.imap_host,
            self.imap_port,
            self.imap_user,
            self.imap_pass.chars().count(),
            page
        );
        let mut client = ImapClient::connect(&self.imap_host, self.imap_port).map_err(|e| {
            log::info!("--> IMAP connect error: {}", e);
            format!("IMAP connect failed: {}", e)
        })?;
        log::info!("--> IMAP connected; logging in as '{}'", self.imap_user);
        client.login(&self.imap_user, &self.imap_pass).map_err(|e| {
            log::info!("--> IMAP login error: {}", e);
            format!("IMAP login failed: {}", e)
        })?;
        log::info!("--> IMAP login OK; selecting INBOX");
        let select_resp = client.select("INBOX").map_err(|e| format!("IMAP SELECT failed: {}", e))?;
        let total = parse_exists(&select_resp).unwrap_or(0);
        if total == 0 {
            let _ = client.logout();
            return Ok((0, Vec::new()));
        }

        // Map the 0-based page to an absolute recency window (1 = newest),
        // then to IMAP sequence numbers (seq = total - (recency - 1)).
        let first_recency = (page * page_size + 1) as u32;
        if first_recency > total {
            let _ = client.logout();
            return Ok((total, Vec::new())); // past the end of the mailbox
        }
        let last_recency = (first_recency + page_size as u32 - 1).min(total);
        let high_seq = total - (first_recency - 1); // newest message on the page
        let low_seq = total - (last_recency - 1); // oldest message on the page
        let range = format!("{}:{}", low_seq, high_seq);
        log::info!(
            "--> IMAP fetch page {} range {} (recency {}..{} of {})",
            page,
            range,
            first_recency,
            last_recency,
            total
        );

        let responses = client
            .fetch(&range, "BODY.PEEK[HEADER.FIELDS (SUBJECT FROM)]")
            .map_err(|e| format!("IMAP FETCH failed: {}", e));
        let _ = client.logout();
        let responses = responses?;

        let mut items: Vec<(u32, InboxEntry)> = Vec::new();
        for chunks in responses.iter() {
            let seq = chunks
                .first()
                .and_then(|c| match c {
                    ImapChunk::Text(t) => parse_seq_num(&String::from_utf8_lossy(t)),
                    ImapChunk::Literal(_) => None,
                })
                .unwrap_or(0);
            let flat = flatten_chunks(chunks);
            let subject = extract_header(&flat, "subject:").unwrap_or_else(|| String::from("(no subject)"));
            let from = extract_header(&flat, "from:").unwrap_or_else(|| String::from("(unknown sender)"));
            items.push((seq, InboxEntry { recency: 0, from, subject }));
        }
        items.sort_by(|a, b| b.0.cmp(&a.0)); // descending: most recent first

        // Recency is derived directly from the sequence number, so it's the
        // absolute mailbox position no matter which page we fetched.
        let entries = items
            .into_iter()
            .map(|(seq, mut e)| {
                e.recency = (total - seq + 1) as usize;
                e
            })
            .collect();
        Ok((total, entries))
    }

    /// Connects, selects INBOX, and fetches the raw bytes of message
    /// `recency` (1 = most recent) via BODY.PEEK[]. Returns (total, raw).
    fn imap_fetch_raw(&mut self, recency: usize) -> Result<(u32, Vec<u8>), String> {
        if recency == 0 {
            return Err(String::from("Message number must be 1 or greater."));
        }
        log::info!("--> IMAP connect {}:{} (fetch #{})", self.imap_host, self.imap_port, recency);
        let mut client = ImapClient::connect(&self.imap_host, self.imap_port).map_err(|e| {
            log::info!("--> IMAP connect error: {}", e);
            format!("IMAP connect failed: {}", e)
        })?;
        client.login(&self.imap_user, &self.imap_pass).map_err(|e| {
            log::info!("--> IMAP login error: {}", e);
            format!("IMAP login failed: {}", e)
        })?;
        let select_resp = client.select("INBOX").map_err(|e| format!("IMAP SELECT failed: {}", e))?;
        let total = parse_exists(&select_resp).unwrap_or(0);
        if total == 0 {
            let _ = client.logout();
            return Err(String::from("Mailbox is empty."));
        }
        if recency as u32 > total {
            let _ = client.logout();
            return Err(format!("Only {} message(s) in mailbox.", total));
        }
        let seq = total - (recency as u32 - 1);

        let responses = client.fetch(&seq.to_string(), "BODY.PEEK[]").map_err(|e| format!("IMAP FETCH failed: {}", e));
        let _ = client.logout();
        let responses = responses?;

        let mut raw = Vec::new();
        for chunks in responses.iter() {
            for chunk in chunks {
                if let ImapChunk::Literal(bytes) = chunk {
                    raw.extend_from_slice(bytes);
                }
            }
        }
        Ok((total, raw))
    }

    /// Fetches message `recency`, walks to a readable text/plain part,
    /// transfer-decodes it, and returns (From, Subject, body text).
    fn imap_read_body(&mut self, recency: usize) -> Result<(String, String, String), String> {
        let (_total, raw) = self.imap_fetch_raw(recency)?;
        let text = String::from_utf8_lossy(&raw).into_owned();
        let (top_headers, top_body) = split_headers_body(&text);

        // From/Subject come from the *real* top-level header block, so the
        // line-anchored header_value parser is correct here (unlike the
        // concatenated HEADER.FIELDS fetch used by imap_list).
        let from = header_value(top_headers, "from")
            .map(|v| decode_rfc2047(&v))
            .unwrap_or_else(|| String::from("(unknown sender)"));
        let subject = header_value(top_headers, "subject")
            .map(|v| decode_rfc2047(&v))
            .unwrap_or_else(|| String::from("(no subject)"));

        let (part_headers, part_body) = find_text_part(top_headers, top_body, 4);
        let cte = header_value(&part_headers, "content-transfer-encoding").map(|v| v.to_lowercase());
        let body = match cte.as_deref() {
            Some("quoted-printable") => decode_quoted_printable(&part_body),
            Some("base64") => {
                let joined: String = part_body.split_whitespace().collect();
                B64.decode(joined.as_bytes())
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .unwrap_or(part_body)
            }
            _ => part_body,
        };
        Ok((from, subject, body))
    }

    /// Connects to the SMTP server and sends a message. `body` is the plain
    /// message text; headers are assembled here.
    fn smtp_send(&mut self, to_addr: &str, subject: &str, body: &str) -> String {
        if self.smtp_from.is_empty() {
            return String::from("No From address configured (F3 - SMTP settings).");
        }
        // Normalize the body to CRLF line endings for the wire; dot-stuffing
        // is handled inside SmtpClient::send.
        let body_crlf = body.replace("\r\n", "\n").replace('\n', "\r\n");
        // NOTE: no Date:/Message-ID: header -- the device needs an RTC-backed
        // clock to generate a compliant Date:. Some spam filters may
        // downgrade mail without them. Left as a follow-up (same as edlin).
        let message = format!(
            "From: {}\r\nTo: {}\r\nSubject: {}\r\n\r\n{}",
            self.smtp_from, to_addr, subject, body_crlf
        );

        let mut client = match SmtpClient::connect(&self.smtp_host, self.smtp_port) {
            Ok(c) => c,
            Err(e) => return format!("SMTP connect failed: {}", e),
        };
        // EHLO wants the client's own identity; use the domain half of the
        // From address as a stand-in (the device has no FQDN of its own).
        let ehlo_domain = self.smtp_from.split('@').nth(1).unwrap_or(self.smtp_host.as_str()).to_string();
        if let Err(e) = client.ehlo(&ehlo_domain) {
            return format!("SMTP EHLO failed: {}", e);
        }
        if let Err(e) = client.auth_login(&self.smtp_user, &self.smtp_pass) {
            return format!("SMTP auth failed: {}", e);
        }
        if let Err(e) = client.send(&self.smtp_from, &[to_addr], &message) {
            return format!("SMTP send failed: {}", e);
        }
        let _ = client.quit();
        format!("Sent to {}.", to_addr)
    }

    // ---- small helpers ------------------------------------------------

    fn notify(&self, msg: &str) {
        self.modals.show_notification(msg, None).ok();
    }
}

/// Computes the message-pager page geometry from the runtime glyph height,
/// so pages fit the modal at whatever font size is configured (this build's
/// SYSTEM_STYLE is Large = 24px). Returns `(cols, lines)`.
///
/// Vertical (the dimension that actually clips): the modal is at most
/// `MODAL_Y_MAX_PX` tall, i.e. `MODAL_Y_MAX_PX / line_px` lines; we hold
/// back `MODAL_RESERVED_LINES` for the "page i/N" title line and margins.
/// Horizontal: GAM exposes no glyph-width hint and the fonts are
/// proportional, so we estimate the *average* advance width as ~0.40x the
/// glyph height. That's deliberately a touch wider than reality (measured
/// empirically: text was reaching the full modal width at ~0.37x), which
/// leaves a small margin so an occasional wide line doesn't force the modal
/// to re-wrap (which would add a line and risk a vertical clip).
/// Returns `(page_cols, page_lines, pad_lines)`: characters per wrapped
/// line, content lines per page, and the fixed line count every multi-page
/// page is padded to (which exceeds the modal's max height, forcing a
/// constant clamped height -- see `paginate`).
fn compute_page_geometry(gam: &gam::Gam) -> (usize, usize, usize) {
    let line_px = gam.glyph_height_hint(gam::SYSTEM_STYLE).ok().unwrap_or(24).max(1);

    let total_lines = (MODAL_Y_MAX_PX / line_px).max(1);
    let page_lines = total_lines.saturating_sub(MODAL_RESERVED_LINES).max(3);
    // Pad past the top of the modal so every page overflows and clamps to the
    // same max height.
    let pad_lines = total_lines + PAD_EXTRA;

    let avg_glyph_px = (line_px * 40 / 100).max(1);
    let page_cols = (MODAL_WIDTH_PX / avg_glyph_px).max(8);

    (page_cols, page_lines, pad_lines)
}

/// The reader's dynamic-notification title: "page i/N" when there's more
/// than one page, or `None` for a single-page message.
fn page_title(idx: usize, n: usize) -> Option<String> {
    if n > 1 { Some(format!("page {}/{}", idx + 1, n)) } else { None }
}

/// Word-wraps `text` to at most `cols` characters per line (preserving
/// existing line breaks and blank lines; hard-splitting any single word
/// longer than `cols`), then groups the wrapped lines into pages of
/// `lines_per_page` content lines.
///
/// When there is more than one page, every page is padded with blank " "
/// lines out to `pad_to` lines. `pad_to` is chosen (see
/// `compute_page_geometry`) to exceed the modal's max height, so *every*
/// page overflows and the modal clamps it to the same maximum height. This
/// is what keeps the reader a fixed size: the modal auto-sizes to content
/// and doesn't clear the screen when it shrinks, so a shorter page would
/// otherwise leave the previous page's residue behind -- and because the
/// proportional font wraps some lines wider than our character estimate,
/// even equal content-line counts render at different heights. Overflowing
/// every page removes both problems. The extra blank lines are simply
/// clipped, and content (kept under the clamp by `page_lines`) stays fully
/// visible at the top. A single-page message isn't padded -- there's
/// nothing to shrink from.
fn paginate(text: &str, cols: usize, lines_per_page: usize, pad_to: usize) -> Vec<String> {
    let wrapped = wrap_lines(text, cols);
    let lpp = lines_per_page.max(1);
    if wrapped.is_empty() {
        return vec![String::new()];
    }
    let chunks: Vec<&[String]> = wrapped.chunks(lpp).collect();
    let pad = chunks.len() > 1;
    chunks
        .iter()
        .map(|chunk| {
            let mut lines: Vec<&str> = chunk.iter().map(|s| s.as_str()).collect();
            if pad {
                while lines.len() < pad_to {
                    lines.push(" ");
                }
            }
            lines.join("\n")
        })
        .collect()
}

/// Greedy word-wrap to `cols` chars, one output entry per visual line.
fn wrap_lines(text: &str, cols: usize) -> Vec<String> {
    let cols = cols.max(1);
    let mut out: Vec<String> = Vec::new();
    for raw in text.lines() {
        if raw.is_empty() {
            out.push(String::new()); // preserve paragraph spacing
            continue;
        }
        let mut cur = String::new();
        for word in raw.split(' ') {
            // Hard-split a word that can't fit on a line by itself.
            if word.chars().count() > cols {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                let mut rest = word;
                while rest.chars().count() > cols {
                    let split_at = rest.char_indices().nth(cols).map(|(i, _)| i).unwrap_or(rest.len());
                    out.push(rest[..split_at].to_string());
                    rest = &rest[split_at..];
                }
                cur = rest.to_string();
                continue;
            }
            let projected =
                if cur.is_empty() { word.chars().count() } else { cur.chars().count() + 1 + word.chars().count() };
            if !cur.is_empty() && projected > cols {
                out.push(std::mem::take(&mut cur));
                cur = word.to_string();
            } else {
                if !cur.is_empty() {
                    cur.push(' ');
                }
                cur.push_str(word);
            }
        }
        out.push(cur);
    }
    out
}

/// A run of asterisks the same length (in characters) as `s`. Used to
/// pre-fill the password field in the settings form so its length is
/// visible without revealing the password; an unchanged field comes back
/// equal to this and is treated as "keep the stored password".
fn mask(s: &str) -> String { "*".repeat(s.chars().count()) }

/// Extracts a bare email address to reply to from a From header value.
/// Handles "Display Name <addr@host>" (returns the angle-bracketed address)
/// and a plain "addr@host" (returned as-is). Falls back to the trimmed
/// input if there's no parseable address.
fn reply_address(from: &str) -> String {
    if let Some(start) = from.rfind('<') {
        if let Some(len) = from[start + 1..].find('>') {
            let addr = from[start + 1..start + 1 + len].trim();
            if !addr.is_empty() {
                return addr.to_string();
            }
        }
    }
    from.trim().to_string()
}

/// Builds the reply subject: keeps an existing "Re:" prefix (case-
/// insensitive) rather than stacking "Re: Re: ...", otherwise prepends one.
fn reply_subject(subject: &str) -> String {
    let s = subject.trim();
    if s.is_empty() {
        String::from("Re:")
    } else if s.get(..3).map(|p| p.eq_ignore_ascii_case("re:")).unwrap_or(false) {
        s.to_string()
    } else {
        format!("Re: {}", s)
    }
}

/// Builds the quoted reply body: two blank lines to type the reply into,
/// an attribution line, then the original body with each line "> "-quoted
/// (standard top-posting layout).
fn quote_body(from: &str, body: &str) -> String {
    let mut out = String::from("\n\n");
    out.push_str(from.trim());
    out.push_str(" wrote:\n");
    for line in body.lines() {
        out.push_str("> ");
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Truncates a string to `max` chars (char-boundary safe), adding a ".."
/// marker when cut. Keeps inbox labels from overrunning the LCD. ASCII ".."
/// rather than "…" since the device font may lack the ellipsis glyph.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(2)).collect();
        out.push_str("..");
        out
    }
}
