use core::fmt::{Error, Write};
use std::sync::mpsc::{channel, Receiver, Sender};

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

    Output {
        tx,
        rx,
        stdout: std::io::stdout(),
    }
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
                    }
                },
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(e) => panic!("Error: {}", e),
            }
        }
    }

    pub fn get_writer(&self) -> OutputWriter {
        OutputWriter {
            tx: self.tx.clone(),
        }
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        self.tx.send(ControlMessage::Exit).unwrap();
    }
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
    pub fn putc(&self, c: u8) {
        self.tx.send(ControlMessage::Byte(c)).unwrap();
    }

    /// Write a buffer to the output and return the number of
    /// bytes written. This is mostly compatible with `std::io::Write`,
    /// except it is infallible.
    pub fn write(&mut self, buf: &[u8]) -> usize {
        for c in buf {
            self.putc(*c);
        }
        buf.len()
    }

    pub fn write_all(&mut self, buf: &[u8]) -> core::result::Result<usize, ()> {
        Ok(self.write(buf))
    }
}

impl Write for OutputWriter {
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        // It would be nice if this worked with &str
        self.tx.send(ControlMessage::Text(s.to_owned())).unwrap();
        Ok(())
    }
}
