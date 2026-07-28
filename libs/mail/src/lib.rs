//! Minimal, hand-rolled SMTP and IMAP clients over the same rustls +
//! std::net::TcpStream pattern used by `libs/tls/src/xtls.rs`. No tokio,
//! no async runtime — synchronous/blocking throughout, since Xous's std
//! target has no executor to drive one.
//!
//! Supports both implicit TLS (connect straight into a handshake — 465
//! for SMTPS, 993 for IMAPS) and STARTTLS (587 for SMTP submission, 143
//! for IMAP — negotiate TLS after a short plaintext exchange).
//!
//! Untrusted certificate chains are handled the same way `xtls.rs` handles
//! them for HTTPS: `Tls::probe_port()` fetches the offered chain on the
//! actual port being connected to (not always 443 — mail servers routinely
//! present a different cert there, or nothing at all), and `Tls::trust_modal()`
//! shows it to the user via a GAM modal so they can choose to trust it and
//! save it to the pddb. No separate UI is needed in callers of this crate;
//! [`connect_tls`] and the `*::connect_starttls` methods drive that flow
//! automatically and retry once the user has made a choice.
//!
//! # Known gaps
//!
//! - [`ImapClient::fetch`] returns raw [`ImapChunk`]s (correctly framed
//!   around `{n}` literals) rather than a parsed FETCH response. Splitting
//!   those into per-attribute values (FLAGS vs BODY[...] vs ENVELOPE)
//!   means parsing IMAP's parenthesized-list grammar, which is out of
//!   scope here — this module solves the wire framing, not the semantics.
//! - Tagged response lines are assumed to never themselves start with a
//!   literal (true in practice); if a server literal-quotes plain-text of
//!   a tagged status line, [`ImapClient`]'s tag matching only looks at
//!   the first text chunk.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use rustls::pki_types::ServerName;
use rustls::{ClientConnection, RootCertStore, StreamOwned};
use tls::Tls;

pub type TlsStream = StreamOwned<ClientConnection, TcpStream>;

// =======================================================================
// Shared TLS plumbing
// =======================================================================

enum TlsWrapError {
    /// rustls rejected the chain because we don't have a trust anchor for
    /// it — recoverable via Tls::probe() + trust_modal(), same as
    /// xtls.rs.
    InvalidCertificate,
    Io(io::Error),
}

impl From<io::Error> for TlsWrapError {
    fn from(e: io::Error) -> Self { TlsWrapError::Io(e) }
}

/// Wraps an already-open TCP socket in TLS. Used both for implicit TLS
/// (fresh socket, TLS from byte 0) and for STARTTLS (socket that's
/// already had a plaintext command exchange on it).
fn wrap_tls(host: &str, mut sock: TcpStream, root_store: RootCertStore) -> Result<TlsStream, TlsWrapError> {
    let server_name = ServerName::try_from(host.to_owned())
        .map_err(|e| TlsWrapError::Io(io::Error::new(io::ErrorKind::InvalidInput, format!("{e}"))))?;

    let config = rustls::ClientConfig::builder().with_root_certificates(root_store).with_no_client_auth();
    let mut conn = ClientConnection::new(Arc::new(config), server_name)
        .map_err(|e| TlsWrapError::Io(io::Error::new(io::ErrorKind::Other, format!("{e}"))))?;

    match conn.complete_io(&mut sock) {
        Ok(_) if conn.peer_certificates().is_some() => {
            log::info!("mail: tls handshake complete for {host}");
            Ok(StreamOwned::new(conn, sock))
        }
        Ok(_) => Err(TlsWrapError::Io(io::Error::new(io::ErrorKind::Other, "handshake completed, no peer cert"))),
        Err(e) => {
            let invalid_cert = e
                .get_ref()
                .and_then(|inner| inner.downcast_ref::<rustls::Error>())
                .map(|re| matches!(re, rustls::Error::InvalidCertificate(_)))
                .unwrap_or(false);
            if invalid_cert { Err(TlsWrapError::InvalidCertificate) } else { Err(TlsWrapError::Io(e)) }
        }
    }
}

/// Probes the untrusted chain *on the port we're actually connecting to*
/// and prompts the user to trust it (same UX as xtls.rs). Returns true if
/// the caller should retry the connection.
///
/// Probing port 443 (Tls::probe()'s default) would ask the wrong
/// question here: a mail host's cert on 993/465/587/143 has no
/// guaranteed relationship to whatever's on 443, if anything is even
/// listening there. Using Tls::probe_port() instead makes sure the chain
/// shown to the user, and the trust anchor saved, actually matches the
/// connection that's failing.
fn retry_after_probe(tls: &Tls, host: &str, port: u16) -> bool {
    match tls.probe_port(host, port) {
        Ok(certs) if !certs.is_empty() => {
            log::info!("mail: untrusted cert chain for {host}:{port}, prompting user to trust");
            tls.trust_modal(certs);
            true
        }
        _ => false,
    }
}

/// Implicit TLS: connect straight into a handshake on a dedicated TLS
/// port (465 for SMTPS, 993 for IMAPS).
pub fn connect_tls(host: &str, port: u16) -> io::Result<TlsStream> {
    loop {
        let tls = Tls::new();
        let sock = TcpStream::connect((host, port))?;
        match wrap_tls(host, sock, tls.root_store()) {
            Ok(stream) => return Ok(stream),
            Err(TlsWrapError::InvalidCertificate) => {
                if retry_after_probe(&tls, host, port) {
                    continue;
                }
                return Err(io::Error::new(io::ErrorKind::Other, "untrusted certificate chain"));
            }
            Err(TlsWrapError::Io(e)) => return Err(e),
        }
    }
}

fn expect(code: u16, want: u16, context: &str) -> io::Result<()> {
    if code == want {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::Other, format!("{context}: expected {want}, got {code}")))
    }
}

/// Injection guard shared by SMTP and IMAP STARTTLS: if the server (or a
/// MITM sitting in front of it) slipped extra plaintext bytes into the
/// same TCP segment as the STARTTLS response, BufReader will have
/// buffered them internally. Those bytes must never be treated as
/// post-handshake data — this is the exact shape of the STARTTLS
/// command-injection bugs found across several MTA/IMAP implementations
/// (circa 2011, and again in the 2021 multi-vendor disclosures).
/// BufReader::into_inner() silently drops any such buffered bytes, which
/// is the safe outcome, but we fail loudly instead of proceeding quietly,
/// since a non-empty buffer here is itself a tamper signal.
fn check_no_buffered_plaintext<S>(buf: &BufReader<S>) -> io::Result<()> {
    if buf.buffer().is_empty() {
        Ok(())
    } else {
        log::warn!("mail: unexpected data buffered before TLS handshake (possible STARTTLS injection)");
        Err(io::Error::new(
            io::ErrorKind::Other,
            "unexpected data buffered before TLS handshake (possible STARTTLS injection)",
        ))
    }
}

// =======================================================================
// SMTP
// =======================================================================

pub struct SmtpClient {
    stream: BufReader<TlsStream>,
}

/// Reads one SMTP response, following multi-line continuations
/// ("250-...\r\n" ... "250 ...\r\n"). Generic over the transport so the
/// same parser works pre- and post-STARTTLS.
fn read_smtp_response<S: Read>(r: &mut BufReader<S>) -> io::Result<(u16, String)> {
    let mut code: u16;
    let mut text = String::new();
    loop {
        let mut line = String::new();
        if r.read_line(&mut line)? == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "SMTP connection closed"));
        }
        if line.len() < 4 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "malformed SMTP line"));
        }
        code = line[0..3].parse().map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad code"))?;
        let cont = line.as_bytes()[3] == b'-';
        text.push_str(line[4..].trim_end());
        if !cont {
            break;
        }
        text.push('\n');
    }
    Ok((code, text))
}

fn smtp_command<S: Read + Write>(rw: &mut BufReader<S>, cmd: &str) -> io::Result<(u16, String)> {
    rw.get_mut().write_all(cmd.as_bytes())?;
    rw.get_mut().write_all(b"\r\n")?;
    read_smtp_response(rw)
}

impl SmtpClient {
    /// Implicit TLS connect (port 465). Consumes the 220 greeting.
    pub fn connect(host: &str, port: u16) -> io::Result<Self> {
        let tls_stream = connect_tls(host, port)?;
        let mut client = SmtpClient { stream: BufReader::new(tls_stream) };
        let (code, _) = client.read_response()?;
        expect(code, 220, "greeting")?;
        Ok(client)
    }

    /// STARTTLS connect (port 587, the common submission port). Speaks
    /// plaintext just long enough to negotiate TLS, then upgrades the
    /// same socket.
    pub fn connect_starttls(host: &str, port: u16, client_domain: &str) -> io::Result<Self> {
        loop {
            let sock = TcpStream::connect((host, port))?;
            let mut plain = BufReader::new(sock);

            let (code, _) = read_smtp_response(&mut plain)?;
            expect(code, 220, "greeting")?;

            let (code, _) = smtp_command(&mut plain, &format!("EHLO {client_domain}"))?;
            expect(code, 250, "EHLO")?;

            let (code, _) = smtp_command(&mut plain, "STARTTLS")?;
            expect(code, 220, "STARTTLS")?;

            check_no_buffered_plaintext(&plain)?;

            let sock = plain.into_inner();
            let tls = Tls::new();
            match wrap_tls(host, sock, tls.root_store()) {
                Ok(tls_stream) => {
                    let mut client = SmtpClient { stream: BufReader::new(tls_stream) };
                    // RFC 3207 requires discarding any EHLO capabilities
                    // learned before the handshake and re-issuing EHLO
                    // after it — the plaintext exchange was
                    // unauthenticated and could have been tampered with
                    // (e.g. a stripped AUTH mechanism list).
                    client.ehlo(client_domain)?;
                    return Ok(client);
                }
                Err(TlsWrapError::InvalidCertificate) => {
                    if retry_after_probe(&tls, host, port) {
                        continue;
                    }
                    return Err(io::Error::new(io::ErrorKind::Other, "untrusted certificate chain"));
                }
                Err(TlsWrapError::Io(e)) => return Err(e),
            }
        }
    }

    fn read_response(&mut self) -> io::Result<(u16, String)> { read_smtp_response(&mut self.stream) }

    fn command(&mut self, cmd: &str) -> io::Result<(u16, String)> { smtp_command(&mut self.stream, cmd) }

    pub fn ehlo(&mut self, client_domain: &str) -> io::Result<()> {
        let (code, _) = self.command(&format!("EHLO {client_domain}"))?;
        expect(code, 250, "EHLO")
    }

    /// AUTH LOGIN — fine to start with; prefer AUTH PLAIN or XOAUTH2 in
    /// production, and never log the credentials.
    pub fn auth_login(&mut self, user: &str, pass: &str) -> io::Result<()> {
        let (code, _) = self.command("AUTH LOGIN")?;
        expect(code, 334, "AUTH LOGIN")?;
        let (code, _) = self.command(&B64.encode(user))?;
        expect(code, 334, "AUTH LOGIN username")?;
        let (code, _) = self.command(&B64.encode(pass))?;
        expect(code, 235, "AUTH LOGIN password")
    }

    /// Sends one message. `body` should be full RFC 5322 content
    /// (headers + blank line + text); dot-stuffing is handled here.
    pub fn send(&mut self, from: &str, to: &[&str], body: &str) -> io::Result<()> {
        let (code, _) = self.command(&format!("MAIL FROM:<{from}>"))?;
        expect(code, 250, "MAIL FROM")?;
        for rcpt in to {
            let (code, _) = self.command(&format!("RCPT TO:<{rcpt}>"))?;
            expect(code, 250, "RCPT TO")?;
        }
        let (code, _) = self.command("DATA")?;
        expect(code, 354, "DATA")?;

        for line in body.lines() {
            if line.starts_with('.') {
                self.stream.get_mut().write_all(b".")?;
            }
            self.stream.get_mut().write_all(line.as_bytes())?;
            self.stream.get_mut().write_all(b"\r\n")?;
        }
        self.stream.get_mut().write_all(b".\r\n")?;

        let (code, _) = self.read_response()?;
        expect(code, 250, "end of DATA")
    }

    pub fn quit(&mut self) -> io::Result<()> { self.command("QUIT").map(|_| ()) }
}

// =======================================================================
// IMAP
// =======================================================================

/// One piece of a logical IMAP response line: either syntax/text, or the
/// raw payload of a `{n}` literal.
///
/// Kept as bytes rather than String because literal payloads (message
/// bodies, attachments) aren't guaranteed to be valid UTF-8 and must not
/// be re-parsed as IMAP syntax — treat `Literal` contents as opaque data.
#[derive(Debug)]
pub enum ImapChunk {
    Text(Vec<u8>),
    Literal(Vec<u8>),
}

impl ImapChunk {
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            ImapChunk::Text(b) | ImapChunk::Literal(b) => b,
        }
    }
}

/// Parses a trailing "{123}" or "{123+}" (LITERAL+ non-synchronizing
/// literal) marker off the end of a response fragment.
fn parse_literal_marker(line: &[u8]) -> Option<usize> {
    if line.last() != Some(&b'}') {
        return None;
    }
    let open = line.iter().rposition(|&b| b == b'{')?;
    let inner = &line[open + 1..line.len() - 1];
    let inner = inner.strip_suffix(b"+").unwrap_or(inner);
    std::str::from_utf8(inner).ok()?.parse::<usize>().ok()
}

/// Reads one full logical response line, following any `{n}` literal
/// embedded in it and reassembling the surrounding text as separate
/// chunks.
///
/// This is the piece that has to be right for the protocol to stay in
/// sync: a plain read_line() through a literal will hit a stray CRLF
/// inside binary/attachment content, treat it as a line break, and
/// desync every response after that point. Instead: read a fragment up
/// to CRLF; if it ends in "{n}", strip the marker, read exactly n raw
/// bytes with read_exact (binary-safe, ignores embedded CRLFs), then
/// keep reading — whatever follows the literal up to the next real CRLF
/// is a continuation of the same logical response (FETCH can carry more
/// than one literal, e.g. separate BODY[HEADER] and BODY[TEXT] parts).
fn read_logical_line<S: Read>(r: &mut BufReader<S>) -> io::Result<Vec<ImapChunk>> {
    let mut chunks = Vec::new();
    loop {
        let mut raw = Vec::new();
        let n = r.read_until(b'\n', &mut raw)?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "IMAP connection closed"));
        }
        while raw.last() == Some(&b'\n') || raw.last() == Some(&b'\r') {
            raw.pop();
        }

        if let Some(len) = parse_literal_marker(&raw) {
            let marker_start = raw.iter().rposition(|&b| b == b'{').unwrap();
            let text = raw[..marker_start].to_vec();
            if !text.is_empty() {
                chunks.push(ImapChunk::Text(text));
            }
            let mut literal = vec![0u8; len];
            r.read_exact(&mut literal)?;
            chunks.push(ImapChunk::Literal(literal));
            continue; // resume reading the tail of this same response
        }

        chunks.push(ImapChunk::Text(raw));
        return Ok(chunks);
    }
}

pub struct ImapClient {
    stream: BufReader<TlsStream>,
    tag: u32,
}

impl ImapClient {
    /// Implicit TLS connect (port 993). Consumes the "* OK ..." greeting.
    pub fn connect(host: &str, port: u16) -> io::Result<Self> {
        let tls_stream = connect_tls(host, port)?;
        let mut client = ImapClient { stream: BufReader::new(tls_stream), tag: 0 };
        client.expect_greeting()?;
        Ok(client)
    }

    /// STARTTLS connect (port 143).
    pub fn connect_starttls(host: &str, port: u16) -> io::Result<Self> {
        loop {
            let sock = TcpStream::connect((host, port))?;
            let mut plain = BufReader::new(sock);

            let mut greeting = String::new();
            plain.read_line(&mut greeting)?;
            if !greeting.starts_with("* OK") {
                return Err(io::Error::new(io::ErrorKind::Other, format!("unexpected greeting: {greeting}")));
            }

            let mut pre_tls_tag = 0u32;
            let (status, _) = imap_command(&mut plain, &mut pre_tls_tag, "STARTTLS")?;
            if !status.starts_with("OK") {
                return Err(io::Error::new(io::ErrorKind::Other, format!("STARTTLS refused: {status}")));
            }

            check_no_buffered_plaintext(&plain)?;

            let sock = plain.into_inner();
            let tls = Tls::new();
            match wrap_tls(host, sock, tls.root_store()) {
                Ok(tls_stream) => {
                    let mut client = ImapClient { stream: BufReader::new(tls_stream), tag: 0 };
                    // RFC 2595/3501: don't trust CAPABILITY learned before
                    // STARTTLS (e.g. LOGINDISABLED or an AUTH mechanism
                    // list could have been altered pre-handshake) — this
                    // re-fetches it fresh over the encrypted channel.
                    let _ = client.capability()?;
                    return Ok(client);
                }
                Err(TlsWrapError::InvalidCertificate) => {
                    if retry_after_probe(&tls, host, port) {
                        continue;
                    }
                    return Err(io::Error::new(io::ErrorKind::Other, "untrusted certificate chain"));
                }
                Err(TlsWrapError::Io(e)) => return Err(e),
            }
        }
    }

    fn expect_greeting(&mut self) -> io::Result<()> {
        let mut greeting = String::new();
        self.stream.read_line(&mut greeting)?;
        if !greeting.starts_with("* OK") {
            return Err(io::Error::new(io::ErrorKind::Other, format!("unexpected greeting: {greeting}")));
        }
        Ok(())
    }

    /// Full chunked response — use this when the command might return
    /// literals (FETCH).
    fn command(&mut self, cmd: &str) -> io::Result<(String, Vec<Vec<ImapChunk>>)> {
        imap_command(&mut self.stream, &mut self.tag, cmd)
    }

    /// Convenience wrapper for commands that never return literals
    /// (LOGIN, SELECT, LOGOUT, CAPABILITY) — flattens each response line
    /// to a lossy UTF-8 string.
    fn command_text(&mut self, cmd: &str) -> io::Result<(String, Vec<String>)> {
        let (status, untagged) = self.command(cmd)?;
        let lines = untagged
            .into_iter()
            .map(|chunks| {
                chunks.iter().map(|c| String::from_utf8_lossy(c.as_bytes()).into_owned()).collect::<String>()
            })
            .collect();
        Ok((status, lines))
    }

    fn expect_ok(status: &str) -> io::Result<()> {
        if status.starts_with("OK") {
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::Other, format!("IMAP error: {status}")))
        }
    }

    pub fn login(&mut self, user: &str, pass: &str) -> io::Result<()> {
        // NOTE: quote user/pass per IMAP string literal rules if they can
        // contain spaces, quotes, or backslashes; omitted here.
        let (status, _) = self.command_text(&format!("LOGIN \"{user}\" \"{pass}\""))?;
        Self::expect_ok(&status)
    }

    pub fn capability(&mut self) -> io::Result<Vec<String>> {
        let (status, untagged) = self.command_text("CAPABILITY")?;
        Self::expect_ok(&status)?;
        Ok(untagged)
    }

    pub fn select(&mut self, mailbox: &str) -> io::Result<Vec<String>> {
        let (status, untagged) = self.command_text(&format!("SELECT {mailbox}"))?;
        Self::expect_ok(&status)?;
        Ok(untagged)
    }

    /// e.g. fetch("1:5", "FLAGS BODY[HEADER.FIELDS (SUBJECT FROM DATE)]")
    /// or fetch("3", "BODY[]") for a full message including binary
    /// attachments — literal-safe either way.
    ///
    /// Returns the raw chunk structure rather than a parsed FETCH
    /// response; see the module-level docs for why.
    pub fn fetch(&mut self, seq: &str, items: &str) -> io::Result<Vec<Vec<ImapChunk>>> {
        let (status, untagged) = self.command(&format!("FETCH {seq} ({items})"))?;
        Self::expect_ok(&status)?;
        Ok(untagged)
    }

    pub fn logout(&mut self) -> io::Result<()> { self.command_text("LOGOUT").map(|_| ()) }
}

/// Free function so both the connected client and the pre-TLS STARTTLS
/// dialog can share it (the latter operates on `BufReader<TcpStream>`
/// before the struct even exists yet).
fn imap_command<S: Read + Write>(
    rw: &mut BufReader<S>,
    tag_counter: &mut u32,
    cmd: &str,
) -> io::Result<(String, Vec<Vec<ImapChunk>>)> {
    *tag_counter += 1;
    let tag = format!("a{tag_counter}");
    rw.get_mut().write_all(format!("{tag} {cmd}\r\n").as_bytes())?;

    let mut untagged = Vec::new();
    loop {
        let chunks = read_logical_line(rw)?;
        // The tag only ever appears as plain text at the start of a
        // response line (never itself inside a literal), so checking the
        // first Text chunk is sufficient.
        if let Some(ImapChunk::Text(first)) = chunks.first() {
            let first_str = String::from_utf8_lossy(first);
            if let Some(rest) = first_str.strip_prefix(&format!("{tag} ")) {
                return Ok((rest.to_string(), untagged));
            }
        }
        untagged.push(chunks);
    }
}
