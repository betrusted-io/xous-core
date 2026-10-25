pub trait SerialInteract {
    /// Receive a single new character.
    fn rx_char(&mut self, c: u8);

    /// Process characters received so far.
    fn process(&mut self);
}
