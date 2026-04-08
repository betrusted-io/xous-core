use core::fmt::{Error, Write};
#[cfg(feature = "hosted-dabao")]
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::mpsc::{Receiver, Sender, channel};

enum ControlMessage {
    Text(String),
    Byte(u8),
    Exit,
}

pub struct Output {
    tx: Sender<ControlMessage>,
    rx: Receiver<ControlMessage>,
    stdout: std::io::Stdout,
}

pub fn init() -> Output {
    let (tx, rx) = channel();

    Output { tx, rx, stdout: std::io::stdout() }
}

impl Output {
    pub fn run(&mut self) {
        use std::io::Write;
        loop {
            match self.rx.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok(msg) => match msg {
                    ControlMessage::Exit => break,
                    ControlMessage::Text(s) => print!("{}", s),
                    ControlMessage::Byte(s) => {
                        let mut handle = self.stdout.lock();
                        handle.write_all(&[s]).unwrap();
                        handle.flush().unwrap();
                    }
                },
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(e) => panic!("Error: {}", e),
            }
        }
    }

    pub fn get_writer(&self) -> OutputWriter { OutputWriter { tx: self.tx.clone() } }
}

impl Drop for Output {
    fn drop(&mut self) { self.tx.send(ControlMessage::Exit).unwrap(); }
}

impl Write for Output {
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        // It would be nice if this worked with &str
        self.tx.send(ControlMessage::Text(s.to_owned())).unwrap();
        Ok(())
    }
}

pub struct OutputWriter {
    tx: Sender<ControlMessage>,
}

impl OutputWriter {
    pub fn putc(&self, c: u8) { self.tx.send(ControlMessage::Byte(c)).unwrap(); }
}

impl Write for OutputWriter {
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        // It would be nice if this worked with &str
        self.tx.send(ControlMessage::Text(s.to_owned())).unwrap();
        Ok(())
    }
}

/// Spawn a background thread that reads stdin in raw mode and forwards
/// each character to the `keyboard_bouncer` server, mirroring the UART
/// IRQ handler path used on real hardware.
#[cfg(feature = "hosted-dabao")]
pub fn start_stdin_keyboard(echo: OutputWriter) { std::thread::spawn(move || stdin_keyboard_thread(echo)); }

/// Cached connection ID to keyboard_bouncer. 0 means "not yet connected"
/// (xous CIDs start at 1, so 0 is never a valid connection).
#[cfg(feature = "hosted-dabao")]
static KBD_CONN: AtomicU32 = AtomicU32::new(0);

/// RAII guard that restores terminal settings on drop (including panic).
#[cfg(feature = "hosted-dabao")]
struct RawModeGuard(libc::termios);

#[cfg(feature = "hosted-dabao")]
impl Drop for RawModeGuard {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.0);
        }
    }
}

#[cfg(feature = "hosted-dabao")]
fn stdin_keyboard_thread(echo: OutputWriter) {
    use std::io::Read;

    // Save original terminal settings and switch to raw mode so we get
    // characters one at a time without waiting for newline.
    let orig_termios = unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(libc::STDIN_FILENO, &mut t) != 0 {
            eprintln!("stdin-keyboard: tcgetattr failed, keyboard input disabled");
            return;
        }
        t
    };
    // Guard ensures terminal is restored on normal exit, early return, or panic.
    let _guard = RawModeGuard(orig_termios);
    let mut raw = orig_termios;
    unsafe {
        libc::cfmakeraw(&mut raw);
        // Keep ISIG so Ctrl-C still works, and OPOST so \n → \r\n
        // output translation is preserved for log output.
        raw.c_lflag |= libc::ISIG;
        raw.c_oflag |= libc::OPOST | libc::ONLCR;
        if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) != 0 {
            eprintln!("stdin-keyboard: tcsetattr failed, keyboard input disabled");
            return;
        }
    }

    let mut stdin = std::io::stdin().lock();
    let mut buf = [0u8; 1];
    loop {
        match stdin.read(&mut buf) {
            Ok(0) => break, // EOF
            Ok(_) => {
                let c = buf[0];
                if c == 0 {
                    continue;
                }

                // Local echo via the Output channel to avoid racing with log output
                if c == b'\r' || c == b'\n' {
                    echo.putc(b'\n');
                } else if c == 0x7f || c == 0x08 {
                    // Backspace: move back, overwrite with space, move back
                    for &b in b"\x08 \x08" {
                        echo.putc(b);
                    }
                } else if c >= 0x20 {
                    echo.putc(c);
                }

                // Lazily connect to keyboard_bouncer
                if KBD_CONN.load(Relaxed) == 0 {
                    match xous::try_connect(xous::SID::from_bytes(b"keyboard_bouncer").unwrap()) {
                        Ok(cid) => KBD_CONN.store(cid, Relaxed),
                        _ => continue, // server not ready yet, drop the char
                    }
                }
                let conn = KBD_CONN.load(Relaxed);
                xous::try_send_message(conn, xous::Message::new_scalar(0, c as usize, 0, 0, 0)).ok();
            }
            Err(e) => {
                eprintln!("stdin-keyboard: read error: {}", e);
                break;
            }
        }
    }
}
